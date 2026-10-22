//! Notes about people (ADR-0020): the person's own, and agents' (Triage),
//! kept per account and person.
//!
//! - **An agent's note** is an intent: audited, and undoable for a while.
//!   One that carries a link, an address or bank details waits for the
//!   person first (a `note` hold), its source named when it is not the
//!   person the note is about.
//! - **Switched off**, notes stay kept but nobody is given them, and agents
//!   cannot add any. Only the person forgets them.
//! - **`memory.get`** gathers what helps write to someone: the person's
//!   voice, how much each side wrote, and the latest notes.

use crate::State;
use crate::compose::now_ms;
use crate::holds::About;
use regards_core::headers::Person;
use regards_core::notes::Review;
use regards_ipc::protocol::{
    MemoryParams, MemoryResult, NoteAddParams, NoteAddResult, NoteAuthor, NoteListParams, NoteListResult, NoteRef,
    NoteRemoved, NoteSetEnabledParams, NoteSettings, NoteUpdateParams, NoteView, NotesForgotten, PersonMemory,
    RpcError, codes,
};
use regards_store::holds::Reason;
use regards_store::notes::{self, Author, NewNote, Note, Source};
use regards_store::{StoreError, accounts, identities, mailboxes, messages, people, settings};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

/// The setting that switches notes off: absent is on.
const SETTING: &str = "notes";
/// Notes an agent is given about one person: the latest.
pub(crate) const FOR_AGENTS: u32 = 20;

/// Whether notes are on (the default).
pub(crate) async fn enabled(state: &State) -> Result<bool, RpcError> {
    let value = state
        .store
        .read(|conn| settings::get(conn, SETTING))
        .await
        .map_err(crate::session::internal)?;
    Ok(value.as_deref() != Some("off"))
}

/// For an agent: notes are on, or the setting to change is named.
pub(crate) async fn require_on(state: &State) -> Result<(), RpcError> {
    if enabled(state).await? {
        Ok(())
    } else {
        Err(RpcError::new(
            codes::FORBIDDEN,
            "notes are switched off in Regards (Settings › Agents › Notes about people)",
        ))
    }
}

pub(crate) async fn settings(state: &State) -> Result<Value, RpcError> {
    let on = enabled(state).await?;
    let (notes, people) = state
        .store
        .read(notes::totals)
        .await
        .map_err(crate::session::internal)?;
    Ok(json!(NoteSettings { on, notes, people }))
}

pub(crate) async fn set_enabled(state: &State, params: NoteSetEnabledParams) -> Result<Value, RpcError> {
    let value = if params.on { "on" } else { "off" };
    state
        .store
        .write(move |tx| settings::set(tx, SETTING, value))
        .await
        .map_err(crate::session::internal)?;
    settings(state).await
}

pub(crate) async fn forget_all(state: &State) -> Result<Value, RpcError> {
    let forgotten = state
        .store
        .write(notes::forget_all)
        .await
        .map_err(crate::session::internal)?
        .value;
    Ok(json!(NotesForgotten {
        forgotten: u32::try_from(forgotten).unwrap_or(u32::MAX),
    }))
}

/// The notes about one person in one account, the latest `limit`, oldest first.
pub(crate) async fn list(state: &State, params: NoteListParams, limit: u32) -> Result<Value, RpcError> {
    let found = state
        .store
        .read(move |conn| {
            if accounts::get(conn, &params.account_id)?.is_none() {
                return Ok(None);
            }
            Ok(Some(notes::about(conn, &params.account_id, &params.address, limit)?))
        })
        .await
        .map_err(crate::session::internal)?
        .ok_or_else(|| RpcError::new(codes::NOT_FOUND, "no such account"))?;
    Ok(json!(NoteListResult {
        notes: found.into_iter().map(view).collect(),
    }))
}

/// A note's text as kept, or why not.
pub(crate) fn checked(text: &str) -> Result<String, RpcError> {
    notes::checked_text(text).map_err(|error| match error {
        StoreError::Conflict(message) => RpcError::new(codes::INVALID_PARAMS, message),
        other => crate::session::internal(other),
    })
}

/// The message a note is drawn from, of the account it is kept in.
async fn source(state: &State, account_id: &str, message: Option<i64>) -> Result<Source, RpcError> {
    let Some(id) = message else {
        return Ok(Source::default());
    };
    let account_id = account_id.to_owned();
    state
        .store
        .read(move |conn| {
            let Some(found) = messages::get(conn, id)? else {
                return Ok(None);
            };
            let same = mailboxes::get(conn, found.mailbox_id)?.is_some_and(|b| b.account_id == account_id);
            Ok(same.then(|| Source {
                message_id: found.message_id,
                subject: Some(found.subject),
                from: found.from.and_then(|from| from.address).map(|a| a.to_lowercase()),
            }))
        })
        .await
        .map_err(crate::session::internal)?
        .ok_or_else(|| RpcError::new(codes::NOT_FOUND, "no such message in that account"))
}

