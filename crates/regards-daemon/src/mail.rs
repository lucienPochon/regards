//! Reading mail from the store. Only the `ui` principal reaches these until
//! agent access levels exist (ADR-0012).

use crate::State;
use crate::sync;
use regards_core::mailbox::Role;
use regards_core::search::{Field, Query};
use regards_core::smart::{Screening, Section};
use regards_core::{body, html};
use regards_imap::SearchTerm;
use regards_ipc::protocol::{
    AccountRef, ConnectionLogSettings, MessageCursor, MessageDetail, MessageHtml, MessageHtmlParams, MessageListParams,
    MessageListResult, MessageRef, MessageSource, MessageSummary, ReadingSettings, RpcError, SearchParams,
    SearchResult, SenderImagesParams, SenderImagesResult, ServerSearchResult, ThreadEntry, ThreadView, View,
    ViewParams, codes,
};
use regards_store::mailboxes::Mailbox;
use regards_store::marks::{self, Mark, Marks};
use regards_store::messages::{self, Cursor, Message};
use regards_store::{StoreError, mailboxes};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::collections::hash_map::Entry;
use std::time::Duration;

const DEFAULT_PAGE: u32 = 50;
const MAX_PAGE: u32 = 500;
/// Larger messages are not downloaded yet (they need chunked FETCH).
const MAX_DOWNLOAD: u32 = 50 * 1024 * 1024;
/// Longer than any frame host needs.
const MAX_LINK_PREFIX: usize = 512;
/// Longer than anyone types.
const MAX_QUERY: usize = 1_000;

pub(crate) async fn list(state: &State, params: MessageListParams) -> Result<Value, RpcError> {
    let limit = params.limit.unwrap_or(DEFAULT_PAGE);
    if !(1..=MAX_PAGE).contains(&limit) {
        return Err(RpcError::new(
            codes::INVALID_PARAMS,
            format!("limit must be 1 to {MAX_PAGE}"),
        ));
    }
    let (mailbox_id, section) = (params.mailbox_id, params.section);
    let before = params.before.map(|c| Cursor {
        received_at: c.received_at,
        id: c.id,
    });
    let now = crate::compose::now_ms();
    let page = state
        .store
        .read(move |conn| {
            let Some(mailbox) = mailboxes::get(conn, mailbox_id)? else {
                return Ok(None);
            };
            // The inbox leaves out what is set aside or snoozed (plan §5.5–5.6).
            let parked = (mailbox.role == Some(Role::Inbox)).then_some((mailbox.account_id.as_str(), now));
            // One extra row tells whether another page exists.
            let rows = match parked {
                Some((account, now)) => {
                    let screening = regards_store::smart::screening(conn)?;
                    let (only, except) = shown_sections(section, screening);
                    let sections = messages::Sections { only: &only, except };
                    messages::inbox_page(conn, mailbox_id, limit + 1, before, account, now, &sections)?
                }
                None => messages::list(conn, mailbox_id, limit + 1, before)?,
            };
            let info = page_info(conn, &mailbox.account_id, &rows)?;
            Ok(Some((rows, info)))
        })
        .await
        .map_err(store_error)?;
    let Some((mut rows, info)) = page else {
        return Err(RpcError::new(codes::NOT_FOUND, "no such mailbox"));
    };
    let more = rows.len() > limit as usize;
    rows.truncate(limit as usize);
    let next = more.then(|| {
        let last = rows.last().expect("a full page is not empty");
        MessageCursor {
            received_at: last.listed_at,
            id: last.id,
        }
    });
    let messages = rows.into_iter().map(|row| summary(row, &info)).collect();
    Ok(json!(MessageListResult { messages, next }))
}

/// The marks of a page of messages.
/// What a page's summaries need beyond its rows: their marks (by Message-ID)
/// and the size of their conversations (by thread).
#[derive(Default)]
struct PageInfo {
    marks: HashMap<String, Marks>,
    threads: HashMap<i64, u32>,
}

fn page_info(conn: &rusqlite::Connection, account_id: &str, rows: &[Message]) -> Result<PageInfo, StoreError> {
    let ids: Vec<&str> = rows.iter().filter_map(|m| m.message_id.as_deref()).collect();
    let mut threads: Vec<i64> = rows.iter().filter_map(|m| m.thread_id).collect();
    threads.sort_unstable();
    threads.dedup();
    Ok(PageInfo {
        marks: marks::of(conn, account_id, &ids)?,
        threads: messages::thread_sizes(conn, &threads)?,
    })
}

