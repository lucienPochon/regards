//! Templates (plan §5.9): a message to start from: subject, text,
//! recipients and files. The daemon keeps them; the composer puts one into
//! a draft, and its files on it ([`use_on_draft`]). `ui` only.

use crate::State;
use crate::compose::now_ms;
use regards_ipc::protocol::{
    DraftUseTemplateParams, RpcError, TemplateFile, TemplateListResult, TemplateRef, TemplateSaveParams, TemplateView,
    codes,
};
use regards_store::StoreError;
use regards_store::templates::{self, Fields, File, Template};
use serde_json::{Value, json};

const MAX_NAME: usize = 200;
const MAX_SUBJECT: usize = 1000;
/// Templates are a few paragraphs, not documents.
const MAX_BODY: usize = 256 * 1024;
/// Recipients a template may keep, To, Cc and Bcc together.
const MAX_RECIPIENTS: usize = 100;

pub(crate) async fn list(state: &State) -> Result<Value, RpcError> {
    let found = state
        .store
        .read(|conn| {
            templates::list(conn)?
                .into_iter()
                .map(|template| Ok((templates::files(conn, template.id)?, template)))
                .collect::<Result<Vec<_>, StoreError>>()
        })
        .await
        .map_err(crate::session::internal)?;
    Ok(json!(TemplateListResult {
        templates: found
            .into_iter()
            .map(|(files, template)| view(template, files))
            .collect(),
    }))
}

/// Add a template (no `id`) or change one. Formatted text is sanitised as an
/// outgoing message's is, and its plain-text version derived from it. With
/// `files_of_draft`, it keeps that draft's files (the same stored bytes).
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
    if params.to.len() + params.cc.len() + params.bcc.len() > MAX_RECIPIENTS {
        return Err(RpcError::new(
            codes::TOO_LARGE,
            "a template can keep 100 recipients at most",
        ));
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
        to: params.to,
        cc: params.cc,
        bcc: params.bcc,
    };
    let (id, now, files_of, removed) = (params.id, now_ms(), params.files_of_draft, params.remove_files);
    let saved = state
        .store
        .write(move |tx| {
            let id = match id {
                None => templates::add(tx, &fields, now)?,
                Some(id) if templates::update(tx, id, &fields, now)? => id,
                Some(_) => return Err(StoreError::Conflict("no such template".into())),
            };
            for file in removed {
                templates::remove_file(tx, id, file)?;
            }
            if let Some(draft) = files_of {
                templates::keep_draft_files(tx, id, draft)?;
            }
            let template = templates::get(tx, id)?.ok_or_else(|| StoreError::Conflict("no such template".into()))?;
            Ok((templates::files(tx, id)?, template))
        })
        .await
        .map_err(not_found)?
        .value;
    Ok(json!(view(saved.1, saved.0)))
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

/// Put a template's files on a draft (those it does not have); the draft,
/// as it is then.
pub(crate) async fn use_on_draft(state: &State, params: DraftUseTemplateParams) -> Result<Value, RpcError> {
    let (draft_id, template_id) = (params.draft_id, params.template_id);
    state
        .store
        .write(move |tx| {
            if templates::get(tx, template_id)?.is_none() {
                return Err(StoreError::Conflict("no such template".into()));
            }
            templates::put_files_on(tx, template_id, draft_id)
        })
        .await
        .map_err(not_found)?;
    crate::compose::view(state, draft_id).await
}

fn not_found(error: StoreError) -> RpcError {
    match error {
        StoreError::Conflict(message) => RpcError::new(codes::NOT_FOUND, message),
        other => crate::session::internal(other),
    }
}

fn view(template: Template, files: Vec<File>) -> TemplateView {
    TemplateView {
        id: template.id,
        name: template.name,
        subject: template.subject,
        body: template.body,
        html: template.html,
        to: template.to,
        cc: template.cc,
        bcc: template.bcc,
        files: files
            .into_iter()
            .map(|file| TemplateFile {
                id: file.id,
                name: file.name,
                mime: file.mime,
                size: file.size,
                inline: file.content_id.is_some(),
            })
            .collect(),
        updated_at: template.updated_at,
    }
}
