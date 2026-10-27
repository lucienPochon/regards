//! Writing and sending mail (Phase 2): drafts, replies and forwards, and the
//! outbox's user-facing side. Sending itself is the outbox runner's job
//! ([`crate::outbox`]). Only the `ui` principal reaches these for now; agents
//! get them through access levels and holds (ADR-0012).

use crate::State;
use regards_core::compose::{self, Outgoing};
use regards_core::executor::Executor;
use regards_core::headers::{self, Person};
use regards_ipc::protocol::{
    DraftAttachFileParams, DraftAttachImageParams, DraftAttachment, DraftCheckResult, DraftCreateParams,
    DraftDetachParams, DraftForwardParams, DraftImage, DraftImagesResult, DraftListParams, DraftListResult, DraftRef,
    DraftReplyParams, DraftSendParams, DraftSendResult, DraftSetFromParams, DraftUpdateParams, DraftView, MessageRef,
    OutboxEntryView, OutboxListResult, OutboxRef, OutboxRescheduleParams, OutboxState, OutboxUndoResult, RpcError,
    codes,
};
use regards_store::drafts::{self, Content, Draft, Edit};
use regards_store::identities::{self, Identity};
use regards_store::marks::{self, Mark};
use regards_store::notices::{self, Kind, NewNotice};
use regards_store::outbox::{self, Entry, NewEntry, State as EntryState};
use regards_store::{StoreError, accounts, mailboxes, messages};
use rusqlite::Transaction;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

/// ADR-0010 §4: undo send for 10 s by default, at most 30.
const DEFAULT_UNDO_SECONDS: u32 = 10;
const MAX_UNDO_SECONDS: u32 = 60;
/// More recipients than this is a mailing, not a message.
const MAX_RECIPIENTS: usize = 100;
/// Bodies larger than this are not a typed message.
const MAX_BODY: usize = 2 * 1024 * 1024;
/// Most mail servers take messages up to about 25 MB (attachments grow by a third in transit).
const MAX_ATTACHMENTS: u64 = 25 * 1024 * 1024;
/// An image in the text: at most this big …
const MAX_IMAGE: u64 = 5 * 1024 * 1024;
/// … and all of a draft's together, so they reach the editor in one piece.
const MAX_IMAGES: u64 = 10 * 1024 * 1024;
/// An image just added is kept this long even if a save does not have it yet.
const IMAGE_GRACE_MS: i64 = 60_000;
/// Images a message's text may carry.
const IMAGE_TYPES: &[&str] = &["image/png", "image/jpeg", "image/gif", "image/webp"];

pub(crate) async fn create(state: &State, params: DraftCreateParams) -> Result<Value, RpcError> {
    check_content(&params.to, &params.cc, &params.bcc, &params.body)?;
    let account_id = params.account_id.clone();
    let now = now_ms();
    let id = state
        .store
        .write(move |tx| {
            let account =
                accounts::get(tx, &account_id)?.ok_or_else(|| StoreError::Conflict("no such account".into()))?;
            let identity = identity_of(tx, &account, params.identity_id)?;
            let content = Content {
                to: params.to,
                cc: params.cc,
                bcc: params.bcc,
                subject: params.subject,
                body: format!("{}{}", params.body, compose::signature_block(&identity.signature)),
                identity_id: stored(&identity),
                ..Content::default()
            };
            drafts::create(tx, &account_id, &content, now)
        })
        .await
        .map_err(conflict)?
        .value;
    view(state, id).await
}

/// A reply, addressed (Reply-To, sender, everyone for reply all) and quoted.
pub(crate) async fn reply(state: &State, params: DraftReplyParams) -> Result<Value, RpcError> {
    reply_with(state, params, "").await
}

/// A reply with `written` already in it, above the signature and the quote.
pub(crate) async fn reply_with(state: &State, params: DraftReplyParams, written: &str) -> Result<Value, RpcError> {
    let (message, source) = crate::mail::load(state, params.message_id).await?;
    let mailbox_id = message.mailbox_id;
    let original = headers::parse(&source);
    // It goes out from the address it was written to, if that is one of the account's.
    let written_to: Vec<String> = original
        .to
        .iter()
        .chain(&original.cc)
        .filter_map(|person| person.address.clone())
        .collect();
    let (account, identity, mine) = state
        .store
        .read(move |conn| {
            let Some(mailbox) = mailboxes::get(conn, mailbox_id)? else {
                return Ok(None);
            };
            let Some(account) = accounts::get(conn, &mailbox.account_id)? else {
                return Ok(None);
            };
            let addresses: Vec<&str> = written_to.iter().map(String::as_str).collect();
            let identity = match identities::for_addresses(conn, &account.id, &addresses)? {
                Some(found) => found,
                None => identity_of(conn, &account, None)?,
            };
            let mut mine: Vec<String> = identities::list(conn, &account.id)?
                .into_iter()
                .map(|i| i.email)
                .collect();
            mine.push(account.email.clone());
            Ok(Some((account, identity, mine)))
        })
        .await
        .map_err(crate::session::internal)?
        .ok_or_else(|| RpcError::new(codes::NOT_FOUND, "the message's account no longer exists"))?;
    let text = regards_core::body::parse(&source).text;
    // None of the account's own addresses is answered.
    let me: Vec<&str> = mine.iter().map(String::as_str).collect();
    let (to, cc) = compose::reply_recipients(
        original.from.as_ref(),
        &original.reply_to,
        &original.to,
        &original.cc,
        &me,
        params.all,
    );
    let offset = params
        .utc_offset_minutes
        .filter(|offset| (-14 * 60..=14 * 60).contains(offset));
    let above = format!("{written}{}", compose::signature_block(&identity.signature));
    // Formatted mail is quoted formatted; the plain text goes with it.
    let html = regards_core::html::quotable(&source)
        .map(|quoted| compose::quote_html(&above, &quoted, original.from.as_ref(), original.date, offset));
    let content = Content {
        to,
        cc,
        bcc: Vec::new(),
        subject: compose::reply_subject(&original.subject),
        body: format!(
            "{above}{}",
            compose::quote_at(&text, original.from.as_ref(), original.date, offset)
        ),
        in_reply_to: original.message_id.clone(),
        references: compose::reply_references(&original.references, original.message_id.as_deref()),
        replying_to: Some(message.id),
        identity_id: stored(&identity),
        html,
    };
    store_new(state, account.id, content).await
}