/// An account's pinned, set-aside or snoozed messages (one per Message-ID), newest first.
pub(crate) async fn view(state: &State, params: ViewParams) -> Result<Value, RpcError> {
    let limit = params.limit.unwrap_or(DEFAULT_PAGE);
    if !(1..=MAX_PAGE).contains(&limit) {
        return Err(RpcError::new(
            codes::INVALID_PARAMS,
            format!("limit must be 1 to {MAX_PAGE}"),
        ));
    }
    let mark = match params.view {
        View::Pinned => Mark::Pinned,
        View::Aside => Mark::Aside,
        View::Snoozed => Mark::Snoozed,
        View::Reminders => Mark::Remind,
    };
    let before = params.before.map(|c| Cursor {
        received_at: c.received_at,
        id: c.id,
    });
    let now = crate::compose::now_ms();
    let account_id = params.account_id;
    let (mut rows, info) = state
        .store
        .read(move |conn| {
            let rows = messages::marked(conn, &account_id, mark, now, limit + 1, before)?;
            let info = page_info(conn, &account_id, &rows)?;
            Ok((rows, info))
        })
        .await
        .map_err(store_error)?;
    let more = rows.len() > limit as usize;
    rows.truncate(limit as usize);
    let next = more.then(|| {
        let last = rows.last().expect("a full page is not empty");
        MessageCursor {
            received_at: last.listed_at,
            id: last.id,
        }
    });
    let messages = rows.into_iter().map(|row| summary(row, &info)).collect();
    Ok(json!(MessageListResult { messages, next }))
}

fn store_error(error: StoreError) -> RpcError {
    crate::session::internal(error)
}

/// What a search asks for, checked: the page size, the words and filters,
/// and where the last page ended.
fn search_request(params: &SearchParams) -> Result<(u32, Query, Option<Cursor>), RpcError> {
    let limit = params.limit.unwrap_or(DEFAULT_PAGE);
    if !(1..=MAX_PAGE).contains(&limit) {
        return Err(RpcError::new(
            codes::INVALID_PARAMS,
            format!("limit must be 1 to {MAX_PAGE}"),
        ));
    }
    if params.query.len() > MAX_QUERY {
        return Err(RpcError::new(codes::INVALID_PARAMS, "the search is too long"));
    }
    let before = params.before.map(|c| Cursor {
        received_at: c.received_at,
        id: c.id,
    });
    Ok((limit, regards_core::search::parse(&params.query), before))
}

/// [`page_info`] for rows of any accounts: marks are kept by account.
fn page_info_any(conn: &rusqlite::Connection, rows: &[Message]) -> Result<PageInfo, StoreError> {
    let mut accounts: HashMap<i64, Option<String>> = HashMap::new();
    let mut by_account: HashMap<String, Vec<Message>> = HashMap::new();
    for message in rows {
        let account = match accounts.entry(message.mailbox_id) {
            Entry::Occupied(known) => known.get().clone(),
            Entry::Vacant(new) => new
                .insert(mailboxes::get(conn, message.mailbox_id)?.map(|m| m.account_id))
                .clone(),
        };
        if let Some(account) = account {
            by_account.entry(account).or_default().push(message.clone());
        }
    }
    let mut info = PageInfo::default();
    for (account, rows) in by_account {
        let more = page_info(conn, &account, &rows)?;
        info.marks.extend(more.marks);
        info.threads.extend(more.threads);
    }
    Ok(info)
}

/// Cut `page` (asked for one row more) to `limit`; where the next one starts.
fn cut(page: &mut Vec<Message>, limit: u32) -> Option<MessageCursor> {
    let more = page.len() > limit as usize;
    page.truncate(limit as usize);
    more.then(|| {
        let last = page.last().expect("a full page is not empty");
        MessageCursor {
            received_at: last.listed_at,
            id: last.id,
        }
    })
}

