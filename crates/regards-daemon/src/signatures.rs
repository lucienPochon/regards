//! Signatures (ADR-0021): a list for every account, each identity using one
//! as its default. `ui` only: agents' drafts get the default, as the
//! person's do, and they never manage the list.

use crate::State;
use crate::compose::now_ms;
use regards_ipc::protocol::{RpcError, SignatureListResult, SignatureRef, SignatureSaveParams, SignatureView, codes};
use regards_store::StoreError;
use regards_store::signatures::{self, BODY_MAX, Fields, HTML_MAX, NAME_MAX, Signature};
use serde_json::{Value, json};

pub(crate) async fn list(state: &State) -> Result<Value, RpcError> {
    let found = state
        .store
        .read(signatures::list)
        .await
        .map_err(crate::session::internal)?;
    Ok(json!(SignatureListResult {
        signatures: found.into_iter().map(view).collect(),
    }))
}

pub(crate) async fn save(state: &State, params: SignatureSaveParams) -> Result<Value, RpcError> {
    let invalid = |message: &str| Err(RpcError::new(codes::INVALID_PARAMS, message));
    let html = params
        .html
        .as_deref()
        .map(regards_core::html::sanitize_signature)
        .filter(|html| !html.trim().is_empty());
    // With formatting, its text is the editor's (which keeps spaces and line
    // breaks between styled words), or else made from the HTML.
    let body = match &html {
        Some(html) if params.body.trim().is_empty() => regards_core::html::to_text(html),
        _ => params.body,
    };
    let fields = Fields {
        name: params.name.trim().to_owned(),
        body: body.trim_end().to_owned(),
        html,
    };
    if fields.name.is_empty() || fields.body.trim().is_empty() {
        return invalid("a signature needs a name and some text");
    }
    if fields.name.chars().count() > NAME_MAX
        || fields.body.chars().count() > BODY_MAX
        || fields.html.as_ref().is_some_and(|html| html.len() > HTML_MAX)
    {
        return invalid("that name or signature is too long");
    }
    let (id, now) = (params.id, now_ms());
    let saved = state
        .store
        .write(move |tx| {
            let id = match id {
                None => signatures::add(tx, &fields, now)?,
                Some(id) if signatures::update(tx, id, &fields, now)? => id,
                Some(_) => return Ok(None),
            };
            signatures::get(tx, id)
        })
        .await
        .map_err(crate::session::internal)?
        .value
        .ok_or_else(|| RpcError::new(codes::NOT_FOUND, "no such signature"))?;
    Ok(json!(view(saved)))
}

pub(crate) async fn remove(state: &State, params: SignatureRef) -> Result<Value, RpcError> {
    let id = params.id;
    let removed = state
        .store
        .write(move |tx| signatures::remove(tx, id))
        .await
        .map_err(|error: StoreError| crate::session::internal(error))?
        .value;
    if !removed {
        return Err(RpcError::new(codes::NOT_FOUND, "no such signature"));
    }
    Ok(json!({ "removed": true }))
}

fn view(signature: Signature) -> SignatureView {
    SignatureView {
        id: signature.id,
        name: signature.name,
        body: signature.body,
        html: signature.html,
        updated_at: signature.updated_at,
    }
}
