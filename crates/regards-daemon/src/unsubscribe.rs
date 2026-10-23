//! Leaving a mailing list from one of its messages (`message.unsubscribe`):
//! the ways its headers give ([`regards_core::unsubscribe`]), best first.
//! The links carry the person's own token: they are never logged.

use crate::State;
use crate::compose;
use regards_ipc::protocol::{DraftSendParams, DraftSendResult, RpcError, UnsubscribeParams, UnsubscribeResult, codes};
use regards_net::https::post_form;
use serde_json::{Value, json};
use std::sync::Arc;

pub(crate) async fn unsubscribe(state: &Arc<State>, params: UnsubscribeParams) -> Result<Value, RpcError> {
    let (message, source) = crate::mail::load(state, params.id).await?;
    let ways = regards_core::unsubscribe::ways(&source);
    // RFC 8058: one POST, no cookies, nothing to confirm. Refused or out of
    // reach, the other ways remain.
    if let Some(url) = &ways.one_click {
        match post_form(url, &[("List-Unsubscribe", "One-Click")], &state.tls).await {
            Ok(answer) if (200..300).contains(&answer.status) => {
                tracing::info!("unsubscribed with one click");
                return Ok(json!(UnsubscribeResult::Done));
            }
            Ok(answer) => tracing::warn!(status = answer.status, "the one-click unsubscribe was refused"),
            Err(error) => tracing::warn!(%error, "the one-click unsubscribe failed"),
        }
    }
    if let Some(mailto) = ways.mailto {
        let draft = compose::unsubscribe_draft(state, message.id, &source, mailto).await?;
        let sent = compose::send(
            state,
            DraftSendParams {
                id: draft,
                undo_seconds: params.undo_seconds,
                send_at: None,
                remind_at: None,
            },
        )
        .await?;
        let queued: DraftSendResult = serde_json::from_value(sent).map_err(crate::session::internal)?;
        tracing::info!("unsubscribe email queued");
        return Ok(json!(UnsubscribeResult::Sent { queued }));
    }
    match ways.web.or(ways.one_click) {
        Some(url) => Ok(json!(UnsubscribeResult::Web { url })),
        None => Err(RpcError::new(
            codes::NOT_FOUND,
            "this message has no way to unsubscribe; its sender may have one on its website",
        )),
    }
}