/// Search the mail (plan §5.10): one copy of each message, newest first; on
/// the first page of words that match few messages, the best matches too.
/// `scope`: only these accounts (an agent's), else the one asked for or all.
pub(crate) async fn search(state: &State, params: SearchParams, scope: Option<Vec<String>>) -> Result<Value, RpcError> {
    let (limit, query, before) = search_request(&params)?;
    let accounts = scope.or(params.account_id.map(|account| vec![account]));
    let input = params.query;
    let (hits, info, indexing, suggestion) = state
        .store
        .read(move |conn| {
            // One extra row tells whether another page exists.
            let hits = regards_store::search::search(conn, &query, accounts.as_deref(), limit + 1, before)?;
            let rows: Vec<Message> = hits.page.iter().chain(&hits.top).cloned().collect();
            let info = page_info_any(conn, &rows)?;
            // Nothing at all: perhaps a word was typed wrong.
            let suggestion = match (&query.text, before, hits.page.is_empty()) {
                (Some(_), None, true) => regards_store::search::suggest(conn, &input, accounts.as_deref())?,
                _ => None,
            };
            Ok((hits, info, regards_store::search::pending(conn)?, suggestion))
        })
        .await
        .map_err(|error| match error {
            StoreError::Conflict(message) => RpcError::new(codes::CONFLICT, message),
            other => store_error(other),
        })?;
    let mut page = hits.page;
    let next = cut(&mut page, limit);
    Ok(json!(SearchResult {
        messages: page.into_iter().map(|row| summary(row, &info)).collect(),
        top: hits.top.into_iter().map(|row| summary(row, &info)).collect(),
        next,
        indexing,
        suggestion,
    }))
}

/// Search the servers as well (plan §5.10): they have the text of messages
/// not opened on this device, which the index here lacks. What they find
/// that is on this device is listed as [`search`] lists it; the rest is
/// counted (older than what has been fetched so far, or just arrived).
pub(crate) async fn search_server(
    state: &State,
    params: SearchParams,
    scope: Option<Vec<String>>,
) -> Result<Value, RpcError> {
    let (limit, query, before) = search_request(&params)?;
    if query.is_empty() {
        return Ok(json!(ServerSearchResult {
            messages: Vec::new(),
            next: None,
            not_here: 0,
            unreachable: Vec::new(),
        }));
    }
    let terms = server_terms(&query);
    let (only, folder) = (
        scope.or(params.account_id.map(|account| vec![account])),
        query.folder.clone(),
    );
    // Each account's folders to ask, and the folders results may show in.
    let (plan, shown) = state
        .store
        .read(move |conn| {
            let accounts: Vec<String> = match only {
                Some(accounts) => accounts,
                None => regards_store::accounts::list(conn)?.into_iter().map(|a| a.id).collect(),
            };
            let mut plan: Vec<(String, Vec<Mailbox>)> = Vec::new();
            let mut shown: HashMap<i64, Option<Role>> = HashMap::new();
            for account in accounts {
                let ids =
                    regards_store::search::folders(conn, Some(std::slice::from_ref(&account)), folder.as_deref())?;
                let mut asked: Vec<Mailbox> = mailboxes::list(conn, &account)?
                    .into_iter()
                    .filter(|m| ids.contains(&m.id))
                    .collect();
                shown.extend(asked.iter().map(|m| (m.id, m.role)));
                asked.retain(|m| m.selectable && m.uid_validity.is_some());
                // All Mail holds what the other folders do, but Trash and Junk: one search.
                if folder.is_none()
                    && let Some(all) = asked.iter().find(|m| m.role == Some(Role::All)).cloned()
                {
                    asked = vec![all];
                }
                plan.push((account, asked));
            }
            Ok((plan, shown))
        })
        .await
        .map_err(store_error)?;

    let mut found: Vec<(String, Message)> = Vec::new();
    let (mut not_here, mut unreachable) = (0, Vec::new());
    for (account, folders) in plan {
        if folders.is_empty() {
            continue;
        }
        match ask_server(state, &account, &folders, &terms).await {
            Ok((rows, missing)) => {
                found.extend(rows.into_iter().map(|row| (account.clone(), row)));
                not_here += missing;
            }
            Err(error) => {
                tracing::info!(account = %account, %error, "the server was not searched");
                unreachable.push(account);
            }
        }
    }

    let has_attachment = query.has_attachment;
    let (page, next, info) = state
        .store
        .read(move |conn| {
            // One copy of each message, its inbox copy first, as the index search shows them.
            let rank = |m: &Message| {
                let role = shown.get(&m.moving_to.unwrap_or(m.mailbox_id)).copied().flatten();
                (role != Some(Role::Inbox), role == Some(Role::All), m.id)
            };
            let mut best: HashMap<(String, String), Message> = HashMap::new();
            for (account, row) in found {
                let copies = match &row.message_id {
                    Some(id) => messages::copies(conn, &account, id)?,
                    None => vec![row],
                };
                for copy in copies {
                    let place = copy.moving_to.unwrap_or(copy.mailbox_id);
                    if copy.flags.deleted
                        || !shown.contains_key(&place)
                        || (has_attachment && copy.has_attachments != Some(true))
                    {
                        continue;
                    }
                    let key = copy.message_id.clone().unwrap_or_else(|| format!("#{}", copy.id));
                    match best.entry((account.clone(), key)) {
                        Entry::Vacant(new) => {
                            new.insert(copy);
                        }
                        Entry::Occupied(mut known) => {
                            if rank(&copy) < rank(known.get()) {
                                known.insert(copy);
                            }
                        }
                    }
                }
            }
            let mut page: Vec<Message> = best
                .into_values()
                .filter(|m| before.is_none_or(|c| (m.received_at, m.id) < (c.received_at, c.id)))
                .collect();
            page.sort_by_key(|m| std::cmp::Reverse((m.received_at, m.id)));
            let next = cut(&mut page, limit);
            let info = page_info_any(conn, &page)?;
            Ok((page, next, info))
        })
        .await
        .map_err(store_error)?;
    Ok(json!(ServerSearchResult {
        messages: page.into_iter().map(|row| summary(row, &info)).collect(),
        next,
        not_here,
        unreachable,
    }))
}

