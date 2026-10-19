//! What agents did (ADR-0012 §4): every request an agent makes is recorded
//! after it is answered, allowed or refused, with the account it acted on and
//! what that account allowed then. The app lists it (`agent.activity`).
//!
//! Records are quiet writes: an agent's requests do not make every client
//! refetch. Secrets are taken out of the parameters before they are kept.

use crate::State;
use regards_core::agents::Access;
use regards_ipc::protocol::{
    AgentActivity, AgentActivityParams, AgentActivityResult, AgentDecision, ClientKind, RpcError, codes,
};
use regards_store::audit::{self, Decision, NewEntry};
use serde_json::{Value, json};
use std::sync::Arc;
use std::time::Duration;

/// How long records are kept.
const KEEP: Duration = Duration::from_secs(90 * 24 * 60 * 60);
/// Parameters longer than this (as JSON) are cut.
const MAX_ARGS: usize = 4 * 1024;
const DEFAULT_PAGE: u32 = 100;
const MAX_PAGE: u32 = 500;

/// One answered agent request.
pub(crate) struct Answered<'a> {
    pub session: &'a str,
    pub client: ClientKind,
    pub method: &'a str,
    pub params: &'a Value,
    /// The account the request was checked against, and its level then.
    pub checked: Option<(String, Access)>,
    pub error: Option<&'a RpcError>,
    pub duration: Duration,
    /// For a change: how what it changed stood before.
    pub before: Option<Value>,
    /// The undo entry that reverses it.
    pub undo_id: Option<i64>,
}

pub(crate) async fn record(state: &State, answered: Answered<'_>) {
    let (account_id, access) = answered.checked.unzip();
    let entry = NewEntry {
        at: crate::compose::now_ms(),
        session: answered.session.to_owned(),
        client: client_name(answered.client).to_owned(),
        method: answered.method.to_owned(),
        args: kept_args(answered.params),
        account_id,
        access,
        decision: match answered.error {
            Some(error) if error.code == codes::FORBIDDEN => Decision::Denied,
            Some(error) if error.code == codes::HELD => Decision::Held,
            _ => Decision::Allowed,
        },
        error: answered.error.map(|error| (error.code, error.message.clone())),
        duration_ms: answered.duration.as_millis().try_into().unwrap_or(i64::MAX),
        // Only what came to pass: a refused change changed nothing.
        before: answered
            .error
            .is_none()
            .then(|| answered.before.map(|before| kept_args(&before)))
            .flatten(),
        undo_id: answered.undo_id,
        hold_id: answered
            .error
            .filter(|error| error.code == codes::HELD)
            .and_then(|error| error.data.as_ref()?.get("hold_id")?.as_i64()),
    };
    if let Err(error) = state.store.write_quiet(move |tx| audit::record(tx, &entry)).await {
        tracing::warn!(%error, "an agent request was not recorded");
    }
}

pub(crate) fn client_name(client: ClientKind) -> &'static str {
    match client {
        ClientKind::Desktop => "desktop",
        ClientKind::Cli => "cli",
        ClientKind::Mcp => "mcp",
    }
}

/// The parameters as kept: secrets taken out, cut short when long.
fn kept_args(params: &Value) -> String {
    fn scrub(value: &mut Value) {
        match value {
            Value::Object(map) => {
                for (key, field) in map.iter_mut() {
                    let key = key.to_ascii_lowercase();
                    if ["password", "secret", "token"].iter().any(|word| key.contains(word)) {
                        *field = json!("[removed]");
                    } else {
                        scrub(field);
                    }
                }
            }
            Value::Array(items) => items.iter_mut().for_each(scrub),
            _ => {}
        }
    }
    let mut params = params.clone();
    scrub(&mut params);
    let text = params.to_string();
    if text.len() <= MAX_ARGS {
        return text;
    }
    let mut end = MAX_ARGS;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    json!({ "cut": format!("{}…", &text[..end]) }).to_string()
}

/// What agents did, newest first. Only the app gets here.
pub(crate) async fn activity(state: &State, params: AgentActivityParams) -> Result<Value, RpcError> {
    let limit = params.limit.unwrap_or(DEFAULT_PAGE);
    if !(1..=MAX_PAGE).contains(&limit) {
        return Err(RpcError::new(
            codes::INVALID_PARAMS,
            format!("limit must be 1 to {MAX_PAGE}"),
        ));
    }
    let (account_id, before, now) = (params.account_id, params.before, crate::compose::now_ms());
    let (mut rows, open) = state
        .store
        .read(move |conn| {
            let rows = audit::list(conn, account_id.as_deref(), before, limit + 1)?;
            let undos: Vec<i64> = rows.iter().filter_map(|row| row.entry.undo_id).collect();
            Ok((rows, regards_store::undo::open(conn, &undos, now)?))
        })
        .await
        .map_err(crate::session::internal)?;
    let more = rows.len() > limit as usize;
    rows.truncate(limit as usize);
    let next = more.then(|| rows.last().map(|row| row.id)).flatten();
    let entries = rows
        .into_iter()
        .map(|row| {
            let entry = row.entry;
            AgentActivity {
                id: row.id,
                at: entry.at,
                session: entry.session,
                client: entry.client,
                method: entry.method,
                args: serde_json::from_str(&entry.args).unwrap_or(Value::Null),
                account_id: entry.account_id,
                access: entry.access,
                decision: match entry.decision {
                    Decision::Allowed => AgentDecision::Allowed,
                    Decision::Denied => AgentDecision::Denied,
                    Decision::Held => AgentDecision::Held,
                    Decision::Approved => AgentDecision::Approved,
                    Decision::Rejected => AgentDecision::Rejected,
                },
                error_code: entry.error.as_ref().map(|(code, _)| *code),
                error: entry.error.map(|(_, message)| message),
                duration_ms: entry.duration_ms,
                before: entry.before.as_deref().and_then(|text| serde_json::from_str(text).ok()),
                undoable: entry.undo_id.is_some_and(|id| open.contains(&id)),
                undo_id: entry.undo_id,
            }
        })
        .collect();
    Ok(json!(AgentActivityResult { entries, next }))
}

/// Forget records older than [`KEEP`], at start and then daily.
pub(crate) async fn prune(state: Arc<State>) {
    loop {
        let before = crate::compose::now_ms() - KEEP.as_millis() as i64;
        match state.store.write_quiet(move |tx| audit::prune(tx, before)).await {
            Ok(0) => {}
            Ok(forgotten) => tracing::debug!(forgotten, "old agent activity forgotten"),
            Err(error) => tracing::warn!(%error, "old agent activity not forgotten for now"),
        }
        tokio::time::sleep(Duration::from_secs(24 * 60 * 60)).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secrets_are_taken_out_and_long_arguments_cut() {
        let kept = kept_args(&json!({
            "email": "ada@example.test",
            "password": "hunter2",
            "imap": { "Password": "again", "port": 993 },
            "tokens": [{ "ui_token": "t" }],
        }));
        assert!(
            !kept.contains("hunter2") && !kept.contains("again") && !kept.contains("\"t\""),
            "{kept}"
        );
        assert!(kept.contains("ada@example.test") && kept.contains("993"));
        let long = kept_args(&json!({ "body": "é".repeat(5_000) }));
        let parsed: Value = serde_json::from_str(&long).unwrap();
        assert!(parsed["cut"].as_str().unwrap().ends_with('…'));
        assert!(long.len() < MAX_ARGS + 100);
    }
}
