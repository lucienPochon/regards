//! The assistant (ADR-0015 §5, [`regards_core::assistant`]): with the
//! person's own key, a conversation summarised or a reply drafted, only when
//! they ask. It is off until they choose a provider. The key stays in the
//! system's credential store and goes only to the server it was given for.
//! Nothing a model writes is followed: a summary is shown, a reply is a
//! draft to review. `ui` only.

use crate::State;
use crate::accounts::{store_error, vault, vault_error};
use regards_core::assistant::{self, Kind, Message, Task};
use regards_core::headers::Person;
use regards_core::secret::Secret;
use regards_ipc::protocol::{
    AssistantParams, AssistantPreview, AssistantResult, AssistantSetSettingsParams, AssistantSettings,
    DraftReplyParams, DraftView, RpcError, codes,
};
use regards_net::https::post_json;
use regards_store::{accounts, mailboxes, messages, settings};
use regards_vault::SecretKey;
use serde_json::{Value, json};
use std::time::Duration;

const KIND: &str = "assistant.kind";
const BASE_URL: &str = "assistant.base_url";
const MODEL: &str = "assistant.model";
/// The key's name in the credential store.
const KEY: &str = "assistant/api-key";
/// A model may think a while before it answers.
const WAIT: Duration = Duration::from_secs(120);
/// The Messages API version Regards speaks.
const ANTHROPIC_VERSION: &str = "2023-06-01";
/// Messages of a conversation looked at, the latest: more than are sent, as
/// some may be left out.
const LOOKED_AT: usize = 2 * assistant::MAX_MESSAGES;

/// What the person chose.
struct Chosen {
    kind: Kind,
    base_url: String,
    model: String,
}

fn kind_name(kind: Kind) -> &'static str {
    match kind {
        Kind::Anthropic => "anthropic",
        Kind::OpenaiCompatible => "openai_compatible",
    }
}

fn key() -> SecretKey {
    SecretKey::from_stored(KEY)
}

fn invalid(message: &str) -> RpcError {
    RpcError::new(codes::INVALID_PARAMS, message.to_owned())
}

fn failed(message: String) -> RpcError {
    RpcError::new(codes::ASSISTANT_FAILED, message)
}

fn off() -> RpcError {
    RpcError::new(
        codes::CONFLICT,
        "the assistant is off; choose a provider in Settings to use it",
    )
}

async fn chosen(state: &State) -> Result<Option<Chosen>, RpcError> {
    let (kind, base_url, model) = state
        .store
        .read(|conn| {
            Ok((
                settings::get(conn, KIND)?,
                settings::get(conn, BASE_URL)?,
                settings::get(conn, MODEL)?,
            ))
        })
        .await
        .map_err(store_error)?;
    let kind = match kind.as_deref() {
        Some("anthropic") => Kind::Anthropic,
        Some("openai_compatible") => Kind::OpenaiCompatible,
        _ => return Ok(None),
    };
    Ok(match (base_url, model) {
        (Some(base_url), Some(model)) => Some(Chosen { kind, base_url, model }),
        _ => None,
    })
}

async fn current(state: &State) -> Result<AssistantSettings, RpcError> {
    let Some(chosen) = chosen(state).await? else {
        return Ok(AssistantSettings {
            kind: None,
            base_url: None,
            model: None,
            has_key: false,
        });
    };
    let has_key = vault(state).await?.get(&key()).await.map_err(vault_error)?.is_some();
    Ok(AssistantSettings {
        kind: Some(chosen.kind),
        base_url: Some(chosen.base_url),
        model: Some(chosen.model),
        has_key,
    })
}

/// `assistant.settings`.
pub(crate) async fn settings(state: &State) -> Result<Value, RpcError> {
    Ok(json!(current(state).await?))
}

/// `assistant.set_settings`.
pub(crate) async fn set_settings(state: &State, params: AssistantSetSettingsParams) -> Result<Value, RpcError> {
    let Some(kind) = params.kind else {
        // Off: nothing is kept that could be sent.
        state
            .store
            .write(|tx| settings::set(tx, KIND, "off"))
            .await
            .map_err(store_error)?;
        // Without a credential store, no key was ever kept.
        if let Ok(vault) = vault(state).await {
            vault.delete(&key()).await.map_err(vault_error)?;
        }
        return settings(state).await;
    };
    let base_url = match params.base_url.as_deref().map(str::trim).filter(|url| !url.is_empty()) {
        Some(url) => checked_base(url)?,
        None => kind
            .default_base_url()
            .ok_or_else(|| invalid("an OpenAI-compatible server needs its address"))?
            .to_owned(),
    };
    let model = match params.model.as_deref().map(str::trim).filter(|model| !model.is_empty()) {
        Some(model) if model.len() <= 200 && !model.chars().any(char::is_control) => model.to_owned(),
        Some(_) => return Err(invalid("that is not a model's name")),
        None => kind
            .default_model()
            .ok_or_else(|| invalid("which model should answer?"))?
            .to_owned(),
    };
    let new_key = match params.api_key.as_ref().map(|key| key.expose().trim()) {
        Some(text) if !text.is_empty() => {
            if text.len() > 1000 || text.chars().any(|c| c.is_control() || c.is_whitespace()) {
                return Err(invalid("that is not a key"));
            }
            Some(Secret::new(text))
        }
        _ => None,
    };
    let vault = vault(state).await?;
    // A key goes only to the server it was given for: for another, it is forgotten.
    let same_server = chosen(state)
        .await?
        .is_some_and(|before| before.kind == kind && origin(&before.base_url) == origin(&base_url));
    let kept = same_server && vault.get(&key()).await.map_err(vault_error)?.is_some();
    if kind == Kind::Anthropic && new_key.is_none() && !kept {
        return Err(invalid("an Anthropic API key is needed (from the Anthropic Console)"));
    }
    match &new_key {
        Some(secret) => vault.set(&key(), secret).await.map_err(vault_error)?,
        None if !same_server => vault.delete(&key()).await.map_err(vault_error)?,
        None => {}
    }
    state
        .store
        .write(move |tx| {
            settings::set(tx, KIND, kind_name(kind))?;
            settings::set(tx, BASE_URL, &base_url)?;
            settings::set(tx, MODEL, &model)
        })
        .await
        .map_err(store_error)?;
    settings(state).await
}