/// Search `folders` of `account` on its server: the matches stored here,
/// and how many are not.
async fn ask_server(
    state: &State,
    account: &str,
    folders: &[Mailbox],
    terms: &[SearchTerm],
) -> Result<(Vec<Message>, u32), sync::SyncError> {
    let mut session = sync::login(state, account).await?;
    let outcome = async {
        let (mut rows, mut missing) = (Vec::new(), 0);
        for folder in folders {
            let selected = session.select(&folder.remote_name).await?;
            // Rebuilt on the server: its UIDs mean nothing here until it is synced again.
            if folder.uid_validity != Some(i64::from(selected.uid_validity)) {
                continue;
            }
            let uids = session.uid_search(terms).await?;
            let (id, asked) = (folder.id, uids.len());
            let stored = state.store.read(move |conn| messages::by_uids(conn, id, &uids)).await?;
            missing += asked - stored.len();
            rows.extend(stored);
        }
        Ok::<_, sync::SyncError>((rows, u32::try_from(missing).unwrap_or(u32::MAX)))
    }
    .await;
    session.logout().await;
    outcome
}

/// The search as IMAP terms. `has:` and `in:` are not among them: they are
/// checked here. Servers match words their own way, often as parts of words.
fn server_terms(query: &Query) -> Vec<SearchTerm> {
    let mut terms: Vec<SearchTerm> = query
        .terms
        .iter()
        .map(|term| {
            let text = term.text.clone();
            let search = match term.field {
                None => SearchTerm::Text(text),
                Some(Field::Sender) => SearchTerm::From(text),
                Some(Field::Recipients) => SearchTerm::To(text),
                Some(Field::Subject) => SearchTerm::Subject(text),
            };
            if term.negated {
                SearchTerm::Not(Box::new(search))
            } else {
                search
            }
        })
        .collect();
    match query.unread {
        Some(true) => terms.push(SearchTerm::Unseen),
        Some(false) => terms.push(SearchTerm::Seen),
        None => {}
    }
    if query.flagged {
        terms.push(SearchTerm::Flagged);
    }
    const DAY: i64 = 86_400_000;
    if let Some(before) = query.before {
        terms.push(SearchTerm::Before(before.div_euclid(DAY)));
    }
    if let Some(after) = query.after {
        terms.push(SearchTerm::Since(after.div_euclid(DAY)));
    }
    terms
}