/// An email asking a mailing list to drop the person: to the address its
/// `List-Unsubscribe` link names, with what the link asks it to say, from
/// the address the list writes to (as a reply would go). No signature.
pub(crate) async fn unsubscribe_draft(
    state: &State,
    message_id: i64,
    source: &[u8],
    mailto: regards_core::unsubscribe::Mailto,
) -> Result<i64, RpcError> {
    let original = headers::parse(source);
    let written_to: Vec<String> = original
        .to
        .iter()
        .chain(&original.cc)
        .filter_map(|person| person.address.clone())
        .collect();
    let now = now_ms();
    state
        .store
        .write(move |tx| {
            let message =
                messages::get(tx, message_id)?.ok_or_else(|| StoreError::Conflict("no such message".into()))?;
            let mailbox = mailboxes::get(tx, message.mailbox_id)?
                .ok_or_else(|| StoreError::Conflict("the folder no longer exists".into()))?;
            let account = accounts::get(tx, &mailbox.account_id)?
                .ok_or_else(|| StoreError::Conflict("the message's account no longer exists".into()))?;
            let addresses: Vec<&str> = written_to.iter().map(String::as_str).collect();
            let identity = match identities::for_addresses(tx, &account.id, &addresses)? {
                Some(found) => found,
                None => identity_of(tx, &account, None)?,
            };
            let content = Content {
                to: vec![Person {
                    name: None,
                    address: Some(mailto.to),
                }],
                subject: mailto.subject,
                body: mailto.body,
                identity_id: stored(&identity),
                ..Content::default()
            };
            drafts::create(tx, &account.id, &content, now)
        })
        .await
        .map_err(conflict)
        .map(|committed| committed.value)
}

/// A forward: the original's text and headers below, nobody addressed yet.
pub(crate) async fn forward(state: &State, params: DraftForwardParams) -> Result<Value, RpcError> {
    let offset = params
        .utc_offset_minutes
        .filter(|offset| (-14 * 60..=14 * 60).contains(offset));
    let (message, source) = crate::mail::load(state, params.id).await?;
    let mailbox_id = message.mailbox_id;
    let account_id = state
        .store
        .read(move |conn| Ok(mailboxes::get(conn, mailbox_id)?.map(|m| m.account_id)))
        .await
        .map_err(crate::session::internal)?
        .ok_or_else(|| RpcError::new(codes::NOT_FOUND, "the message's account no longer exists"))?;
    let original = headers::parse(&source);
    let formatted = regards_core::html::quotable(&source);
    // The original's files go too.
    let (text, files) = text_and_files(state, source).await?;
    let forwarded = compose::forwarded_at(
        &text,
        original.from.as_ref(),
        &original.to,
        &original.subject,
        original.date,
        offset,
    );
    let subject = compose::forward_subject(&original.subject);
    let now = now_ms();
    let id = state
        .store
        .write(move |tx| {
            let account =
                accounts::get(tx, &account_id)?.ok_or_else(|| StoreError::Conflict("no such account".into()))?;
            let identity = identity_of(tx, &account, None)?;
            let above = compose::signature_block(&identity.signature);
            // Formatted mail is forwarded formatted; the plain text goes with it.
            let html = formatted.map(|original_html| {
                compose::forwarded_html(
                    &above,
                    &original_html,
                    original.from.as_ref(),
                    &original.to,
                    &original.subject,
                    original.date,
                    offset,
                )
            });
            let content = Content {
                subject,
                body: format!("{above}{forwarded}"),
                identity_id: stored(&identity),
                html,
                ..Content::default()
            };
            let id = drafts::create(tx, &account_id, &content, now)?;
            for (hash, name, mime, size) in &files {
                drafts::attach(tx, id, hash, name, mime, *size)?;
            }
            Ok(id)
        })
        .await
        .map_err(conflict)?
        .value;
    view(state, id).await
}

/// "Send again": a new draft saying what a stored message said, to the same
/// people, from the address it went from (the default one when that is not
/// the account's any more), with its files, in its conversation. Nothing is
/// added: its signature is already in it.
pub(crate) async fn again(state: &State, params: MessageRef) -> Result<Value, RpcError> {
    let (message, source) = crate::mail::load(state, params.id).await?;
    let mailbox_id = message.mailbox_id;
    let original = headers::parse(&source);
    let formatted = regards_core::html::quotable(&source);
    let (text, files) = text_and_files(state, source).await?;
    let now = now_ms();
    let id = state
        .store
        .write(move |tx| {
            let mailbox = mailboxes::get(tx, mailbox_id)?
                .ok_or_else(|| StoreError::Conflict("the folder no longer exists".into()))?;
            let account = accounts::get(tx, &mailbox.account_id)?
                .ok_or_else(|| StoreError::Conflict("the message's account no longer exists".into()))?;
            let from: Vec<&str> = original
                .from
                .iter()
                .filter_map(|person| person.address.as_deref())
                .collect();
            let identity = match identities::for_addresses(tx, &account.id, &from)? {
                Some(found) => found,
                None => identity_of(tx, &account, None)?,
            };
            let content = Content {
                to: original.to,
                cc: original.cc,
                bcc: original.bcc,
                subject: original.subject,
                body: text,
                in_reply_to: original.in_reply_to.first().cloned(),
                references: original.references,
                identity_id: stored(&identity),
                html: formatted,
                ..Content::default()
            };
            let id = drafts::create(tx, &account.id, &content, now)?;
            for (hash, name, mime, size) in &files {
                drafts::attach(tx, id, hash, name, mime, *size)?;
            }
            Ok(id)
        })
        .await
        .map_err(conflict)?
        .value;
    view(state, id).await
}

async fn store_new(state: &State, account_id: String, content: Content) -> Result<Value, RpcError> {
    let now = now_ms();
    let id = state
        .store
        .write(move |tx| drafts::create(tx, &account_id, &content, now))
        .await
        .map_err(conflict)?
        .value;
    view(state, id).await
}

pub(crate) async fn get(state: &State, params: DraftRef) -> Result<Value, RpcError> {
    view(state, params.id).await
}