/// `https://` anywhere, `http://` only to this computer; nothing in it but
/// the address (no name and password, query or fragment).
fn checked_base(url: &str) -> Result<String, RpcError> {
    let url = url.trim_end_matches('/');
    if url.len() > 300 || url.chars().any(|c| c.is_whitespace() || c.is_control()) || url.contains(['?', '#', '@']) {
        return Err(invalid("that is not a server's address"));
    }
    let local = ["http://localhost", "http://127.0.0.1", "http://[::1]"]
        .iter()
        .any(|start| {
            url.strip_prefix(start)
                .is_some_and(|rest| rest.is_empty() || rest.starts_with([':', '/']))
        });
    let secure = url.strip_prefix("https://").is_some_and(|rest| !rest.is_empty());
    if !secure && !local {
        return Err(invalid(
            "the address must start with https:// (http:// only for a server on this computer)",
        ));
    }
    Ok(url.to_owned())
}

/// `scheme://host[:port]`: a key is kept for one (ADR-0015 §5).
fn origin(url: &str) -> String {
    let scheme = url.split_once("://").map_or("", |(scheme, _)| scheme);
    format!("{}://{}", scheme.to_ascii_lowercase(), authority(url))
}

/// `host[:port]`, as the person would recognise it.
fn authority(url: &str) -> String {
    let rest = url.split_once("://").map_or(url, |(_, rest)| rest);
    rest.split('/').next().unwrap_or(rest).to_ascii_lowercase()
}

/// What a task would send.
struct Gathered {
    messages: Vec<Message>,
    characters: usize,
    left_out: u32,
    /// The person, as the model is told of them.
    me: String,
}

fn shown(person: Option<&Person>) -> String {
    match person {
        Some(Person {
            name: Some(name),
            address: Some(address),
        }) => format!("{name} <{address}>"),
        Some(Person {
            address: Some(address), ..
        }) => address.clone(),
        Some(Person { name: Some(name), .. }) => name.clone(),
        _ => "unknown".into(),
    }
}

/// The conversation of the message asked about, the latest first to go; for
/// a reply, up to that message.
async fn gather(state: &State, params: &AssistantParams) -> Result<Gathered, RpcError> {
    let id = params.message_id;
    let replying = matches!(params.task, Task::Reply { .. });
    let (mut rows, me) = state
        .store
        .read(move |conn| {
            let Some(message) = messages::get(conn, id)? else {
                return Ok(None);
            };
            let account = match mailboxes::get(conn, message.mailbox_id)? {
                Some(mailbox) => accounts::get(conn, &mailbox.account_id)?,
                None => None,
            };
            let Some(account) = account else {
                return Ok(None);
            };
            let mut rows = match message.thread_id {
                Some(thread) => messages::in_thread(conn, thread)?,
                None => vec![message.clone()],
            };
            if replying {
                // The same message may be in two folders: it is kept once.
                let same = |row: &messages::Message| {
                    row.id == message.id || (row.message_id.is_some() && row.message_id == message.message_id)
                };
                if let Some(at) = rows.iter().position(same) {
                    rows.truncate(at + 1);
                }
            }
            Ok(Some((rows, format!("{} <{}>", account.display_name, account.email))))
        })
        .await
        .map_err(store_error)?
        .ok_or_else(|| RpcError::new(codes::NOT_FOUND, "no such message"))?;
    if rows.len() > LOOKED_AT {
        rows.drain(..rows.len() - LOOKED_AT);
    }
    let mut kept = Vec::new();
    let mut left_out = 0;
    for row in rows {
        let asked = row.id == id;
        let source = match crate::mail::load(state, row.id).await {
            Ok((_, source)) => source,
            Err(error) if asked => return Err(error),
            // Gone from the server since: the conversation goes without it.
            Err(_) => continue,
        };
        let text = regards_core::body::parse(&source).text;
        if assistant::sensitive(&row.subject, &text) {
            if asked {
                return Err(invalid(
                    "this message looks like it carries a sign-in or verification code, so it is not sent to a model",
                ));
            }
            left_out += 1;
            continue;
        }
        kept.push(Message {
            from: shown(row.from.as_ref()),
            date: row.sent_at.or(Some(row.received_at)),
            subject: row.subject,
            text: assistant::own_words(&text),
        });
    }
    let (messages, characters) = assistant::trimmed(&kept);
    if messages.is_empty() {
        return Err(invalid("there is nothing in this conversation to send"));
    }
    Ok(Gathered {
        messages,
        characters,
        left_out,
        me,
    })
}

