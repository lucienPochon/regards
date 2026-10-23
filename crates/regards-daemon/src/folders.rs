//! Folders the person makes, renames, deletes or empties. The server is
//! changed first, over a connection of its own; then the folder list is read
//! again, so what is shown is what the server has. `ui` only: an agent never
//! reshapes an account.
//!
//! Role folders (Inbox, Sent, Drafts, Trash, Junk, Archive) are never renamed
//! or deleted: the app and other clients rely on them.

use crate::{State, sync};
use regards_core::mailbox::Role;
use regards_imap::utf7;
use regards_ipc::protocol::{
    DeleteResult, MailboxCreateParams, MailboxRef, MailboxRenameParams, MailboxSummary, RpcError, codes,
};
use regards_store::mailboxes::{self, Mailbox};
use regards_store::operations::{self, FlagName, NewOperation, OperationKind};
use regards_store::{StoreError, accounts, messages};
use serde_json::{Value, json};
use std::sync::Arc;

/// Longer than this is not a folder name.
const MAX_NAME: usize = 200;
/// UIDs per delete operation when a folder is emptied.
const EMPTY_BATCH: usize = 500;

/// `mailbox.create`: a new folder, at the top of the account or inside `parent_id`.
pub(crate) async fn create(state: &Arc<State>, params: MailboxCreateParams) -> Result<Value, RpcError> {
    let account_id = params.account_id;
    let folders = folders_of(state, &account_id).await?;
    let parent = match params.parent_id {
        Some(id) => Some(
            folders
                .iter()
                .find(|folder| folder.id == id)
                .cloned()
                .ok_or_else(|| RpcError::new(codes::NOT_FOUND, "no such folder"))?,
        ),
        None => None,
    };
    let delimiter = parent
        .as_ref()
        .and_then(|folder| folder.delimiter)
        .or_else(|| folders.iter().find_map(|folder| folder.delimiter))
        .unwrap_or('/');
    let name = valid_name(&params.name, delimiter)?;
    let mut remote = match &parent {
        Some(parent) => [parent.remote_name.as_slice(), delimiter.to_string().as_bytes()].concat(),
        None => personal_prefix(&folders, delimiter),
    };
    remote.extend_from_slice(utf7::encode(&name).as_bytes());
    if folders
        .iter()
        .any(|folder| folder.remote_name.eq_ignore_ascii_case(&remote))
    {
        return Err(RpcError::new(
            codes::CONFLICT,
            "there is a folder with that name already",
        ));
    }

    let mut session = sync::login(state, &account_id).await.map_err(failed)?;
    let outcome = async {
        session.create_mailbox(&remote).await?;
        // Some servers subscribe it themselves, and webmail then shows it anyway.
        if let Err(error) = session.subscribe_mailbox(&remote).await {
            tracing::debug!(%error, "SUBSCRIBE after CREATE refused");
        }
        Ok::<_, regards_imap::ImapError>(())
    }
    .await;
    let listed = match outcome {
        Ok(()) => sync::sync_mailbox_list(state, &mut session, &account_id).await,
        Err(error) => Err(error.into()),
    };
    session.logout().await;
    listed.map_err(failed)?;
    tracing::info!(account = %account_id, "folder created");

    let created = folders_of(state, &account_id)
        .await?
        .into_iter()
        .find(|folder| folder.remote_name == remote)
        .ok_or_else(|| RpcError::new(codes::SYNC_FAILED, "the server made the folder but does not list it"))?;
    Ok(json!(summary(created)))
}