/// `draft.check`: what to ask about before it goes (plan §5.9).
pub(crate) async fn check(state: &State, params: DraftRef) -> Result<Value, RpcError> {
    let id = params.id;
    let found = state
        .store
        .read(move |conn| {
            let Some(draft) = drafts::get(conn, id)? else {
                return Ok(None);
            };
            Ok(Some((draft, drafts::attachments(conn, id)?.len())))
        })
        .await
        .map_err(crate::session::internal)?;
    let (draft, attachments) = found.ok_or_else(|| RpcError::new(codes::NOT_FOUND, "no such draft"))?;
    let recipients = draft
        .to
        .iter()
        .chain(&draft.cc)
        .chain(&draft.bcc)
        .filter(|person| {
            person
                .address
                .as_deref()
                .is_some_and(|address| !address.trim().is_empty())
        })
        .count();
    Ok(json!(DraftCheckResult {
        checks: compose::send_checks(recipients, &draft.subject, &draft.body, attachments),
    }))
}

/// The drafts of the account asked for, or all; `scope` (an agent's): of these accounts only.
pub(crate) async fn list(
    state: &State,
    params: DraftListParams,
    scope: Option<Vec<String>>,
) -> Result<Value, RpcError> {
    let account_id = params.account_id;
    let drafts = state
        .store
        .read(move |conn| {
            let accounts: HashMap<String, accounts::Account> = accounts::list(conn)?
                .into_iter()
                .map(|account| (account.id.clone(), account))
                .collect();
            let mut drafts = Vec::new();
            for draft in drafts::list(conn, account_id.as_deref())? {
                if scope.as_ref().is_some_and(|open| !open.contains(&draft.account_id)) {
                    continue;
                }
                let Some(account) = accounts.get(&draft.account_id) else {
                    continue;
                };
                let identity = identity_of(conn, account, draft.identity_id)?;
                let files = drafts::attachments(conn, draft.id)?;
                drafts.push(draft_view(draft, &identity, files));
            }
            Ok(drafts)
        })
        .await
        .map_err(crate::session::internal)?;
    Ok(json!(DraftListResult { drafts }))
}

/// Autosave: the whole of what the draft says.
pub(crate) async fn update(state: &State, params: DraftUpdateParams) -> Result<Value, RpcError> {
    check_content(&params.to, &params.cc, &params.bcc, &params.body)?;
    if params.html.as_ref().is_some_and(|html| html.len() > MAX_BODY) {
        return Err(RpcError::new(codes::TOO_LARGE, "the message text is too long"));
    }
    // Formatted text is sanitised here, once, and its plain version derived from it.
    let (body, html) = match &params.html {
        Some(written) => {
            let clean = regards_core::html::sanitize_outgoing(written);
            (regards_core::html::to_text(&clean), Some(clean))
        }
        None => (params.body.clone(), None),
    };
    let id = params.id;
    let now = now_ms();
    state
        .store
        .write(move |tx| {
            let edit = Edit {
                to: &params.to,
                cc: &params.cc,
                bcc: &params.bcc,
                subject: &params.subject,
                body: &body,
                html: html.as_deref(),
            };
            drafts::update(tx, params.id, &edit, now)?;
            // An image taken out of the text goes from the draft too, once
            // it has been there a while.
            drafts::drop_unused_images(tx, params.id, html.as_deref(), now - IMAGE_GRACE_MS)?;
            Ok(())
        })
        .await
        .map_err(conflict)?;
    view(state, id).await
}

/// `draft.delete`. What it said and its files are kept for a while, so the
/// deletion can be undone (`undo` in the answer); the files' bytes stay in
/// the blob store meanwhile.
pub(crate) async fn delete(state: &State, params: DraftRef) -> Result<Value, RpcError> {
    let (id, now) = (params.id, now_ms());
    let undo = state
        .store
        .write(move |tx| {
            let Some(kept) = keep_draft(tx, id)? else {
                return Ok(None);
            };
            if !drafts::delete(tx, id)? {
                return Ok(None);
            }
            crate::actions::record_draft_undo(tx, "Deleted a draft", kept, now).map(Some)
        })
        .await
        .map_err(conflict)?
        .value
        .ok_or_else(|| RpcError::new(codes::NOT_FOUND, "no such draft"))?;
    Ok(json!({ "deleted": true, "undo": undo }))
}

/// A draft as it is, to make it again if its deletion is undone; `None` if there is none.
fn keep_draft(tx: &Transaction<'_>, id: i64) -> Result<Option<KeptDraft>, StoreError> {
    let Some(draft) = drafts::get(tx, id)? else {
        return Ok(None);
    };
    let files = drafts::attachments(tx, id)?
        .into_iter()
        .map(|file| (file.blob_hash, file.name, file.mime, file.size, file.content_id))
        .collect();
    Ok(Some(KeptDraft {
        account_id: draft.account_id,
        to: draft.to,
        cc: draft.cc,
        bcc: draft.bcc,
        subject: draft.subject,
        body: draft.body,
        html: draft.html,
        in_reply_to: draft.in_reply_to,
        references: draft.references,
        replying_to: draft.replying_to,
        identity_id: draft.identity_id,
        files,
    }))
}

/// A deleted draft as it was, to make it again when its deletion is undone.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub(crate) struct KeptDraft {
    account_id: String,
    to: Vec<Person>,
    cc: Vec<Person>,
    bcc: Vec<Person>,
    subject: String,
    body: String,
    html: Option<String>,
    in_reply_to: Option<String>,
    references: Vec<String>,
    replying_to: Option<i64>,
    identity_id: Option<i64>,
    /// Blob, name, type, size, and Content-ID for an image in the text.
    files: Vec<(String, String, String, u64, Option<String>)>,
}

/// Make a deleted draft again (a new id); `None` if its account has gone since.
pub(crate) fn restore_draft(tx: &Transaction<'_>, kept: &KeptDraft, now: i64) -> Result<Option<i64>, StoreError> {
    if accounts::get(tx, &kept.account_id)?.is_none() {
        return Ok(None);
    }
    // An identity or answered message deleted since is left out.
    let identity_id = match kept.identity_id {
        Some(id) => identities::get(tx, id)?.map(|identity| identity.id),
        None => None,
    };
    let replying_to = match kept.replying_to {
        Some(id) => messages::get(tx, id)?.map(|message| message.id),
        None => None,
    };
    let content = Content {
        to: kept.to.clone(),
        cc: kept.cc.clone(),
        bcc: kept.bcc.clone(),
        subject: kept.subject.clone(),
        body: kept.body.clone(),
        html: kept.html.clone(),
        in_reply_to: kept.in_reply_to.clone(),
        references: kept.references.clone(),
        replying_to,
        identity_id,
    };
    let id = drafts::create(tx, &kept.account_id, &content, now)?;
    for (hash, name, mime, size, content_id) in &kept.files {
        match content_id {
            Some(cid) => drafts::attach_inline(tx, id, (hash, name, mime, *size), cid)?,
            None => drafts::attach(tx, id, hash, name, mime, *size)?,
        };
    }
    Ok(Some(id))
}

