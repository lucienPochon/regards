//! Context for agents (ADR-0020 §4): with every message, conversation or
//! draft an agent reads or writes, the person's voice for the address in
//! play, and what the account knows of the other people in it: how much
//! each side wrote, and the latest notes. Nothing of it is stored: it is
//! worked out when asked for.

use crate::State;
use regards_core::headers::Person;
use regards_ipc::protocol::{AgentContext, PersonMemory};
use regards_store::identities::{self, Identity};
use regards_store::{drafts, mailboxes, messages, notes, people};
use serde_json::Value;

/// The other people given at most.
const PEOPLE: usize = 10;

/// What the context is for.
#[derive(Debug, Clone, Copy)]
pub(crate) enum Of {
    Message(i64),
    /// The conversation the message is in.
    Thread(i64),
    Draft(i64),
}

impl Of {
    /// For a draft result: the draft it is.
    pub(crate) fn draft_in(result: &Value) -> Option<Self> {
        result.get("id").and_then(Value::as_i64).map(Self::Draft)
    }
}

/// `result` with its `context`; as it was if there is none to give, or if
/// working it out failed (that is logged, not the agent's problem).
pub(crate) async fn attach(state: &State, mut result: Value, of: Option<Of>) -> Value {
    let Some(of) = of else { return result };
    match gather(state, of).await {
        Ok(Some(context)) if result.is_object() => {
            result["context"] = serde_json::to_value(context).expect("protocol types always serialize");
        }
        Ok(_) => {}
        Err(error) => tracing::warn!(%error, "could not gather an agent's context"),
    }
    result
}

async fn gather(state: &State, of: Of) -> Result<Option<AgentContext>, String> {
    let notes_on = crate::notes::enabled(state).await.map_err(|error| error.message)?;
    state
        .store
        .read(move |conn| {
            let (account, identity, participants): (String, Option<Identity>, Vec<Person>) = match of {
                Of::Message(id) | Of::Thread(id) => {
                    let Some(message) = messages::get(conn, id)? else {
                        return Ok(None);
                    };
                    let Some(account) = mailboxes::get(conn, message.mailbox_id)?.map(|b| b.account_id) else {
                        return Ok(None);
                    };
                    let addressed: Vec<String> = message
                        .to
                        .iter()
                        .chain(&message.cc)
                        .filter_map(|p| p.address.clone())
                        .collect();
                    let addressed: Vec<&str> = addressed.iter().map(String::as_str).collect();
                    let identity = identities::for_addresses(conn, &account, &addressed)?;
                    let all = match (of, message.thread_id) {
                        (Of::Thread(_), Some(thread)) => messages::in_thread(conn, thread)?,
                        _ => vec![message],
                    };
                    let participants = all
                        .iter()
                        .flat_map(|m| m.from.iter().chain(&m.to).chain(&m.cc))
                        .cloned()
                        .collect();
                    (account, identity, participants)
                }
                Of::Draft(id) => {
                    let Some(draft) = drafts::get(conn, id)? else {
                        return Ok(None);
                    };
                    let identity = match draft.identity_id {
                        Some(identity) => {
                            identities::get(conn, identity)?.filter(|found| found.account_id == draft.account_id)
                        }
                        None => None,
                    };
                    let participants = draft.to.iter().chain(&draft.cc).chain(&draft.bcc).cloned().collect();
                    (draft.account_id, identity, participants)
                }
            };
            let identity = match identity {
                Some(found) => Some(found),
                None => identities::default_for(conn, &account)?,
            };
            let own: Vec<String> = identities::list(conn, &account)?
                .into_iter()
                .map(|i| i.email.to_lowercase())
                .collect();
            let mut others: Vec<(String, Option<String>)> = Vec::new();
            for person in participants {
                let Some(address) = person.address.map(|a| notes::address_key(&a)) else {
                    continue;
                };
                if own.contains(&address) || others.iter().any(|(seen, _)| *seen == address) {
                    continue;
                }
                others.push((address, person.name));
                if others.len() == PEOPLE {
                    break;
                }
            }
            let mut people_in_it = Vec::new();
            for (address, name) in others {
                let dealings = people::dealings(conn, &account, &address)?;
                let kept = if notes_on {
                    notes::about(conn, &account, &address, crate::notes::FOR_AGENTS)?
                } else {
                    Vec::new()
                };
                people_in_it.push(PersonMemory {
                    name: dealings.as_ref().and_then(|d| d.name.clone()).or(name),
                    sent: dealings.as_ref().map_or(0, |d| d.sent),
                    received: dealings.as_ref().map_or(0, |d| d.received),
                    last_at: dealings.map(|d| d.last_at),
                    notes: kept.into_iter().map(crate::notes::view).collect(),
                    address,
                });
            }
            Ok(Some(AgentContext {
                voice: identity.and_then(|i| i.active_voice().map(str::to_owned)),
                people: people_in_it,
            }))
        })
        .await
        .map_err(|error| error.to_string())
}