/// `mailbox.rename`: a folder's own name; its folders inside follow, and all keep their mail.
pub(crate) async fn rename(state: &Arc<State>, params: MailboxRenameParams) -> Result<Value, RpcError> {
    let folder = folder(state, params.mailbox_id).await?;
    refuse_role(&folder, "renamed")?;
    let delimiter = folder.delimiter.unwrap_or('/');
    let name = valid_name(&params.name, delimiter)?;
    let parent_end = folder
        .remote_name
        .iter()
        .rposition(|&byte| folder.delimiter.is_some() && char::from(byte) == delimiter)
        .map_or(0, |at| at + 1);
    let mut remote = folder.remote_name[..parent_end].to_vec();
    remote.extend_from_slice(utf7::encode(&name).as_bytes());
    if remote == folder.remote_name {
        return Ok(json!(summary(folder)));
    }
    let folders = folders_of(state, &folder.account_id).await?;
    if folders
        .iter()
        .any(|other| other.remote_name.eq_ignore_ascii_case(&remote))
    {
        return Err(RpcError::new(
            codes::CONFLICT,
            "there is a folder with that name already",
        ));
    }

    let mut session = sync::login(state, &folder.account_id).await.map_err(failed)?;
    if let Err(error) = session.rename_mailbox(&folder.remote_name, &remote).await {
        session.logout().await;
        return Err(failed(error.into()));
    }
    if let Err(error) = session.subscribe_mailbox(&remote).await {
        tracing::debug!(%error, "SUBSCRIBE after RENAME refused");
    }
    // Here too, before the list is read again: the folder and those inside
    // keep their ids, mail and sync state.
    let paths: Vec<(i64, Vec<u8>, String)> = std::iter::once(&folder)
        .chain(inside(&folders, &folder))
        .map(|moved| {
            let new_remote = [remote.as_slice(), &moved.remote_name[folder.remote_name.len()..]].concat();
            (moved.id, new_remote.clone(), utf7::decode(&new_remote))
        })
        .collect();
    let stored = state.store.write(move |tx| mailboxes::set_paths(tx, &paths)).await;
    let listed = match stored {
        Ok(_) => sync::sync_mailbox_list(state, &mut session, &folder.account_id).await,
        Err(error) => Err(error.into()),
    };
    session.logout().await;
    listed.map_err(failed)?;
    tracing::info!(account = %folder.account_id, "folder renamed");
    Ok(json!(summary(self::folder(state, folder.id).await?)))
}

/// `mailbox.delete`: a folder, those inside it, and all their mail, deepest first.
pub(crate) async fn delete(state: &Arc<State>, params: MailboxRef) -> Result<Value, RpcError> {
    let folder = folder(state, params.mailbox_id).await?;
    refuse_role(&folder, "deleted")?;
    let folders = folders_of(state, &folder.account_id).await?;
    let mut doomed: Vec<&Mailbox> = inside(&folders, &folder).collect();
    if let Some(role) = doomed.iter().find_map(|inner| inner.role) {
        return Err(RpcError::new(
            codes::CONFLICT,
            format!(
                "a folder inside it is the account's {}; move that out first",
                role.as_str()
            ),
        ));
    }
    doomed.sort_by_key(|inner| std::cmp::Reverse(inner.remote_name.len()));
    doomed.push(&folder);

    let mut session = sync::login(state, &folder.account_id).await.map_err(failed)?;
    let mut outcome = Ok(());
    for gone in &doomed {
        if let Err(error) = session.delete_mailbox(&gone.remote_name).await {
            outcome = Err(error.into());
            break;
        }
    }
    // Read again whatever happened: what was deleted before a refusal is gone.
    let listed = sync::sync_mailbox_list(state, &mut session, &folder.account_id).await;
    session.logout().await;
    outcome.and(listed).map_err(failed)?;
    tracing::info!(account = %folder.account_id, folders = doomed.len(), "folder deleted");
    Ok(json!({ "deleted": true }))
}

/// `mailbox.empty`: everything in Trash or Junk deleted for good, as Delete
/// forever does to each (applied here at once, sent to the server by the queue).
pub(crate) async fn empty(state: &Arc<State>, params: MailboxRef) -> Result<Value, RpcError> {
    let id = params.mailbox_id;
    let now = crate::compose::now_ms();
    let committed = state
        .store
        .write(move |tx| {
            let folder = mailboxes::get(tx, id)?.ok_or_else(|| StoreError::Conflict("no such folder".into()))?;
            if !matches!(folder.role, Some(Role::Trash | Role::Junk)) {
                return Err(StoreError::Conflict("only Trash and Junk can be emptied".into()));
            }
            let uids = messages::shown_uids(tx, id)?;
            if uids.is_empty() {
                return Ok((0, folder.account_id));
            }
            let uid_validity = folder
                .uid_validity
                .ok_or_else(|| StoreError::Conflict("the folder has not been synced yet".into()))?;
            for batch in uids.chunks(EMPTY_BATCH) {
                operations::apply_locally(tx, id, batch, &[FlagName::Deleted], true)?;
                operations::enqueue(
                    tx,
                    &NewOperation {
                        mailbox_id: id,
                        uid_validity: uid_validity as u32,
                        kind: OperationKind::Delete,
                        uids: batch.to_vec(),
                        flags: vec![],
                        target_mailbox_id: None,
                    },
                    now,
                )?;
            }
            Ok((uids.len(), folder.account_id))
        })
        .await
        .map_err(conflict)?;
    let (deleted, account) = committed.value;
    if deleted > 0 {
        state.queue.wake(state, &account);
    }
    tracing::info!(deleted, "folder emptied");
    Ok(json!(DeleteResult { deleted }))
}

