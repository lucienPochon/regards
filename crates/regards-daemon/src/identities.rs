//! Identities (plan §5.9): the names and addresses an account sends as, and
//! their signatures. `ui` only: an agent never picks who mail comes from.

use crate::State;
use crate::compose::now_ms;
use regards_ipc::protocol::{
    AccountRef, IdentityListResult, IdentityRef, IdentitySaveParams, IdentityView, RpcError, codes,
};
use regards_store::identities::{self, Fields, Identity};
use regards_store::{StoreError, accounts};
use serde_json::{Value, json};

/// Longer than this is not a name, an address or a signature.
const MAX_NAME: usize = 200;
const MAX_EMAIL: usize = 320;
const MAX_SIGNATURE: usize = 10_000;

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
    let fields = Fields {
        name: params.name.trim().to_owned(),
        email: params.email.trim().to_owned(),
        signature: params.signature.trim_end().to_owned(),
    };
    check(&fields)?;
    let (id, account_id, now) = (params.id, params.account_id, now_ms());
    let saved = state
        .store
        .write(move |tx| {
            if accounts::get(tx, &account_id)?.is_none() {
                return Err(StoreError::Conflict("no such account".into()));
            }
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

fn check(fields: &Fields) -> Result<(), RpcError> {
    let invalid = |message: &str| Err(RpcError::new(codes::INVALID_PARAMS, message));
    let email = &fields.email;
    let (local, domain) = email.rsplit_once('@').unwrap_or_default();
    if local.is_empty() || domain.is_empty() || email.chars().any(|c| c.is_whitespace() || c == '<' || c == '>') {
        return invalid("enter an address like name@example.org");
    }
    if email.len() > MAX_EMAIL || fields.name.len() > MAX_NAME {
        return invalid("that name or address is too long");
    }
    if fields.signature.len() > MAX_SIGNATURE {
        return invalid("the signature is too long");
    }
    Ok(())
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
        signature: identity.signature,
        is_default: identity.is_default,
    }
}