/// One message with its body. The source is downloaded the first time and
/// kept under `blobs/`; afterwards it opens without contacting the server.
pub(crate) async fn get(state: &State, params: MessageRef) -> Result<Value, RpcError> {
    let (message, source) = load(state, params.id).await?;
    let info = message_info(state, &message).await?;
    let images_from_sender = sender_images(state, &message).await?;
    let (body, remote_images) = tokio::task::spawn_blocking(move || {
        let body = body::parse(&source);
        let remote = if body.has_html {
            html::render(&source, false, None).map_or(0, |rendered| rendered.blocked_remote)
        } else {
            0
        };
        (body, remote)
    })
    .await
    .map_err(crate::session::internal)?;
    Ok(json!(MessageDetail {
        mailbox_id: message.moving_to.unwrap_or(message.mailbox_id),
        to: message.to.clone(),
        cc: message.cc.clone(),
        text: body.text,
        has_html: body.has_html,
        remote_images,
        images_from_sender,
        attachments: body.attachments,
        message: summary(message, &info),
    }))
}

/// The daemon's key for the connection log: "on", or off.
pub(crate) const CONNECTION_LOG: &str = "support.connection_log";

/// The connection log, as the setting says: at start, and when it changes.
pub(crate) fn apply_connection_log(state: &State, on: bool) {
    let path = on.then(|| state.dirs.connection_log());
    if let Err(error) = regards_net::connlog::set(path) {
        tracing::warn!(%error, "could not open the connection log");
    }
}

/// `support.connection_log`.
pub(crate) fn connection_log(_state: &State) -> Value {
    json!(ConnectionLogSettings {
        on: regards_net::connlog::enabled(),
    })
}

/// `support.set_connection_log`: at once.
pub(crate) async fn set_connection_log(state: &State, params: ConnectionLogSettings) -> Result<Value, RpcError> {
    let value = if params.on { "on" } else { "off" };
    state
        .store
        .write(move |tx| regards_store::settings::set(tx, CONNECTION_LOG, value))
        .await
        .map_err(store_error)?;
    apply_connection_log(state, params.on);
    tracing::info!(on = params.on, "connection log");
    Ok(connection_log(state))
}

/// The daemon's key for [`ReadingSettings::clean_links`].
const CLEAN_LINKS: &str = "clean_links";

async fn reading(state: &State) -> Result<ReadingSettings, RpcError> {
    let value = state
        .store
        .read(|conn| regards_store::settings::get(conn, CLEAN_LINKS))
        .await
        .map_err(store_error)?;
    Ok(ReadingSettings {
        clean_links: value.as_deref() != Some("off"),
    })
}

/// `reading.settings`.
pub(crate) async fn reading_settings(state: &State) -> Result<Value, RpcError> {
    Ok(json!(reading(state).await?))
}

/// `reading.set_settings`.
pub(crate) async fn set_reading_settings(state: &State, params: ReadingSettings) -> Result<Value, RpcError> {
    let value = if params.clean_links { "on" } else { "off" };
    state
        .store
        .write(move |tx| regards_store::settings::set(tx, CLEAN_LINKS, value))
        .await
        .map_err(store_error)?;
    Ok(json!(params))
}

/// Whether the person lets the message's sender's remote images load.
async fn sender_images(state: &State, message: &Message) -> Result<bool, RpcError> {
    let Some(address) = message.from.as_ref().and_then(|from| from.address.clone()) else {
        return Ok(false);
    };
    let mailbox_id = message.mailbox_id;
    state
        .store
        .read(move |conn| {
            let Some(mailbox) = mailboxes::get(conn, mailbox_id)? else {
                return Ok(false);
            };
            regards_store::images::allowed(conn, &mailbox.account_id, &address)
        })
        .await
        .map_err(store_error)
}

/// The sender list of [`methods::SENDER_SET_IMAGES`], one more or less.
pub(crate) async fn set_sender_images(state: &State, params: SenderImagesParams) -> Result<Value, RpcError> {
    let address = params.address.trim().to_owned();
    if !address.contains('@') || address.len() > 320 {
        return Err(RpcError::new(codes::INVALID_PARAMS, "that is not an address"));
    }
    let (account_id, allow) = (params.account_id, params.allow);
    state
        .store
        .write(move |tx| {
            if regards_store::accounts::get(tx, &account_id)?.is_none() {
                return Err(StoreError::Conflict("no such account".into()));
            }
            regards_store::images::set(tx, &account_id, &address, allow)
        })
        .await
        .map_err(|error| match error {
            StoreError::Conflict(message) => RpcError::new(codes::NOT_FOUND, message),
            other => store_error(other),
        })?;
    Ok(json!({ "allow": allow }))
}