/// Attach a file the user chose. The daemon reads it once into `blobs/`;
/// after that the draft no longer depends on the file.
pub(crate) async fn attach_file(state: &State, params: DraftAttachFileParams) -> Result<Value, RpcError> {
    let path = std::path::PathBuf::from(&params.path);
    if !path.is_absolute() {
        return Err(RpcError::new(codes::INVALID_PARAMS, "give the file's full path"));
    }
    let draft_id = params.draft_id;
    let current: u64 = state
        .store
        .read(move |conn| Ok(drafts::attachments(conn, draft_id)?.iter().map(|a| a.size).sum()))
        .await
        .map_err(crate::session::internal)?;
    let blobs = state.blobs.clone();
    let read = tokio::task::spawn_blocking(move || -> Result<(String, String, u64), RpcError> {
        let metadata = std::fs::metadata(&path)
            .map_err(|error| RpcError::new(codes::NOT_FOUND, format!("cannot open the file: {error}")))?;
        if !metadata.is_file() {
            return Err(RpcError::new(codes::INVALID_PARAMS, "that is not a file"));
        }
        if current + metadata.len() > MAX_ATTACHMENTS {
            return Err(RpcError::new(
                codes::TOO_LARGE,
                format!(
                    "attachments are limited to {} MB per message, which most mail servers accept",
                    MAX_ATTACHMENTS / 1024 / 1024
                ),
            ));
        }
        let data = std::fs::read(&path)
            .map_err(|error| RpcError::new(codes::NOT_FOUND, format!("cannot read the file: {error}")))?;
        let hash = blobs.put(&data).map_err(crate::session::internal)?;
        let name = compose::file_name(
            &path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default(),
        );
        Ok((hash, name, data.len() as u64))
    })
    .await
    .map_err(crate::session::internal)??;
    let (hash, name, size) = read;
    let mime = compose::guess_mime(&name).to_owned();
    if params.inline {
        check_image(state, draft_id, &mime, size).await?;
        let content_id = new_content_id();
        state
            .store
            .write(move |tx| drafts::attach_inline(tx, draft_id, (&hash, &name, &mime, size), &content_id))
            .await
            .map_err(conflict)?;
    } else {
        state
            .store
            .write(move |tx| drafts::attach(tx, draft_id, &hash, &name, &mime, size))
            .await
            .map_err(conflict)?;
    }
    view(state, draft_id).await
}

/// An image for the text: of a kind mail shows, and not too big.
async fn check_image(state: &State, draft_id: i64, mime: &str, size: u64) -> Result<(), RpcError> {
    if !IMAGE_TYPES.contains(&mime) {
        return Err(RpcError::new(
            codes::INVALID_PARAMS,
            "only PNG, JPEG, GIF and WebP images go in the text; attach other files beside it",
        ));
    }
    if size > MAX_IMAGE {
        return Err(RpcError::new(
            codes::TOO_LARGE,
            format!(
                "an image in the text can be at most {} MB; attach it instead",
                MAX_IMAGE / 1024 / 1024
            ),
        ));
    }
    let (images, all): (u64, u64) = state
        .store
        .read(move |conn| {
            let files = drafts::attachments(conn, draft_id)?;
            let images = files.iter().filter(|f| f.content_id.is_some()).map(|f| f.size).sum();
            Ok((images, files.iter().map(|f| f.size).sum()))
        })
        .await
        .map_err(crate::session::internal)?;
    if images + size > MAX_IMAGES || all + size > MAX_ATTACHMENTS {
        return Err(RpcError::new(
            codes::TOO_LARGE,
            format!(
                "the images in a message's text can be {} MB in all; attach the rest instead",
                MAX_IMAGES / 1024 / 1024
            ),
        ));
    }
    Ok(())
}

fn new_content_id() -> String {
    format!("{}@regards", accounts::new_id())
}

/// Put an image in a draft's text, from its bytes (pasted, say).
pub(crate) async fn attach_image(state: &State, params: DraftAttachImageParams) -> Result<Value, RpcError> {
    // Base64 is a third larger than what it carries.
    if params.data.len() as u64 > MAX_IMAGE / 3 * 4 + 8 {
        return Err(RpcError::new(
            codes::TOO_LARGE,
            format!(
                "an image in the text can be at most {} MB; attach it instead",
                MAX_IMAGE / 1024 / 1024
            ),
        ));
    }
    let data = regards_core::encoding::base64_decode(&params.data)
        .ok_or_else(|| RpcError::new(codes::INVALID_PARAMS, "the image is not base64"))?;
    let (draft_id, mime) = (params.draft_id, params.mime.to_ascii_lowercase());
    check_image(state, draft_id, &mime, data.len() as u64).await?;
    let blobs = state.blobs.clone();
    let size = data.len() as u64;
    let hash = tokio::task::spawn_blocking(move || blobs.put(&data))
        .await
        .map_err(crate::session::internal)?
        .map_err(crate::session::internal)?;
    let (name, content_id) = (compose::file_name(&params.name), new_content_id());
    let cid = content_id.clone();
    state
        .store
        .write(move |tx| drafts::attach_inline(tx, draft_id, (&hash, &name, &mime, size), &cid))
        .await
        .map_err(conflict)?;
    let draft = view(state, draft_id).await?;
    Ok(json!({ "content_id": content_id, "draft": draft }))
}

