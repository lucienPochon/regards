//! Reading mail from the store. Only the `ui` principal reaches these until
//! agent access levels exist (ADR-0012).

use crate::State;
use regards_ipc::protocol::{MessageCursor, MessageListParams, MessageListResult, MessageSummary, RpcError, codes};
use regards_store::messages::{self, Cursor};
use regards_store::{StoreError, mailboxes};
use serde_json::{Value, json};

const DEFAULT_PAGE: u32 = 50;
const MAX_PAGE: u32 = 500;

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
    let messages = rows
        .into_iter()
        .map(|m| MessageSummary {
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
        })
        .collect();
    Ok(json!(MessageListResult { messages, next }))
}

fn store_error(error: StoreError) -> RpcError {
    crate::session::internal(error)
}