/// What a held agent's note is about, as the person is told: why it waits,
/// and whose message it came from when that is not the person's.
pub(crate) async fn held(state: &State, params: &NoteAddParams, review: Review) -> Result<About, RpcError> {
    let source = source(state, &params.account_id, params.from_message).await?;
    let address = notes::address_key(&params.address);
    let description = held_description(&address, review, source.from.as_deref());
    Ok(About {
        account_id: params.account_id.clone(),
        reason: Reason::Note,
        subject: description.clone(),
        description,
        draft: None,
        people: vec![Person {
            name: None,
            address: Some(address),
        }],
    })
}

/// Why a held note waits, and whose message it came from when that is not
/// the person's it is about.
fn held_description(address: &str, review: Review, from: Option<&str>) -> String {
    let mut description = format!("Save a note about {address}. It has {} in it", review.describe());
    if let Some(from) = from.filter(|from| *from != address) {
        description.push_str(&format!(
            ", and comes from a message that {address} did not send ({from})"
        ));
    }
    description.push('.');
    description
}

/// Keep a note: the person's, or an agent's (with its session; undoable).
pub(crate) async fn add(
    state: &State,
    params: NoteAddParams,
    author: Author,
    session: Option<String>,
) -> Result<Value, RpcError> {
    let text = checked(&params.text)?;
    let source = source(state, &params.account_id, params.from_message).await?;
    let address = notes::address_key(&params.address);
    if !address.contains('@') {
        return Err(RpcError::new(codes::INVALID_PARAMS, "give the person's email address"));
    }
    let note = NewNote {
        account_id: params.account_id,
        address,
        text,
        author,
        session,
        source,
    };
    let now = now_ms();
    let added = state
        .store
        .write(move |tx| {
            if accounts::get(tx, &note.account_id)?.is_none() {
                return Err(StoreError::Conflict("no such account".into()));
            }
            let id = notes::add(tx, &note, now)?;
            let undo = if note.author == Author::Agent {
                let description = format!("Saved a note about {}", note.address);
                Some(crate::actions::record_notes_undo(
                    tx,
                    &description,
                    vec![id],
                    Vec::new(),
                    now,
                )?)
            } else {
                None
            };
            let kept = notes::get(tx, id)?.ok_or_else(|| StoreError::Conflict("no such note".into()))?;
            Ok(NoteAddResult { note: view(kept), undo })
        })
        .await
        .map_err(conflict)?
        .value;
    Ok(json!(added))
}

/// The person changes a note.
pub(crate) async fn update(state: &State, params: NoteUpdateParams) -> Result<Value, RpcError> {
    let text = checked(&params.text)?;
    let (id, now) = (params.id, now_ms());
    let changed = state
        .store
        .write(move |tx| {
            if !notes::set_text(tx, id, &text, now)? {
                return Ok(None);
            }
            notes::get(tx, id)
        })
        .await
        .map_err(conflict)?
        .value
        .ok_or_else(|| RpcError::new(codes::NOT_FOUND, "no such note"))?;
    Ok(json!(view(changed)))
}

/// A note, to check who may remove it.
pub(crate) async fn find(state: &State, id: i64) -> Result<Note, RpcError> {
    state
        .store
        .read(move |conn| notes::get(conn, id))
        .await
        .map_err(crate::session::internal)?
        .ok_or_else(|| RpcError::new(codes::NOT_FOUND, "no such note"))
}

/// Remove a note; it can be put back for a while.
pub(crate) async fn remove(state: &State, params: NoteRef) -> Result<Value, RpcError> {
    let (id, now) = (params.id, now_ms());
    let removed = state
        .store
        .write(move |tx| {
            let Some(note) = notes::remove(tx, id)? else {
                return Ok(None);
            };
            let description = format!("Removed a note about {}", note.address);
            let undo = crate::actions::record_notes_undo(tx, &description, Vec::new(), vec![Kept::from(note)], now)?;
            Ok(Some(undo))
        })
        .await
        .map_err(conflict)?
        .value
        .ok_or_else(|| RpcError::new(codes::NOT_FOUND, "no such note"))?;
    Ok(json!(NoteRemoved {
        removed: true,
        undo: Some(removed),
    }))
}