/// A name for one folder: trimmed, not empty, without the server's separator.
fn valid_name(name: &str, delimiter: char) -> Result<String, RpcError> {
    let name = name.trim();
    let invalid = |message: String| Err(RpcError::new(codes::INVALID_PARAMS, message));
    if name.is_empty() {
        return invalid("enter a name for the folder".into());
    }
    if name.chars().count() > MAX_NAME {
        return invalid("that name is too long".into());
    }
    if name.contains(delimiter) || name.chars().any(|c| c.is_control() || matches!(c, '*' | '%')) {
        return invalid(format!("a folder name cannot contain “{delimiter}”, “*” or “%”"));
    }
    if name.eq_ignore_ascii_case("inbox") {
        return invalid("there is an Inbox already".into());
    }
    Ok(name.to_owned())
}

/// Where the account's own folders live: some servers keep them all inside
/// the inbox (`INBOX.Receipts`), and refuse new ones elsewhere.
fn personal_prefix(folders: &[Mailbox], delimiter: char) -> Vec<u8> {
    let prefix = format!("INBOX{delimiter}").into_bytes();
    let others: Vec<&Mailbox> = folders
        .iter()
        .filter(|folder| !folder.remote_name.eq_ignore_ascii_case(b"INBOX"))
        .collect();
    let all_inside = !others.is_empty()
        && others.iter().all(|folder| {
            folder.remote_name.len() > prefix.len() && folder.remote_name[..prefix.len()].eq_ignore_ascii_case(&prefix)
        });
    if all_inside { prefix } else { Vec::new() }
}

/// The folders inside `folder`, at any depth.
fn inside<'a>(folders: &'a [Mailbox], folder: &'a Mailbox) -> impl Iterator<Item = &'a Mailbox> {
    let prefix = folder
        .delimiter
        .map(|delimiter| [folder.remote_name.as_slice(), delimiter.to_string().as_bytes()].concat());
    folders.iter().filter(move |other| {
        other.id != folder.id
            && prefix
                .as_ref()
                .is_some_and(|prefix| other.remote_name.starts_with(prefix))
    })
}

fn refuse_role(folder: &Mailbox, done: &str) -> Result<(), RpcError> {
    match folder.role {
        Some(role) => Err(RpcError::new(
            codes::CONFLICT,
            format!("the account's {} cannot be {done}", role.as_str()),
        )),
        None => Ok(()),
    }
}

async fn folder(state: &State, id: i64) -> Result<Mailbox, RpcError> {
    state
        .store
        .read(move |conn| mailboxes::get(conn, id))
        .await
        .map_err(crate::session::internal)?
        .ok_or_else(|| RpcError::new(codes::NOT_FOUND, "no such folder"))
}

async fn folders_of(state: &State, account_id: &str) -> Result<Vec<Mailbox>, RpcError> {
    let id = account_id.to_owned();
    state
        .store
        .read(move |conn| {
            if accounts::get(conn, &id)?.is_none() {
                return Ok(None);
            }
            Ok(Some(mailboxes::list(conn, &id)?))
        })
        .await
        .map_err(crate::session::internal)?
        .ok_or_else(|| RpcError::new(codes::NOT_FOUND, "no such account"))
}

fn summary(folder: Mailbox) -> MailboxSummary {
    MailboxSummary {
        id: folder.id,
        name: folder.name,
        delimiter: folder.delimiter,
        role: folder.role,
        selectable: folder.selectable,
        unread: 0,
    }
}

/// The server refused, or could not be reached: its own words.
fn failed(error: sync::SyncError) -> RpcError {
    RpcError::new(codes::SYNC_FAILED, error.to_string())
}

fn conflict(error: StoreError) -> RpcError {
    match error {
        StoreError::Conflict(message) => RpcError::new(codes::CONFLICT, message),
        other => crate::session::internal(other),
    }
}
