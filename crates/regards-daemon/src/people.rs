//! `people.suggest`: whom to write to, as one types a recipient. The store
//! keeps who each account writes with (`regards_store::people`); agents get
//! only the people of accounts open to them.

use crate::State;
use regards_core::headers::Person;
use regards_ipc::protocol::{
    PeopleForgetParams, PeopleForgetResult, PeopleSuggestParams, PeopleSuggestResult, RpcError, codes,
};
use serde_json::{Value, json};

const DEFAULT_LIMIT: u32 = 8;
const MAX_LIMIT: u32 = 50;
const MAX_TYPED: usize = 200;

/// `scope`: the accounts an agent may read; `None` for the app (all, or the one asked for).
pub(crate) async fn suggest(
    state: &State,
    params: PeopleSuggestParams,
    scope: Option<Vec<String>>,
) -> Result<Value, RpcError> {
    let limit = params.limit.unwrap_or(DEFAULT_LIMIT);
    if !(1..=MAX_LIMIT).contains(&limit) {
        return Err(RpcError::new(
            codes::INVALID_PARAMS,
            format!("limit must be 1 to {MAX_LIMIT}"),
        ));
    }
    if params.query.len() > MAX_TYPED {
        return Err(RpcError::new(codes::INVALID_PARAMS, "that is too long to look for"));
    }
    let accounts = scope.or(params.account_id.map(|account| vec![account]));
    let typed = params.query;
    let now = crate::compose::now_ms();
    let found = state
        .store
        .read(move |conn| regards_store::people::suggest(conn, &typed, accounts.as_deref(), limit, now))
        .await
        .map_err(crate::session::internal)?;
    Ok(json!(PeopleSuggestResult {
        people: found
            .into_iter()
            .map(|person| Person {
                name: person.name,
                address: Some(person.address),
            })
            .collect(),
    }))
}

/// Not to be suggested any more, until there is new mail with them. The app only.
pub(crate) async fn forget(state: &State, params: PeopleForgetParams) -> Result<Value, RpcError> {
    let address = params.address.trim().to_owned();
    if !address.contains('@') || address.len() > MAX_TYPED {
        return Err(RpcError::new(codes::INVALID_PARAMS, "that is not an address"));
    }
    let accounts = params.account_id.map(|account| vec![account]);
    let now = crate::compose::now_ms();
    let forgotten = state
        .store
        .write(move |tx| regards_store::people::forget(tx, &address, accounts.as_deref(), now))
        .await
        .map_err(crate::session::internal)?
        .value;
    Ok(json!(PeopleForgetResult {
        forgotten: u32::try_from(forgotten).unwrap_or(u32::MAX),
    }))
}
