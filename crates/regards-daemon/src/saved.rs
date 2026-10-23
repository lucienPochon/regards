//! Saved searches (Spark's Smart Folders): a search kept by name, which the
//! sidebar runs again when it is opened. The daemon keeps them; `ui` only.

use crate::State;
use crate::compose::now_ms;
use regards_ipc::protocol::{
    RpcError, SavedSearchListResult, SavedSearchOrderParams, SavedSearchRef, SavedSearchSaveParams, SavedSearchView,
    codes,
};
use regards_store::saved_searches::{self, Fields, SavedSearch};
use regards_store::{StoreError, accounts};
use serde_json::{Value, json};

const MAX_NAME: usize = 100;
/// As a search may be (`mail.rs`).
const MAX_QUERY: usize = 1000;

pub(crate) async fn list(state: &State) -> Result<Value, RpcError> {
    let found = state
        .store
        .read(saved_searches::list)
        .await
        .map_err(crate::session::internal)?;
    Ok(json!(SavedSearchListResult {
        searches: found.into_iter().map(view).collect(),
    }))
}

/// Keep a search (no `id`, last in the order) or change one.
pub(crate) async fn save(state: &State, params: SavedSearchSaveParams) -> Result<Value, RpcError> {
    let invalid = |message: &str| Err(RpcError::new(codes::INVALID_PARAMS, message));
    let name = params.name.trim().to_owned();
    if name.is_empty() || name.chars().count() > MAX_NAME {
        return invalid("give the saved search a name");
    }
    let query = params.query.trim().to_owned();
    if query.is_empty() || query.len() > MAX_QUERY || regards_core::search::parse(&query).is_empty() {
        return invalid("there is nothing to search for");
    }
    let color = params.color.map(|color| color.trim().to_ascii_lowercase());
    if color.as_deref().is_some_and(|color| {
        !(color.len() == 7 && color.starts_with('#') && color[1..].bytes().all(|b| b.is_ascii_hexdigit()))
    }) {
        return invalid("a colour is #rrggbb");
    }
    let fields = Fields {
        name,
        query,
        account_id: params.account_id,
        color,
    };
    let (id, now) = (params.id, now_ms());
    let saved = state
        .store
        .write(move |tx| {
            if let Some(account) = &fields.account_id
                && accounts::get(tx, account)?.is_none()
            {
                return Err(StoreError::Conflict("no such account".into()));
            }
            let id = match id {
                None => saved_searches::add(tx, &fields, now)?,
                Some(id) if saved_searches::update(tx, id, &fields, now)? => id,
                Some(_) => return Err(StoreError::Conflict("no such saved search".into())),
            };
            saved_searches::get(tx, id)?.ok_or_else(|| StoreError::Conflict("no such saved search".into()))
        })
        .await
        .map_err(not_found)?
        .value;
    Ok(json!(view(saved)))
}

pub(crate) async fn reorder(state: &State, params: SavedSearchOrderParams) -> Result<Value, RpcError> {
    let ids = params.ids;
    state
        .store
        .write(move |tx| saved_searches::reorder(tx, &ids))
        .await
        .map_err(crate::session::internal)?;
    list(state).await
}

pub(crate) async fn remove(state: &State, params: SavedSearchRef) -> Result<Value, RpcError> {
    let id = params.id;
    let removed = state
        .store
        .write(move |tx| saved_searches::remove(tx, id))
        .await
        .map_err(crate::session::internal)?
        .value;
    if !removed {
        return Err(RpcError::new(codes::NOT_FOUND, "no such saved search"));
    }
    Ok(json!({ "removed": true }))
}

fn view(saved: SavedSearch) -> SavedSearchView {
    SavedSearchView {
        id: saved.id,
        name: saved.name,
        query: saved.query,
        account_id: saved.account_id,
        color: saved.color,
    }
}

fn not_found(error: StoreError) -> RpcError {
    match error {
        StoreError::Conflict(message) => RpcError::new(codes::NOT_FOUND, message),
        other => crate::session::internal(other),
    }
}
