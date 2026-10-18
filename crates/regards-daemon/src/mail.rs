//! Reading mail from the store. Only the `ui` principal reaches these until
//! agent access levels exist (ADR-0012).

use crate::State;
use crate::sync;
use regards_core::body;
use regards_ipc::protocol::{
    MessageCursor, MessageDetail, MessageListParams, MessageListResult, MessageRef, MessageSummary, RpcError, codes,
};
use regards_store::mailboxes::Mailbox;
use regards_store::messages::{self, Cursor, Message};
use regards_store::{StoreError, mailboxes};
use serde_json::{Value, json};

const DEFAULT_PAGE: u32 = 50;
const MAX_PAGE: u32 = 500;
/// Larger messages are not downloaded yet (they need chunked FETCH).
const MAX_DOWNLOAD: u32 = 50 * 1024 * 1024;

pub(crate) async fn list(state: &State, params: MessageListParams) -> Result<Value, RpcError> {
    let limit = params.limit.unwrap_or(DEFAULT_PAGE);
    if !(1..=MAX_PAGE).contains(&limit) {
        return Err(RpcError::new(
            codes::INVALID_PARAMS,
            format!("limit must be 1 to {MAX_PAGE}"),
        ));
    }
    let mailbox_id = params.mailbox_id;
    let before = params.before.map(|c| Cursor {
        received_at: c.received_at,
        id: c.id,
    });
    let page = state
        .store
        .read(move |conn| {
            if mailboxes::get(conn, mailbox_id)?.is_none() {
                return Ok(None);
            }
            // One extra row tells whether another page exists.
            Ok(Some(messages::list(conn, mailbox_id, limit + 1, before)?))
        })
        .await
        .map_err(store_error)?;
    let Some(mut rows) = page else {
        return Err(RpcError::new(codes::NOT_FOUND, "no such mailbox"));
    };
    let more = rows.len() > limit as usize;
    rows.truncate(limit as usize);
    let next = more.then(|| {
        let last = rows.last().expect("a full page is not empty");
        MessageCursor {
            received_at: last.received_at,
            id: last.id,
        }
    });
    let messages = rows.into_iter().map(summary).collect();
    Ok(json!(MessageListResult { messages, next }))
}

fn store_error(error: StoreError) -> RpcError {
    crate::session::internal(error)
}

/// One message with its body. The source is downloaded the first time and
/// kept under `blobs/`; afterwards it opens without contacting the server.
pub(crate) async fn get(state: &State, params: MessageRef) -> Result<Value, RpcError> {
    let id = params.id;
    let found = state
        .store
        .read(move |conn| {
            let Some(message) = messages::get(conn, id)? else {
                return Ok(None);
            };
            Ok(mailboxes::get(conn, message.mailbox_id)?.map(|mailbox| (message, mailbox)))
        })
        .await
        .map_err(store_error)?;
    let Some((message, mailbox)) = found else {
        return Err(RpcError::new(codes::NOT_FOUND, "no such message"));
    };

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
    let source = match stored {
        Some(source) => source,
        None => download(state, &message, &mailbox).await?,
    };
    let body = tokio::task::spawn_blocking(move || body::parse(&source))
        .await
        .map_err(crate::session::internal)?;
    Ok(json!(MessageDetail {
        to: message.to.clone(),
        cc: message.cc.clone(),
        text: body.text,
        has_html: body.has_html,
        attachments: body.attachments,
        message: summary(message),
    }))
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
    let outcome = async {
        let selected = session.select(&mailbox.remote_name).await?;
        // A different UIDVALIDITY means the UID may now name another message.
        if mailbox.uid_validity != Some(i64::from(selected.uid_validity)) {
            return Ok(None);
        }
        session.fetch_source(message.uid).await
    }
    .await;
    session.logout().await;
    let Some(source) = outcome.map_err(|e| failed(&e))? else {
        return Err(RpcError::new(
            codes::NOT_FOUND,
            "the server no longer has this message; it disappears after the next sync",
        ));
    };

    let blobs = state.blobs.clone();
    let copy = source.clone();
    let hash = tokio::task::spawn_blocking(move || blobs.put(&copy))
        .await
        .map_err(crate::session::internal)?
        .map_err(crate::session::internal)?;
    let id = message.id;
    state
        .store
        .write(move |tx| messages::set_blob(tx, id, &hash))
        .await
        .map_err(store_error)?;
    Ok(source)
}

fn summary(m: Message) -> MessageSummary {
    MessageSummary {
        id: m.id,
        uid: m.uid,
        subject: m.subject,
        from: m.from,
        received_at: m.received_at,
        sent_at: m.sent_at,
        seen: m.flags.seen,
        flagged: m.flags.flagged,
        answered: m.flags.answered,
        size: m.size,
    }
}