pub(crate) async fn sender_images_list(state: &State, params: AccountRef) -> Result<Value, RpcError> {
    let account_id = params.account_id;
    let senders = state
        .store
        .read(move |conn| regards_store::images::list(conn, &account_id))
        .await
        .map_err(store_error)?;
    Ok(json!(SenderImagesResult { senders }))
}

/// What one message's summary needs.
async fn message_info(state: &State, message: &Message) -> Result<PageInfo, RpcError> {
    let (mailbox_id, row) = (message.mailbox_id, message.clone());
    state
        .store
        .read(move |conn| {
            let Some(mailbox) = mailboxes::get(conn, mailbox_id)? else {
                return Ok(PageInfo::default());
            };
            page_info(conn, &mailbox.account_id, std::slice::from_ref(&row))
        })
        .await
        .map_err(store_error)
}

/// The sanitised HTML version of a message.
pub(crate) async fn html(state: &State, params: MessageHtmlParams) -> Result<Value, RpcError> {
    if params
        .link_prefix
        .as_ref()
        .is_some_and(|prefix| prefix.len() > MAX_LINK_PREFIX)
    {
        return Err(RpcError::new(codes::INVALID_PARAMS, "link_prefix is too long"));
    }
    let (message, source) = load(state, params.id).await?;
    let MessageHtmlParams {
        allow_remote,
        link_prefix,
        ..
    } = params;
    let allow_remote = allow_remote || sender_images(state, &message).await?;
    let clean_links = reading(state).await?.clean_links;
    let rendered = tokio::task::spawn_blocking(move || {
        html::render_with(&source, allow_remote, link_prefix.as_deref(), clean_links)
    })
    .await
    .map_err(crate::session::internal)?
    .ok_or_else(|| RpcError::new(codes::NOT_FOUND, "this message has no HTML version"))?;
    Ok(json!(MessageHtml {
        html: rendered.html,
        blocked_remote: rendered.blocked_remote,
    }))
}

/// `message.source`: the message as it came, its first megabyte at most.
pub(crate) async fn source(state: &State, params: MessageRef) -> Result<Value, RpcError> {
    let (_, source) = load(state, params.id).await?;
    Ok(json!(source_view(&source)))
}

fn source_view(source: &[u8]) -> MessageSource {
    let shown = &source[..source.len().min(MessageSource::MAX)];
    MessageSource {
        text: String::from_utf8_lossy(shown).into_owned(),
        truncated: shown.len() < source.len(),
        size: source.len() as u64,
    }
}

/// A message and its source, downloading the source the first time.
pub(crate) async fn load(state: &State, id: i64) -> Result<(Message, Vec<u8>), RpcError> {
    let mut retried = false;
    loop {
        let (message, mailbox) = find(state, id)
            .await?
            .ok_or_else(|| RpcError::new(codes::NOT_FOUND, "no such message"))?;
        let stored = match message.blob_hash.clone() {
            Some(hash) => {
                let blobs = state.blobs.clone();
                tokio::task::spawn_blocking(move || blobs.get(&hash))
                    .await
                    .map_err(crate::session::internal)?
                    .map_err(crate::session::internal)?
            }
            None => None,
        };
        if let Some(source) = stored {
            return Ok((message, source));
        }
        match download(state, &message, &mailbox).await {
            // The server may have just moved it, and we are about to learn its new UID.
            Err(error) if error.code == codes::NOT_FOUND && message.moving_to.is_some() && !retried => {
                retried = true;
                wait_for_move(state, &message).await?;
            }
            outcome => return outcome.map(|source| (message, source)),
        }
    }
}

async fn find(state: &State, id: i64) -> Result<Option<(Message, Mailbox)>, RpcError> {
    state
        .store
        .read(move |conn| {
            let Some(message) = messages::get(conn, id)? else {
                return Ok(None);
            };
            Ok(mailboxes::get(conn, message.mailbox_id)?.map(|mailbox| (message, mailbox)))
        })
        .await
        .map_err(store_error)
}

/// Wait a little for a pending move of `message` to be recorded.
async fn wait_for_move(state: &State, message: &Message) -> Result<(), RpcError> {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(3);
    while tokio::time::Instant::now() < deadline {
        tokio::time::sleep(Duration::from_millis(100)).await;
        let now = find(state, message.id).await?;
        let moved = now.is_none_or(|(row, _)| (row.mailbox_id, row.uid) != (message.mailbox_id, message.uid));
        if moved {
            break;
        }
    }
    Ok(())
}