/// The images in a draft's text, to show while writing.
pub(crate) async fn images(state: &State, params: DraftRef) -> Result<Value, RpcError> {
    let id = params.id;
    let files = state
        .store
        .read(move |conn| {
            if drafts::get(conn, id)?.is_none() {
                return Ok(None);
            }
            Ok(Some(drafts::attachments(conn, id)?))
        })
        .await
        .map_err(crate::session::internal)?
        .ok_or_else(|| RpcError::new(codes::NOT_FOUND, "no such draft"))?;
    let mut images = Vec::new();
    for file in files {
        let Some(content_id) = file.content_id else { continue };
        let blobs = state.blobs.clone();
        let hash = file.blob_hash.clone();
        let data = tokio::task::spawn_blocking(move || blobs.get(&hash))
            .await
            .map_err(crate::session::internal)?
            .map_err(crate::session::internal)?;
        // A missing blob: the image is gone; the editor shows it broken.
        if let Some(data) = data {
            images.push(DraftImage {
                content_id,
                mime: file.mime,
                data: regards_core::encoding::base64(&data),
            });
        }
    }
    Ok(json!(DraftImagesResult { images }))
}

pub(crate) async fn detach(state: &State, params: DraftDetachParams) -> Result<Value, RpcError> {
    let (draft_id, attachment_id) = (params.draft_id, params.attachment_id);
    let removed = state
        .store
        .write(move |tx| drafts::detach(tx, draft_id, attachment_id))
        .await
        .map_err(conflict)?
        .value;
    if !removed {
        return Err(RpcError::new(codes::NOT_FOUND, "no such attachment"));
    }
    view(state, draft_id).await
}

/// Send later: further ahead than this is surely a mistake (seconds read as
/// milliseconds, say).
const MAX_SEND_AHEAD_MS: i64 = 5 * 366 * 24 * 3600 * 1000;

/// Freeze the draft's exact bytes into the outbox. It goes once the undo
/// window is over, or at `send_at`; until then [`undo`] takes it back.
pub(crate) async fn send(state: &Arc<State>, params: DraftSendParams) -> Result<Value, RpcError> {
    send_unchanged(state, params, None).await
}

/// [`send`], only if the draft is still at `version` (an approved hold:
/// what the person saw is what goes).
pub(crate) async fn send_unchanged(
    state: &Arc<State>,
    params: DraftSendParams,
    version: Option<i64>,
) -> Result<Value, RpcError> {
    let undo_seconds = params.undo_seconds.unwrap_or(DEFAULT_UNDO_SECONDS);
    if undo_seconds > MAX_UNDO_SECONDS {
        return Err(RpcError::new(
            codes::INVALID_PARAMS,
            format!("undo_seconds must be 0 to {MAX_UNDO_SECONDS}"),
        ));
    }
    queue_send(state, params, version, undo_seconds, false).await
}

/// An agent's send from an account that lets it go after an undo window
/// (ADR-0012 §3): it waits `window` seconds, which the person is told of
/// and can call it back in, then goes. The agent cannot call it back.
pub(crate) async fn send_for_agent(
    state: &Arc<State>,
    params: DraftSendParams,
    window: u32,
) -> Result<Value, RpcError> {
    queue_send(state, params, None, window, true).await
}

/// Agents' sends that go without approval, per account and hour; past them,
/// a send waits for approval again.
const AGENT_SENDS_PER_HOUR: u32 = 20;

/// The undo window an agent's send of `draft_id` goes with, or `None`: it
/// waits for approval (the default, or past [`AGENT_SENDS_PER_HOUR`]).
pub(crate) async fn agent_send_window(state: &State, draft_id: i64) -> Result<Option<u32>, RpcError> {
    let now = now_ms();
    state
        .store
        .read(move |conn| {
            let Some(draft) = drafts::get(conn, draft_id)? else {
                return Ok(None);
            };
            let Some(window) = accounts::agent_send_window(conn, &draft.account_id)? else {
                return Ok(None);
            };
            let sent = notices::count_since(conn, &draft.account_id, Kind::AgentSending, now - 3_600_000)?;
            Ok((sent < AGENT_SENDS_PER_HOUR).then_some(window))
        })
        .await
        .map_err(crate::session::internal)
}

/// Freeze the draft into the outbox, to go after `undo_seconds`; `agent`:
/// the person is told an agent's message is on its way.
async fn queue_send(
    state: &Arc<State>,
    params: DraftSendParams,
    version: Option<i64>,
    undo_seconds: u32,
    agent: bool,
) -> Result<Value, RpcError> {
    let (id, send_at, remind_at) = (params.id, params.send_at, params.remind_at);
    let now = now_ms();
    if let Some(at) = send_at {
        check_send_at(at, now)?;
    }
    if remind_at.is_some_and(|at| at <= send_at.unwrap_or(now)) {
        return Err(RpcError::new(
            codes::INVALID_PARAMS,
            "a reminder must come after the message goes",
        ));
    }
    let undo_until = now + i64::from(undo_seconds) * 1000;
    let (seen_version, files) = draft_files(state, id).await?;
    if version.is_some_and(|version| version != seen_version) {
        return Err(RpcError::new(
            codes::CONFLICT,
            "the draft changed after it was held for approval, so it was not sent; it needs asking again",
        ));
    }
    let outbox_id = state
        .store
        .write(move |tx| {
            let (draft, account, identity) = unchanged_draft(tx, id, seen_version)?;
            if draft.sending {
                return Err(StoreError::Conflict("this message is already being sent".into()));
            }
            // Dated for when it goes, so a scheduled one does not arrive looking old.
            let message_id = compose::new_message_id(&identity.email);
            let outgoing = outgoing(&draft, &identity, files, message_id, send_at.unwrap_or(now));
            let mime = outgoing
                .build()
                .map_err(|error| StoreError::Conflict(error.to_string()))?;
            let entry = NewEntry {
                account_id: account.id.clone(),
                draft_id: Some(draft.id),
                message_id: outgoing.message_id.clone(),
                subject: draft.subject.clone(),
                recipients: draft.to.iter().chain(&draft.cc).chain(&draft.bcc).cloned().collect(),
                envelope_from: identity.email.clone(),
                envelope_to: outgoing.recipients(),
                mime,
                undo_until,
                send_at,
                answering: draft.replying_to,
                executor: executor_for(&account, send_at),
                // Its undo window was the agent's due, or the person approved its hold.
                by_agent: agent || version.is_some(),
            };
            let queued = outbox::queue(tx, &entry, now)?;
            // Keyed by the new Message-ID, so it applies to the copy in Sent.
            if remind_at.is_some() {
                marks::set(tx, &account.id, &entry.message_id, Mark::Remind, remind_at)?;
            }
            if agent {
                let notice = NewNotice {
                    account_id: account.id.clone(),
                    kind: Kind::AgentSending,
                    message_id: None,
                    outbox_id: Some(queued),
                    people: entry.recipients.clone(),
                    subject: entry.subject.clone(),
                };
                notices::add(tx, &notice, now)?;
            }
            Ok(queued)
        })
        .await
        .map_err(conflict)?
        .value;
    state.outbox.wake();
    if remind_at.is_some() {
        state.scheduler.wake();
    }
    Ok(json!(DraftSendResult {
        outbox_id,
        undo_until,
        send_at,
        executor: Executor::Device,
        needs_this_computer: Executor::Device.needs_this_computer(),
    }))
}

