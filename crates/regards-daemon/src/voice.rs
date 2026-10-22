//! Voices (ADR-0020): how agents should write as each of the person's
//! addresses. Only the app sets one; an agent may suggest one, which always
//! waits for the person (a `voice` hold), who may edit it before approving.

use crate::State;
use crate::holds::About;
use regards_ipc::protocol::{IdentitySetVoiceParams, RpcError, VoiceSuggestParams, codes};
use regards_store::holds::Reason;
use regards_store::identities::{self, Identity, VOICE_MAX};
use regards_store::{StoreError, accounts};
use serde_json::{Value, json};

/// A voice as kept, or why not.
fn checked(text: &str, may_be_empty: bool) -> Result<String, RpcError> {
    let text = text.trim();
    if text.is_empty() && !may_be_empty {
        return Err(RpcError::new(codes::INVALID_PARAMS, "a voice needs some text"));
    }
    if text.chars().count() > VOICE_MAX {
        return Err(RpcError::new(
            codes::INVALID_PARAMS,
            format!("a voice can have at most {VOICE_MAX} characters"),
        ));
    }
    Ok(text.to_owned())
}

/// The person sets a voice, or switches it on or off.
pub(crate) async fn set(state: &State, params: IdentitySetVoiceParams) -> Result<Value, RpcError> {
    let voice = checked(&params.voice, true)?;
    let (id, on) = (params.id, params.on);
    save(state, id, voice, on).await
}

async fn save(state: &State, id: i64, voice: String, on: bool) -> Result<Value, RpcError> {
    let saved = state
        .store
        .write(move |tx| {
            if !identities::set_voice(tx, id, &voice, on)? {
                return Ok(None);
            }
            identities::get(tx, id)
        })
        .await
        .map_err(|error| match error {
            StoreError::Conflict(message) => RpcError::new(codes::INVALID_PARAMS, message),
            other => crate::session::internal(other),
        })?
        .value
        .ok_or_else(|| RpcError::new(codes::NOT_FOUND, "no such identity"))?;
    Ok(json!(crate::identities::view(saved)))
}

/// The identity an address names: in the account given, or among `scope`
/// (`None`: every account). An address in two of them needs the account.
async fn resolve(
    state: &State,
    address: &str,
    account_id: Option<String>,
    scope: Option<Vec<String>>,
) -> Result<Identity, RpcError> {
    let address = address.trim().to_owned();
    let found = state
        .store
        .read(move |conn| {
            let accounts: Vec<String> = match (account_id, scope) {
                (Some(account), _) => vec![account],
                (None, Some(open)) => open,
                (None, None) => accounts::list(conn)?.into_iter().map(|a| a.id).collect(),
            };
            let mut found = Vec::new();
            for account in &accounts {
                if let Some(identity) = identities::for_addresses(conn, account, &[address.as_str()])? {
                    found.push(identity);
                }
            }
            Ok(found)
        })
        .await
        .map_err(crate::session::internal)?;
    match <[Identity; 1]>::try_from(found) {
        Ok([identity]) => Ok(identity),
        Err(found) if found.is_empty() => Err(RpcError::new(
            codes::NOT_FOUND,
            "that is not one of the person's addresses in an account open to you",
        )),
        Err(_) => Err(RpcError::new(
            codes::INVALID_PARAMS,
            "that address is in more than one account: give account_id",
        )),
    }
}

/// What an agent's suggested voice is about, as the person is told; checked
/// before it waits.
pub(crate) async fn held(
    state: &State,
    params: &VoiceSuggestParams,
    scope: Option<Vec<String>>,
) -> Result<About, RpcError> {
    checked(&params.text, false)?;
    let identity = resolve(state, &params.address, params.account_id.clone(), scope).await?;
    let description = format!("Change your voice for {}", identity.email);
    Ok(About {
        account_id: identity.account_id,
        reason: Reason::Voice,
        subject: description.clone(),
        description,
        draft: None,
        people: Vec::new(),
    })
}

/// A suggested voice, approved: saved and switched on, as the person
/// edited it (`text`) or as suggested.
pub(crate) async fn apply(state: &State, params: VoiceSuggestParams, text: Option<String>) -> Result<Value, RpcError> {
    let voice = checked(text.as_deref().unwrap_or(&params.text), false)?;
    let identity = resolve(state, &params.address, params.account_id, None).await?;
    save(state, identity.id, voice, true).await
}

/// Check an edited voice before a hold is decided on it.
pub(crate) fn check_edit(text: &str) -> Result<(), RpcError> {
    checked(text, false).map(|_| ())
}