async fn download(state: &State, message: &Message, mailbox: &Mailbox) -> Result<Vec<u8>, RpcError> {
    if message.size.is_some_and(|size| size > MAX_DOWNLOAD) {
        return Err(RpcError::new(
            codes::TOO_LARGE,
            "this message is larger than 50 MB and cannot be opened yet",
        ));
    }
    let failed = |error: &dyn std::fmt::Display| {
        RpcError::new(codes::SYNC_FAILED, format!("could not download the message: {error}"))
    };
    let mut session = sync::login(state, &mailbox.account_id).await.map_err(|e| failed(&e))?;
    let outcome = source_from(&mut session, message, mailbox).await;
    session.logout().await;
    let Some(source) = outcome.map_err(|e| failed(&e))? else {
        return Err(RpcError::new(
            codes::NOT_FOUND,
            "the server no longer has this message; it disappears after the next sync",
        ));
    };
    keep_source(state, message, mailbox, source).await
}

/// A message's source from the server, if it still has it under the same UIDVALIDITY.
async fn source_from(
    session: &mut regards_imap::Session,
    message: &Message,
    mailbox: &Mailbox,
) -> Result<Option<Vec<u8>>, regards_imap::ImapError> {
    let selected = session.select(&mailbox.remote_name).await?;
    // A different UIDVALIDITY means the UID may now name another message.
    if mailbox.uid_validity != Some(i64::from(selected.uid_validity)) {
        return Ok(None);
    }
    session.fetch_source(message.uid).await
}

/// Download message `id` with an open session, unless it is downloaded already
/// (prefetching); whether it was.
pub(crate) async fn fetch_with(state: &State, session: &mut regards_imap::Session, id: i64) -> Result<bool, RpcError> {
    let Some((message, mailbox)) = find(state, id).await? else {
        return Ok(false);
    };
    if message.blob_hash.is_some() || message.size.is_some_and(|size| size > MAX_DOWNLOAD) {
        return Ok(false);
    }
    let source = source_from(session, &message, &mailbox)
        .await
        .map_err(|error| RpcError::new(codes::SYNC_FAILED, error.to_string()))?;
    let Some(source) = source else { return Ok(false) };
    keep_source(state, &message, &mailbox, source).await?;
    Ok(true)
}

/// Keep a downloaded source in blobs/, and its whole text searchable.
async fn keep_source(
    state: &State,
    message: &Message,
    mailbox: &Mailbox,
    source: Vec<u8>,
) -> Result<Vec<u8>, RpcError> {
    let blobs = state.blobs.clone();
    let copy = source.clone();
    // Kept, and its whole text searchable from now on.
    let (hash, text) = tokio::task::spawn_blocking(move || (blobs.put(&copy), body::parse(&copy).text))
        .await
        .map_err(crate::session::internal)?;
    let hash = hash.map_err(crate::session::internal)?;
    let (id, account, message_id, now) = (
        message.id,
        mailbox.account_id.clone(),
        message.message_id.clone(),
        crate::compose::now_ms(),
    );
    state
        .store
        .write(move |tx| {
            messages::set_blob(tx, id, &hash)?;
            regards_store::search::set_text(tx, &account, id, message_id.as_deref(), &text, now)
        })
        .await
        .map_err(store_error)?;
    Ok(source)
}

/// A message as lists show it, with its marks (by Message-ID).
/// Recipients a summary carries: a list row shows two and a count.
const SUMMARY_RECIPIENTS: usize = 3;