/// What helps write to someone, from the first of `scope` (`None`: every
/// account) where they are known, or the account asked for.
pub(crate) async fn memory(state: &State, params: MemoryParams, scope: Option<Vec<String>>) -> Result<Value, RpcError> {
    let notes_on = enabled(state).await?;
    let address = notes::address_key(&params.address);
    let asked = params.account_id;
    let found = state
        .store
        .read(move |conn| {
            let candidates: Vec<String> = match (asked, scope) {
                (Some(account), _) => vec![account],
                (None, Some(open)) => open,
                (None, None) => accounts::list(conn)?.into_iter().map(|a| a.id).collect(),
            };
            let mut chosen = None;
            for account in &candidates {
                if accounts::get(conn, account)?.is_none() {
                    continue;
                }
                let dealings = people::dealings(conn, account, &address)?;
                let known = dealings.is_some() || notes::count_about(conn, account, &address)? > 0;
                if chosen.is_none() || known {
                    chosen = Some((account.clone(), dealings));
                }
                if known {
                    break;
                }
            }
            let Some((account_id, dealings)) = chosen else {
                return Ok(None);
            };
            let voice = identities::default_for(conn, &account_id)?
                .and_then(|identity| identity.active_voice().map(str::to_owned));
            let kept = if notes_on {
                notes::about(conn, &account_id, &address, FOR_AGENTS)?
            } else {
                Vec::new()
            };
            Ok(Some(MemoryResult {
                account_id,
                voice,
                person: PersonMemory {
                    address: address.clone(),
                    name: dealings.as_ref().and_then(|d| d.name.clone()),
                    sent: dealings.as_ref().map_or(0, |d| d.sent),
                    received: dealings.as_ref().map_or(0, |d| d.received),
                    last_at: dealings.map(|d| d.last_at),
                    notes: kept.into_iter().map(view).collect(),
                },
            }))
        })
        .await
        .map_err(crate::session::internal)?
        .ok_or_else(|| RpcError::new(codes::NOT_FOUND, "no such account"))?;
    Ok(json!(found))
}

fn conflict(error: StoreError) -> RpcError {
    match error {
        StoreError::Conflict(message) => RpcError::new(codes::CONFLICT, message),
        other => crate::session::internal(other),
    }
}

pub(crate) fn view(note: Note) -> NoteView {
    NoteView {
        id: note.id,
        account_id: note.account_id,
        address: note.address,
        text: note.text,
        by: match note.author {
            Author::Person => NoteAuthor::Person,
            Author::Agent => NoteAuthor::Agent,
        },
        from_message_id: note.source.message_id,
        from_subject: note.source.subject,
        from_address: note.source.from,
        created_at: note.created_at,
        updated_at: note.updated_at,
    }
}

/// A removed note as undo keeps it, to put it back as it was.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct Kept {
    id: i64,
    account_id: String,
    address: String,
    text: String,
    by_agent: bool,
    session: Option<String>,
    message_id: Option<String>,
    subject: Option<String>,
    from: Option<String>,
    created_at: i64,
    updated_at: i64,
}

impl From<Note> for Kept {
    fn from(note: Note) -> Self {
        Self {
            id: note.id,
            account_id: note.account_id,
            address: note.address,
            text: note.text,
            by_agent: note.author == Author::Agent,
            session: note.session,
            message_id: note.source.message_id,
            subject: note.source.subject,
            from: note.source.from,
            created_at: note.created_at,
            updated_at: note.updated_at,
        }
    }
}

impl From<Kept> for Note {
    fn from(kept: Kept) -> Self {
        Self {
            id: kept.id,
            account_id: kept.account_id,
            address: kept.address,
            text: kept.text,
            author: if kept.by_agent { Author::Agent } else { Author::Person },
            session: kept.session,
            source: Source {
                message_id: kept.message_id,
                subject: kept.subject,
                from: kept.from,
            },
            created_at: kept.created_at,
            updated_at: kept.updated_at,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_held_note_says_why_and_whose_message_it_came_from() {
        let anna = "anna@client.test";
        assert_eq!(
            held_description(anna, Review::Link, None),
            "Save a note about anna@client.test. It has a link in it."
        );
        assert_eq!(
            held_description(anna, Review::BankDetails, Some(anna)),
            "Save a note about anna@client.test. It has bank or card details in it.",
            "her own message: nothing more to say"
        );
        assert_eq!(
            held_description(anna, Review::BankDetails, Some("billing@payments.test")),
            "Save a note about anna@client.test. It has bank or card details in it, and comes from a \
             message that anna@client.test did not send (billing@payments.test)."
        );
    }
}