/// Who sends a message (plan §5.7): in 1.0, always this device. From 1.1 a
/// remote executor the person has set up (the provider's own scheduled
/// delivery, a Send Service) is picked here for a scheduled message, and
/// handed the frozen bytes once they are queued.
fn executor_for(_account: &accounts::Account, _send_at: Option<i64>) -> Executor {
    Executor::Device
}

/// Send a scheduled or held message at another time, or now. Its draft is
/// frozen, so the bytes are built again from it only to date them for when
/// they go; the Message-ID stays.
pub(crate) async fn reschedule(state: &Arc<State>, params: OutboxRescheduleParams) -> Result<Value, RpcError> {
    let (id, send_at) = (params.id, params.send_at);
    let now = now_ms();
    if let Some(at) = send_at {
        check_send_at(at, now)?;
    }
    let too_late = || RpcError::new(codes::CONFLICT, "too late: the message is being sent or has gone");
    let entry = state
        .store
        .read(move |conn| outbox::get(conn, id))
        .await
        .map_err(crate::session::internal)?
        .ok_or_else(|| RpcError::new(codes::NOT_FOUND, "no such message in the outbox"))?;
    if !matches!(entry.state, EntryState::Queued | EntryState::Held) {
        return Err(too_late());
    }
    let draft_id = entry.draft_id.ok_or_else(too_late)?;
    let (seen_version, files) = draft_files(state, draft_id).await?;
    let message_id = entry.message_id;
    let moved = state
        .store
        .write(move |tx| {
            let (draft, _, identity) = unchanged_draft(tx, draft_id, seen_version)?;
            let mime = outgoing(&draft, &identity, files, message_id, send_at.unwrap_or(now))
                .build()
                .map_err(|error| StoreError::Conflict(error.to_string()))?;
            outbox::reschedule(tx, id, send_at, &mime, now)
        })
        .await
        .map_err(conflict)?
        .value;
    if !moved {
        return Err(too_late());
    }
    state.outbox.wake();
    Ok(json!({ "rescheduled": true }))
}

/// A file kept for a draft: its blob, name, type and size.
pub(crate) type KeptFile = (String, String, String, u64);

/// A message's text, and its files kept in blobs/ like attached ones (up to
/// the usual limit), for a draft made from it (a forward, a draft picked up).
pub(crate) async fn text_and_files(state: &State, source: Vec<u8>) -> Result<(String, Vec<KeptFile>), RpcError> {
    let blobs = state.blobs.clone();
    tokio::task::spawn_blocking(move || -> std::io::Result<_> {
        let text = regards_core::body::parse(&source).text;
        let mut total = 0u64;
        let mut kept = Vec::new();
        for file in regards_core::body::files(&source) {
            total += file.data.len() as u64;
            if total > MAX_ATTACHMENTS {
                break;
            }
            let hash = blobs.put(&file.data)?;
            kept.push((hash, compose::file_name(&file.name), file.mime, file.data.len() as u64));
        }
        Ok((text, kept))
    })
    .await
    .map_err(crate::session::internal)?
    .map_err(crate::session::internal)
}

/// The copy of a draft for the server's Drafts folder (plan §5.9): its
/// account, the version it holds and its bytes, with the Message-ID all its
/// copies carry. `None` if the draft went or is being sent.
pub(crate) async fn draft_copy(state: &State, id: i64) -> Result<Option<(String, i64, Vec<u8>)>, RpcError> {
    let (version, files) = match draft_files(state, id).await {
        Ok(found) => found,
        Err(error) if error.code == codes::NOT_FOUND => return Ok(None),
        Err(error) => return Err(error),
    };
    let built = state
        .store
        .write(move |tx| {
            let Some(draft) = drafts::get(tx, id)? else {
                return Ok(None);
            };
            if draft.sending || draft.updated_at != version {
                return Ok(None); // Changed since the files were read: next time.
            }
            let account =
                accounts::get(tx, &draft.account_id)?.ok_or_else(|| StoreError::Conflict("no such account".into()))?;
            let identity = identity_of(tx, &account, draft.identity_id)?;
            let message_id =
                regards_store::draft_sync::message_id(tx, id, || compose::new_message_id(&identity.email))?
                    .ok_or_else(|| StoreError::Conflict("no such draft".into()))?;
            let mime = outgoing(&draft, &identity, files, message_id, draft.updated_at)
                .build_draft()
                .map_err(|error| StoreError::Conflict(error.to_string()))?;
            Ok(Some((account.id, version, mime)))
        })
        .await
        .map_err(conflict)?
        .value;
    Ok(built)
}

fn check_send_at(at: i64, now: i64) -> Result<(), RpcError> {
    if at <= now {
        return Err(RpcError::new(codes::INVALID_PARAMS, "that time has passed"));
    }
    if at - now > MAX_SEND_AHEAD_MS {
        return Err(RpcError::new(
            codes::INVALID_PARAMS,
            "send_at is more than five years ahead; it is Unix time in milliseconds",
        ));
    }
    Ok(())
}

