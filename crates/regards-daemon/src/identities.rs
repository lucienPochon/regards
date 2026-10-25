//! Identities (plan §5.9): the names and addresses an account sends as, and
//! their default signatures (ADR-0021). `ui` only: an agent never picks
//! who mail comes from.

use crate::State;
use crate::compose::now_ms;
use regards_ipc::protocol::{
    AccountRef, IdentityListResult, IdentityRef, IdentitySaveParams, IdentitySetSignatureParams, IdentityView,
    RpcError, codes,
};
use regards_store::identities::{self, Fields, Identity};
use regards_store::signatures::{self, BODY_MAX};
use regards_store::{StoreError, accounts};
use serde_json::{Value, json};

/// Longer than this is not a name or an address.
const MAX_NAME: usize = 200;
const MAX_EMAIL: usize = 320;

/// The default signature asked for: chosen from the list, or, as before the
/// list, given as text (the signature with that text, made if need be).
enum Asked {
    Chosen(Option<i64>),
    Text(String),
}

pub(crate) async fn list(state: &State, params: AccountRef) -> Result<Value, RpcError> {
    let account_id = params.account_id;
    let found = state
        .store
        .read(move |conn| {
            if accounts::get(conn, &account_id)?.is_none() {
                return Ok(None);
            }
            Ok(Some(identities::list(conn, &account_id)?))
        })
        .await
        .map_err(crate::session::internal)?
        .ok_or_else(|| RpcError::new(codes::NOT_FOUND, "no such account"))?;
    Ok(json!(IdentityListResult {
        identities: found.into_iter().map(view).collect(),
    }))
}

pub(crate) async fn save(state: &State, params: IdentitySaveParams) -> Result<Value, RpcError> {
    let mut fields = Fields {
        name: params.name.trim().to_owned(),
        email: params.email.trim().to_owned(),
        signature_id: None,
    };
    let asked = match params.signature_id {
        Some(chosen) => Asked::Chosen(chosen),
        None => Asked::Text(params.signature.trim_end().to_owned()),
    };
    check(&fields, &asked)?;
    let (id, account_id, now) = (params.id, params.account_id, now_ms());
    let saved = state
        .store
        .write(move |tx| {
            if accounts::get(tx, &account_id)?.is_none() {
                return Err(StoreError::Conflict("no such account".into()));
            }
            fields.signature_id = match asked {
                Asked::Chosen(chosen) => chosen,
                Asked::Text(text) if text.trim().is_empty() => None,
                Asked::Text(text) => Some(match signatures::with_body(tx, &text)? {
                    Some(found) => found.id,
                    None => match own_signature(tx, id, &account_id)? {
                        // Its own signature's text changed: that one, edited, not a new copy each time.
                        Some(own) => {
                            let edited = signatures::Fields {
                                name: own.name,
                                body: text,
                                // Edited as text: no formatting from then on.
                                html: None,
                            };
                            signatures::update(tx, own.id, &edited, now)?;
                            own.id
                        }
                        None => {
                            let named = signatures::Fields {
                                name: fields.email.clone(),
                                body: text,
                                html: None,
                            };
                            signatures::add(tx, &named, now)?
                        }
                    },
                }),
            };
            let id = match id {
                None => identities::add(tx, &account_id, &fields, now)?,
                Some(id) => {
                    let mine = identities::get(tx, id)?.is_some_and(|i| i.account_id == account_id);
                    if !mine || !identities::update(tx, id, &fields)? {
                        return Err(StoreError::Conflict("no such identity".into()));
                    }
                    id
                }
            };
            identities::get(tx, id)?.ok_or_else(|| StoreError::Conflict("no such identity".into()))
        })
        .await
        .map_err(conflict)?
        .value;
    Ok(json!(view(saved)))
}

pub(crate) async fn remove(state: &State, params: IdentityRef) -> Result<Value, RpcError> {
    let id = params.id;
    let removed = state
        .store
        .write(move |tx| identities::remove(tx, id))
        .await
        .map_err(conflict)?
        .value;
    if !removed {
        return Err(RpcError::new(codes::NOT_FOUND, "no such identity"));
    }
    Ok(json!({ "removed": true }))
}

pub(crate) async fn make_default(state: &State, params: IdentityRef) -> Result<Value, RpcError> {
    let id = params.id;
    let made = state
        .store
        .write(move |tx| identities::make_default(tx, id))
        .await
        .map_err(conflict)?
        .value;
    if !made {
        return Err(RpcError::new(codes::NOT_FOUND, "no such identity"));
    }
    Ok(json!({ "default": true }))
}

/// The signature an existing identity of `account_id` has as its default,
/// if no other identity has it too: editing its text edits that signature.
fn own_signature(
    conn: &rusqlite::Connection,
    id: Option<i64>,
    account_id: &str,
) -> Result<Option<signatures::Signature>, StoreError> {
    let Some(identity) = id.map(|id| identities::get(conn, id)).transpose()?.flatten() else {
        return Ok(None);
    };
    let Some(signature_id) = identity.signature_id.filter(|_| identity.account_id == account_id) else {
        return Ok(None);
    };
    if identities::sharing_signature(conn, signature_id, identity.id)? > 0 {
        return Ok(None);
    }
    signatures::get(conn, signature_id)
}

fn check(fields: &Fields, asked: &Asked) -> Result<(), RpcError> {
    let invalid = |message: &str| Err(RpcError::new(codes::INVALID_PARAMS, message));
    let email = &fields.email;
    let (local, domain) = email.rsplit_once('@').unwrap_or_default();
    if local.is_empty() || domain.is_empty() || email.chars().any(|c| c.is_whitespace() || c == '<' || c == '>') {
        return invalid("enter an address like name@example.org");
    }
    if email.len() > MAX_EMAIL || fields.name.len() > MAX_NAME {
        return invalid("that name or address is too long");
    }
    if let Asked::Text(text) = asked
        && text.chars().count() > BODY_MAX
    {
        return invalid("the signature is too long");
    }
    Ok(())
}

/// Set only an identity's default signature, or none.
pub(crate) async fn set_signature(state: &State, params: IdentitySetSignatureParams) -> Result<Value, RpcError> {
    let (id, signature_id) = (params.id, params.signature_id);
    let saved = state
        .store
        .write(move |tx| {
            if !identities::set_signature(tx, id, signature_id)? {
                return Ok(None);
            }
            identities::get(tx, id)
        })
        .await
        .map_err(conflict)?
        .value
        .ok_or_else(|| RpcError::new(codes::NOT_FOUND, "no such identity"))?;
    Ok(json!(view(saved)))
}

fn conflict(error: StoreError) -> RpcError {
    match error {
        StoreError::Conflict(message) => RpcError::new(codes::CONFLICT, message),
        other => crate::session::internal(other),
    }
}

pub(crate) fn view(identity: Identity) -> IdentityView {
    IdentityView {
        id: identity.id,
        account_id: identity.account_id,
        name: identity.name,
        email: identity.email,
        signature_id: identity.signature_id,
        signature: identity.signature,
        voice: identity.voice,
        voice_on: identity.voice_on,
        is_default: identity.is_default,
    }
}