fn summary(m: Message, info: &PageInfo) -> MessageSummary {
    let mark = m
        .message_id
        .as_ref()
        .and_then(|id| info.marks.get(id))
        .copied()
        .unwrap_or_default();
    MessageSummary {
        mailbox_id: m.moving_to.unwrap_or(m.mailbox_id),
        pinned: mark.pinned_at.is_some(),
        aside: mark.aside_at.is_some(),
        snoozed_until: mark.snoozed_until,
        remind_at: mark.remind_at,
        reminded: mark.reminded_at.is_some(),
        section: m.section,
        listed_at: m.listed_at,
        thread_size: m
            .thread_id
            .and_then(|thread| info.threads.get(&thread))
            .copied()
            .unwrap_or(1),
        has_attachments: m.has_attachments.unwrap_or(false),
        preview: m.preview,
        id: m.id,
        uid: m.uid,
        subject: m.subject,
        to: m.to.into_iter().chain(m.cc).take(SUMMARY_RECIPIENTS).collect(),
        from: m.from,
        received_at: m.received_at,
        sent_at: m.sent_at,
        seen: m.flags.seen,
        flagged: m.flags.flagged,
        answered: m.flags.answered,
        size: m.size,
    }
}

/// The conversation of a message, across folders.
pub(crate) async fn thread(state: &State, params: MessageRef) -> Result<Value, RpcError> {
    let id = params.id;
    let entries = state
        .store
        .read(move |conn| {
            let Some(message) = messages::get(conn, id)? else {
                return Ok(None);
            };
            let rows = match message.thread_id {
                Some(thread) => messages::in_thread(conn, thread)?,
                None => vec![message],
            };
            let mut folders: HashMap<i64, Option<Mailbox>> = HashMap::new();
            let mut entries = Vec::new();
            for row in rows {
                // A message on its way to another folder is shown there.
                let shown_in = row.moving_to.unwrap_or(row.mailbox_id);
                let folder = match folders.entry(shown_in) {
                    Entry::Occupied(known) => known.into_mut(),
                    Entry::Vacant(new) => new.insert(mailboxes::get(conn, shown_in)?),
                };
                let Some(folder) = folder else {
                    continue;
                };
                let info = page_info(conn, &folder.account_id, std::slice::from_ref(&row))?;
                entries.push(ThreadEntry {
                    mailbox_id: folder.id,
                    mailbox_name: folder.name.clone(),
                    mailbox_role: folder.role,
                    message: summary(row, &info),
                });
            }
            Ok(Some(entries))
        })
        .await
        .map_err(store_error)?;
    let messages = entries.ok_or_else(|| RpcError::new(codes::NOT_FOUND, "no such message"))?;
    Ok(json!(ThreadView { messages }))
}

/// Thread the messages stored before threading existed, a batch at a time so
/// other writes are not held up.
pub(crate) async fn thread_backfill(state: std::sync::Arc<State>) {
    match state.store.read(regards_store::threads::any_missing).await {
        Ok(true) => {}
        Ok(false) => return,
        Err(error) => {
            tracing::warn!(%error, "checking for unthreaded messages failed");
            return;
        }
    }
    let mut total = 0;
    loop {
        match state
            .store
            .write(|tx| regards_store::threads::assign_missing(tx, 500))
            .await
        {
            Ok(done) if done.value == 0 => break,
            Ok(done) => total += done.value,
            Err(error) => {
                tracing::warn!(%error, "threading stored messages failed");
                return;
            }
        }
    }
    if total > 0 {
        tracing::info!(messages = total, "threaded stored messages");
    }
}

/// Which sections an inbox list shows, for `asked` (all when none) under a
/// screening mode: off folds new senders into People, hold keeps them out
/// of everything but their own section.
fn shown_sections(asked: Option<Section>, screening: Screening) -> (Vec<Section>, Option<Section>) {
    match (asked, screening) {
        (None, Screening::Hold) => (Vec::new(), Some(Section::NewSenders)),
        (None, _) => (Vec::new(), None),
        (Some(Section::People), Screening::Off) => (vec![Section::People, Section::NewSenders], None),
        (Some(section), _) => (vec![section], None),
    }
}

/// A message's summary, without its page's marks and conversation sizes.
pub(crate) fn plain_summary(m: Message) -> MessageSummary {
    summary(m, &PageInfo::default())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_source_is_shown_as_text_up_to_a_megabyte() {
        let small = source_view(b"Subject: Hi\r\n\r\nCaf\xc3\xa9 \xff");
        assert_eq!(small.text, "Subject: Hi\r\n\r\nCafé \u{fffd}");
        assert!(!small.truncated);
        assert_eq!(small.size, 22);
        let big = vec![b'a'; MessageSource::MAX + 10];
        let cut = source_view(&big);
        assert!(cut.truncated);
        assert_eq!((cut.text.len(), cut.size), (MessageSource::MAX, big.len() as u64));
    }
}