/// A draft's version and files, read before the write so the writer does not
/// wait on the disk.
async fn draft_files(state: &State, id: i64) -> Result<(i64, Vec<compose::Attachment>), RpcError> {
    let listed = state
        .store
        .read(move |conn| {
            let Some(draft) = drafts::get(conn, id)? else {
                return Ok(None);
            };
            Ok(Some((draft.updated_at, drafts::attachments(conn, id)?)))
        })
        .await
        .map_err(crate::session::internal)?;
    let (version, listed) = listed.ok_or_else(|| RpcError::new(codes::NOT_FOUND, "no such draft"))?;
    let mut files = Vec::with_capacity(listed.len());
    for attachment in listed {
        let blobs = state.blobs.clone();
        let hash = attachment.blob_hash.clone();
        let data = tokio::task::spawn_blocking(move || blobs.get(&hash))
            .await
            .map_err(crate::session::internal)?
            .map_err(crate::session::internal)?
            .ok_or_else(|| {
                RpcError::new(
                    codes::CONFLICT,
                    format!("the attached file {} is missing; attach it again", attachment.name),
                )
            })?;
        files.push(compose::Attachment {
            name: attachment.name,
            mime: attachment.mime,
            data,
            content_id: attachment.content_id,
        });
    }
    Ok((version, files))
}

/// The draft, its account and who it goes out from, if the draft is as it
/// was when its files were read: what is frozen must be what was shown.
fn unchanged_draft(
    tx: &Transaction<'_>,
    id: i64,
    seen_version: i64,
) -> Result<(Draft, accounts::Account, Identity), StoreError> {
    let draft = drafts::get(tx, id)?.ok_or_else(|| StoreError::Conflict("no such draft".into()))?;
    if draft.updated_at != seen_version {
        return Err(StoreError::Conflict(
            "the draft changed while it was being sent; send it again".into(),
        ));
    }
    let account =
        accounts::get(tx, &draft.account_id)?.ok_or_else(|| StoreError::Conflict("no such account".into()))?;
    let identity = identity_of(tx, &account, draft.identity_id)?;
    Ok((draft, account, identity))
}

fn outgoing(
    draft: &Draft,
    identity: &Identity,
    attachments: Vec<compose::Attachment>,
    message_id: String,
    date: i64,
) -> Outgoing {
    Outgoing {
        attachments,
        from: sender(identity),
        to: draft.to.clone(),
        cc: draft.cc.clone(),
        bcc: draft.bcc.clone(),
        subject: draft.subject.clone(),
        text: draft.body.clone(),
        html: draft.html.clone(),
        message_id,
        in_reply_to: draft.in_reply_to.clone(),
        references: draft.references.clone(),
        date,
        calendar: None,
    }
}

pub(crate) async fn outbox_list(state: &State) -> Result<Value, RpcError> {
    let entries = state.store.read(outbox::list).await.map_err(crate::session::internal)?;
    Ok(json!(OutboxListResult {
        entries: entries
            .into_iter()
            .map(|entry| {
                // Tried, and its account is signed out: it goes once signed in again.
                let waiting = entry.state == EntryState::Queued
                    && entry.attempts > 0
                    && state.oauth.needs_sign_in(&entry.account_id);
                OutboxEntryView {
                    waiting_for_sign_in: waiting,
                    ..entry_view(entry)
                }
            })
            .collect(),
    }))
}

/// Undo send, or "edit" after a failure: the entry goes, the draft stays,
/// and so does no reminder: sending it again gives it a new Message-ID.
pub(crate) async fn undo(state: &State, params: OutboxRef) -> Result<Value, RpcError> {
    let id = params.id;
    let taken = state
        .store
        .write(move |tx| {
            let entry = outbox::get(tx, id)?;
            let taken = outbox::take_back(tx, id)?;
            if let (Some(_), Some(entry)) = (taken, entry) {
                marks::set(tx, &entry.account_id, &entry.message_id, Mark::Remind, None)?;
            }
            Ok(taken)
        })
        .await
        .map_err(crate::session::internal)?
        .value;
    let Some(draft_id) = taken else {
        return Err(RpcError::new(
            codes::CONFLICT,
            "too late: the message is being sent or has gone",
        ));
    };
    Ok(json!(OutboxUndoResult { draft_id }))
}

pub(crate) async fn send_again(state: &State, params: OutboxRef) -> Result<Value, RpcError> {
    let id = params.id;
    let now = now_ms();
    let queued = state
        .store
        .write(move |tx| outbox::send_again(tx, id, now))
        .await
        .map_err(crate::session::internal)?
        .value;
    if !queued {
        return Err(RpcError::new(
            codes::CONFLICT,
            "only failed or uncertain messages can be sent again",
        ));
    }
    state.outbox.wake();
    Ok(json!({ "queued": true }))
}

/// `outbox.dismiss`: forget a failed or uncertain message. A failed one's
/// draft is kept for a while, so Undo (`undo` in the answer) brings it back
/// to Drafts; one that did go out ("It was sent") has none.
pub(crate) async fn dismiss(state: &State, params: OutboxRef) -> Result<Value, RpcError> {
    let (id, now) = (params.id, now_ms());
    let dismissed = state
        .store
        .write(move |tx| {
            let kept = match outbox::get(tx, id)? {
                Some(entry) if entry.state == outbox::State::Failed => match entry.draft_id {
                    Some(draft) => keep_draft(tx, draft)?,
                    None => None,
                },
                _ => None,
            };
            if !outbox::dismiss(tx, id)? {
                return Ok(None);
            }
            let undo = match kept {
                Some(kept) => Some(crate::actions::record_draft_undo(
                    tx,
                    "Discarded a message that was not sent",
                    kept,
                    now,
                )?),
                None => None,
            };
            Ok(Some(undo))
        })
        .await
        .map_err(crate::session::internal)?
        .value;
    let Some(undo) = dismissed else {
        return Err(RpcError::new(
            codes::CONFLICT,
            "only failed or uncertain messages can be dismissed",
        ));
    };
    Ok(json!({ "dismissed": true, "undo": undo }))
}

pub(crate) async fn view(state: &State, id: i64) -> Result<Value, RpcError> {
    let found = state
        .store
        .read(move |conn| {
            let Some(draft) = drafts::get(conn, id)? else {
                return Ok(None);
            };
            let Some(account) = accounts::get(conn, &draft.account_id)? else {
                return Ok(None);
            };
            let identity = identity_of(conn, &account, draft.identity_id)?;
            let files = drafts::attachments(conn, id)?;
            Ok(Some(draft_view(draft, &identity, files)))
        })
        .await
        .map_err(crate::session::internal)?;
    let view = found.ok_or_else(|| RpcError::new(codes::NOT_FOUND, "no such draft"))?;
    Ok(json!(view))
}