/// `assistant.preview`: what would go, and where, before it goes.
pub(crate) async fn preview(state: &State, params: AssistantParams) -> Result<Value, RpcError> {
    let chosen = chosen(state).await?.ok_or_else(off)?;
    let gathered = gather(state, &params).await?;
    Ok(json!(AssistantPreview {
        messages: u32::try_from(gathered.messages.len()).unwrap_or(u32::MAX),
        characters: u32::try_from(gathered.characters).unwrap_or(u32::MAX),
        left_out: gathered.left_out,
        destination: authority(&chosen.base_url),
        model: chosen.model,
    }))
}

/// `assistant.run`: the summary, or the reply drafted (never sent).
pub(crate) async fn run(state: &State, params: AssistantParams) -> Result<Value, RpcError> {
    let chosen = chosen(state).await?.ok_or_else(off)?;
    let gathered = gather(state, &params).await?;
    let (system, user) = assistant::prompt(&params.task, &gathered.messages, &gathered.me);
    let key = vault(state).await?.get(&key()).await.map_err(vault_error)?;
    let text = ask(state, &chosen, key.as_ref(), &params.task, &system, &user).await?;
    let draft = match params.task {
        Task::Summary => None,
        Task::Reply { .. } => {
            let reply = DraftReplyParams {
                message_id: params.message_id,
                all: params.all,
            };
            let made = crate::compose::reply_with(state, reply, &text).await?;
            Some(serde_json::from_value::<DraftView>(made).map_err(crate::session::internal)?)
        }
    };
    Ok(json!(AssistantResult { text, draft }))
}

async fn ask(
    state: &State,
    chosen: &Chosen,
    key: Option<&Secret>,
    task: &Task,
    system: &str,
    user: &str,
) -> Result<String, RpcError> {
    let bearer: Secret;
    let mut headers: Vec<(&str, &str)> = Vec::new();
    let (url, body) = match chosen.kind {
        Kind::Anthropic => {
            let key = key.ok_or_else(|| failed("no Anthropic key is kept; add one in Settings".into()))?;
            headers.push(("x-api-key", key.expose()));
            headers.push(("anthropic-version", ANTHROPIC_VERSION));
            (
                format!("{}/v1/messages", chosen.base_url),
                assistant::anthropic_body(&chosen.model, task, system, user),
            )
        }
        Kind::OpenaiCompatible => {
            // A server on this computer may want no key.
            if let Some(key) = key {
                bearer = Secret::new(format!("Bearer {}", key.expose()));
                headers.push(("Authorization", bearer.expose()));
            }
            (
                format!("{}/chat/completions", chosen.base_url),
                assistant::openai_body(&chosen.model, task, system, user),
            )
        }
    };
    let answer = post_json(&url, &headers, &body, WAIT, &state.tls)
        .await
        .map_err(|error| failed(format!("could not reach {}: {error}", authority(&chosen.base_url))))?;
    let text = match chosen.kind {
        Kind::Anthropic => assistant::read_anthropic(answer.status, &answer.body),
        Kind::OpenaiCompatible => assistant::read_openai(answer.status, &answer.body),
    };
    text.map_err(|error| failed(error.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_go_only_to_an_address_that_is_safe_to_send_them_to() {
        assert_eq!(
            checked_base("https://api.anthropic.com/").unwrap(),
            "https://api.anthropic.com"
        );
        assert!(checked_base("http://localhost:11434/v1").is_ok());
        assert!(checked_base("http://127.0.0.1/v1").is_ok());
        assert!(checked_base("http://[::1]:8080").is_ok());
        assert!(checked_base("http://example.org/v1").is_err(), "never in the clear");
        assert!(checked_base("http://localhost.example.org/v1").is_err());
        assert!(checked_base("https://user:pass@example.org").is_err());
        assert!(checked_base("https://example.org/v1?key=1").is_err());
        assert!(checked_base("https://").is_err());
        assert!(checked_base("ftp://example.org").is_err());
        assert_eq!(authority("https://API.example.org:8443/v1"), "api.example.org:8443");
        assert_eq!(authority("http://[::1]:8080/v1"), "[::1]:8080");
        assert_ne!(origin("http://localhost:8443/v1"), origin("https://localhost:8443/v1"));
        assert_eq!(origin("https://Example.org/v1"), origin("https://example.org/v2"));
    }
}
