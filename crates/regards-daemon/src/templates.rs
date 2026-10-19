//! Templates (plan §5.9): text to start a message with. The daemon keeps
//! them; the composer puts one into a draft. `ui` only.

use crate::State;
use crate::compose::now_ms;
use regards_ipc::protocol::{RpcError, TemplateListResult, TemplateRef, TemplateSaveParams, TemplateView, codes};
use regards_store::StoreError;
use regards_store::templates::{self, Fields, Template};
use serde_json::{Value, json};

const MAX_NAME: usize = 200;
const MAX_SUBJECT: usize = 1000;
/// Templates are a few paragraphs, not documents.
const MAX_BODY: usize = 256 * 1024;

pub(crate) async fn list(state: &State) -> Result<Value, RpcError> {
    let found = state
        .store
        .read(templates::list)
        .await
        .map_err(crate::session::internal)?;
    Ok(json!(TemplateListResult {
        templates: found.into_iter().map(view).collect(),
    }))
}

/// Add a template (no `id`) or change one. Formatted text is sanitised as an
/// outgoing message's is, and its plain-text version derived from it.
pub(crate) async fn save(state: &State, params: TemplateSaveParams) -> Result<Value, RpcError> {
    let name = params.name.trim().to_owned();
    if name.is_empty() || name.len() > MAX_NAME {
        return Err(RpcError::new(codes::INVALID_PARAMS, "give the template a name"));
    }
    if params.subject.len() > MAX_SUBJECT
        || params.body.len() > MAX_BODY
        || params.html.as_ref().is_some_and(|html| html.len() > MAX_BODY)
    {
        return Err(RpcError::new(codes::TOO_LARGE, "the template is too long"));
    }
    let (body, html) = match &params.html {
        Some(written) => {
            let clean = regards_core::html::sanitize_outgoing(written);
            (regards_core::html::to_text(&clean), Some(clean))
        }
        None => (params.body.clone(), None),
    };
    let fields = Fields {
        name,
        subject: params.subject.trim().to_owned(),
        body,
        html,
    };
    let (id, now) = (params.id, now_ms());
    let saved = state
        .store
        .write(move |tx| {
            let id = match id {
                None => templates::add(tx, &fields, now)?,
                Some(id) if templates::update(tx, id, &fields, now)? => id,
                Some(_) => return Err(StoreError::Conflict("no such template".into())),
            };
            templates::get(tx, id)?.ok_or_else(|| StoreError::Conflict("no such template".into()))
        })
        .await
        .map_err(not_found)?
        .value;
    Ok(json!(view(saved)))
}

pub(crate) async fn remove(state: &State, params: TemplateRef) -> Result<Value, RpcError> {
    let id = params.id;
    let removed = state
        .store
        .write(move |tx| templates::remove(tx, id))
        .await
        .map_err(crate::session::internal)?
        .value;
    if !removed {
        return Err(RpcError::new(codes::NOT_FOUND, "no such template"));
    }
    Ok(json!({ "removed": true }))
}

fn not_found(error: StoreError) -> RpcError {
    match error {
        StoreError::Conflict(message) => RpcError::new(codes::NOT_FOUND, message),
        other => crate::session::internal(other),
    }
}

fn view(template: Template) -> TemplateView {
    TemplateView {
        id: template.id,
        name: template.name,
        subject: template.subject,
        body: template.body,
        html: template.html,
        updated_at: template.updated_at,
    }
}