/// Who an account's messages come from: its name (when it is not just the address) and address.
/// Who an identity's messages come from: its name (when it is not just the address) and address.
fn sender(identity: &Identity) -> Person {
    Person {
        name: Some(identity.name.clone()).filter(|name| name != &identity.email && !name.trim().is_empty()),
        address: Some(identity.email.clone()),
    }
}

/// A message that is no draft (an answer to an invitation): from `identity`
/// to `to`, in reply to `in_reply_to`, carrying the iCalendar `calendar`.
pub(crate) fn outgoing_from(
    identity: &Identity,
    to: Vec<Person>,
    subject: String,
    text: String,
    in_reply_to: Option<String>,
    date: i64,
    calendar: String,
) -> Outgoing {
    Outgoing {
        from: sender(identity),
        to,
        cc: Vec::new(),
        bcc: Vec::new(),
        subject,
        text,
        message_id: compose::new_message_id(&identity.email),
        references: in_reply_to.iter().cloned().collect(),
        in_reply_to,
        date,
        html: None,
        attachments: Vec::new(),
        calendar: Some(calendar),
    }
}

/// Who a draft of `account` goes out from: identity `id` if it is one of the
/// account's, else its default. An account always has one; the account
/// itself stands in otherwise.
pub(crate) fn identity_of(
    conn: &rusqlite::Connection,
    account: &accounts::Account,
    id: Option<i64>,
) -> Result<Identity, StoreError> {
    if let Some(id) = id
        && let Some(found) = identities::get(conn, id)?.filter(|i| i.account_id == account.id)
    {
        return Ok(found);
    }
    if let Some(found) = identities::default_for(conn, &account.id)? {
        return Ok(found);
    }
    Ok(Identity {
        id: 0,
        account_id: account.id.clone(),
        name: account.display_name.clone(),
        email: account.email.clone(),
        signature_id: None,
        signature: String::new(),
        voice: String::new(),
        voice_on: false,
        is_default: true,
    })
}

/// The identity to record on a draft: none for the stand-in.
fn stored(identity: &Identity) -> Option<i64> {
    (identity.id != 0).then_some(identity.id)
}

/// Send a draft from another identity: of its account, or of another
/// account, where the draft then moves (with its copy on the server). A
/// plain-text draft's signature is swapped; a formatted one keeps its text.
pub(crate) async fn set_from(state: &State, params: DraftSetFromParams) -> Result<Value, RpcError> {
    let (id, identity_id, now) = (params.id, params.identity_id, now_ms());
    state
        .store
        .write(move |tx| {
            let draft = drafts::get(tx, id)?.ok_or_else(|| StoreError::Conflict("no such draft".into()))?;
            let account =
                accounts::get(tx, &draft.account_id)?.ok_or_else(|| StoreError::Conflict("no such account".into()))?;
            let new = identities::get(tx, identity_id)?
                .ok_or_else(|| StoreError::Conflict("there is no such address".into()))?;
            let old = identity_of(tx, &account, draft.identity_id)?;
            let body = if draft.html.is_some() {
                draft.body.clone()
            } else {
                compose::swap_signature(&draft.body, &old.signature, &new.signature)
            };
            if new.account_id == account.id {
                drafts::set_identity(tx, id, new.id, &body, now)
            } else {
                drafts::move_to(tx, id, &new.account_id, new.id, &body, now)
            }
        })
        .await
        .map_err(conflict)?;
    view(state, id).await
}

/// The account an identity is of, if there is such an identity.
pub(crate) async fn identity_account(state: &State, identity_id: i64) -> Result<Option<String>, RpcError> {
    state
        .store
        .read(move |conn| Ok(identities::get(conn, identity_id)?.map(|identity| identity.account_id)))
        .await
        .map_err(crate::session::internal)
}

fn draft_view(draft: Draft, identity: &Identity, files: Vec<drafts::Attachment>) -> DraftView {
    DraftView {
        attachments: files
            .into_iter()
            .map(|file| DraftAttachment {
                id: file.id,
                name: file.name,
                mime: file.mime,
                size: file.size,
                content_id: file.content_id,
            })
            .collect(),
        id: draft.id,
        account_id: draft.account_id,
        from: sender(identity),
        identity_id: stored(identity),
        to: draft.to,
        cc: draft.cc,
        bcc: draft.bcc,
        subject: draft.subject,
        body: draft.body,
        html: draft.html,
        replying_to: draft.replying_to,
        updated_at: draft.updated_at,
        sending: draft.sending,
    }
}

fn entry_view(entry: Entry) -> OutboxEntryView {
    OutboxEntryView {
        id: entry.id,
        account_id: entry.account_id,
        draft_id: entry.draft_id,
        subject: entry.subject,
        recipients: entry.recipients,
        state: match entry.state {
            EntryState::Queued => OutboxState::Queued,
            EntryState::Held => OutboxState::Held,
            EntryState::HandedOff => OutboxState::HandedOff,
            // Sent entries are not listed; a sending one shows as sending.
            EntryState::Sending | EntryState::Sent => OutboxState::Sending,
            EntryState::Failed => OutboxState::Failed,
            EntryState::Uncertain => OutboxState::Uncertain,
        },
        undo_until: entry.undo_until,
        send_at: entry.send_at,
        next_attempt_at: entry.next_attempt_at,
        attempts: entry.attempts,
        last_error: entry.last_error,
        executor: entry.executor,
        needs_this_computer: entry.executor.needs_this_computer(),
        waiting_for_sign_in: false,
        by_agent: entry.by_agent,
    }
}

fn check_content(to: &[Person], cc: &[Person], bcc: &[Person], body: &str) -> Result<(), RpcError> {
    if to.len() + cc.len() + bcc.len() > MAX_RECIPIENTS {
        return Err(RpcError::new(
            codes::INVALID_PARAMS,
            format!("at most {MAX_RECIPIENTS} recipients"),
        ));
    }
    if body.len() > MAX_BODY {
        return Err(RpcError::new(codes::TOO_LARGE, "the message text is too long"));
    }
    Ok(())
}

fn conflict(error: StoreError) -> RpcError {
    match error {
        StoreError::Conflict(message) => RpcError::new(codes::CONFLICT, message),
        other => crate::session::internal(other),
    }
}

pub(crate) fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or_default()
}
