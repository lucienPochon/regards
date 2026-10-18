//! Account methods. Only the `ui` principal reaches these (ADR-0006).

use crate::{State, sync};
use regards_imap::{ImapError, ImapServer};
use regards_ipc::protocol::{
    AccountAddParams, AccountCheckFailure, AccountRef, AccountRemoveParams, AccountSummary, ConnectionSecurity,
    MailboxListResult, MailboxSummary, RpcError, ServerSettings, SyncResult, codes,
};
use regards_store::StoreError;
use regards_store::accounts::{self, Account, AuthKind, NewAccount, Security, Server};
use regards_store::mailboxes;
use regards_vault::{SecretKey, SecretKind, Vault, VaultError};
use serde_json::{Value, json};
use std::sync::Arc;

pub(crate) async fn list(state: &State) -> Result<Value, RpcError> {
    let accounts = state.store.read(accounts::list).await.map_err(store_error)?;
    Ok(json!(accounts.into_iter().map(summary).collect::<Vec<_>>()))
}

pub(crate) async fn add(state: &Arc<State>, params: AccountAddParams) -> Result<Value, RpcError> {
    let new = validate(params)?;

    // Refuse a duplicate before contacting the server.
    let existing = state.store.read(accounts::list).await.map_err(store_error)?;
    if existing
        .iter()
        .any(|account| account.email.eq_ignore_ascii_case(&new.email))
    {
        return Err(conflict(&new.email));
    }

    let server = ImapServer {
        host: new.imap.host.clone(),
        port: new.imap.port,
    };
    let check = regards_imap::verify_login(&server, &new.username, &new.password, &state.imap_tls)
        .await
        .map_err(check_failed)?;

    let id = accounts::new_id();
    let key = SecretKey::new(&id, SecretKind::MailPassword);
    let vault = vault(state).await?;
    vault.set(&key, &new.password).await.map_err(vault_error)?;

    let record = NewAccount {
        id: id.clone(),
        display_name: new.display_name,
        email: new.email,
        auth_kind: AuthKind::Password,
        username: new.username,
        secret_key: key.as_str().to_owned(),
        imap: new.imap,
        smtp: new.smtp,
        imap_capabilities: check.capabilities,
    };
    match state.store.write(move |tx| accounts::insert(tx, record)).await {
        Ok(committed) => {
            tracing::info!(account = %id, "account added");
            sync::spawn_one(Arc::clone(state), id);
            Ok(json!(summary(committed.value)))
        }
        Err(error) => {
            // Do not leave an orphaned password behind.
            if let Err(cleanup) = vault.delete(&key).await {
                tracing::warn!(error = %cleanup, "could not remove the password of an account that was not added");
            }
            Err(store_error(error))
        }
    }
}

pub(crate) async fn remove(state: &State, params: AccountRemoveParams) -> Result<Value, RpcError> {
    let id = params.id;
    let removed = state
        .store
        .write({
            let id = id.clone();
            move |tx| accounts::delete(tx, &id)
        })
        .await
        .map_err(store_error)?
        .value;
    let Some(account) = removed else {
        return Err(RpcError::new(codes::NOT_FOUND, "no such account"));
    };
    let key = SecretKey::from_stored(account.secret_key);
    match vault(state).await {
        Ok(vault) => {
            if let Err(error) = vault.delete(&key).await {
                tracing::warn!(account = %id, %error, "account removed, but its password could not be deleted");
            }
        }
        Err(error) => tracing::warn!(account = %id, ?error, "account removed, but the credential store is unavailable"),
    }
    state.sync.forget(&id);
    tracing::info!(account = %id, "account removed");
    Ok(json!({ "removed": true }))
}

/// A validated `account.add` request.
struct Validated {
    display_name: String,
    email: String,
    username: String,
    password: regards_vault::Secret,
    imap: Server,
    smtp: Server,
}

fn validate(params: AccountAddParams) -> Result<Validated, RpcError> {
    let email = params.email.trim().to_owned();
    let valid_email = email
        .split_once('@')
        .is_some_and(|(local, domain)| !local.is_empty() && domain.contains('.') && !domain.ends_with('.'))
        && !email.chars().any(char::is_whitespace);
    if !valid_email {
        return Err(invalid("enter a valid email address"));
    }
    if params.password.expose().is_empty() {
        return Err(invalid("enter the password"));
    }
    if params.imap.security != ConnectionSecurity::Tls {
        return Err(invalid(
            "IMAP over STARTTLS is not supported yet; use the server's TLS port (usually 993)",
        ));
    }
    let display_name = match params.display_name.trim() {
        "" => email.clone(),
        name => name.to_owned(),
    };
    let username = match params.username.as_deref().map(str::trim) {
        None | Some("") => email.clone(),
        Some(name) => name.to_owned(),
    };
    Ok(Validated {
        display_name,
        email,
        username,
        password: params.password,
        imap: server(params.imap, "IMAP")?,
        smtp: server(params.smtp, "SMTP")?,
    })
}

fn server(settings: ServerSettings, label: &str) -> Result<Server, RpcError> {
    let host = settings.host.trim().to_ascii_lowercase();
    if host.is_empty() || host.chars().any(|c| c.is_whitespace() || c == '/' || c == ':') {
        return Err(invalid(&format!(
            "enter the {label} server name, for example mail.example.com"
        )));
    }
    if settings.port == 0 {
        return Err(invalid(&format!("enter the {label} port")));
    }
    Ok(Server {
        host,
        port: settings.port,
        security: match settings.security {
            ConnectionSecurity::Tls => Security::Tls,
            ConnectionSecurity::Starttls => Security::Starttls,
        },
    })
}

fn summary(account: Account) -> AccountSummary {
    let settings = |server: Server| ServerSettings {
        host: server.host,
        port: server.port,
        security: match server.security {
            Security::Tls => ConnectionSecurity::Tls,
            Security::Starttls => ConnectionSecurity::Starttls,
        },
    };
    AccountSummary {
        id: account.id,
        display_name: account.display_name,
        email: account.email,
        username: account.username,
        imap: settings(account.imap),
        smtp: settings(account.smtp),
        imap_capabilities: account.imap_capabilities,
        created_at: account.created_at,
    }
}

pub(crate) async fn vault(state: &State) -> Result<Vault, RpcError> {
    state
        .vault
        .get_or_init(|| async {
            tokio::task::spawn_blocking(Vault::native)
                .await
                .unwrap_or_else(|error| Err(VaultError::Unavailable(error.to_string())))
                .map_err(|error| error.to_string())
        })
        .await
        .clone()
        .map_err(|message| {
            RpcError::new(
                codes::CREDENTIAL_STORE,
                format!("the system credential store is not available: {message}"),
            )
        })
}

fn check_failed(error: ImapError) -> RpcError {
    let reason = match error {
        ImapError::Connect(_) => AccountCheckFailure::Connect,
        ImapError::Tls(_) => AccountCheckFailure::Tls,
        ImapError::Timeout(_) => AccountCheckFailure::Timeout,
        ImapError::NotImap(_) => AccountCheckFailure::NotImap,
        ImapError::AuthFailed(_) => AccountCheckFailure::AuthFailed,
        ImapError::LoginDisabled => AccountCheckFailure::LoginDisabled,
        ImapError::Protocol(_) => AccountCheckFailure::Protocol,
    };
    let mut rpc = RpcError::new(codes::ACCOUNT_CHECK_FAILED, error.to_string());
    rpc.data = Some(json!({ "reason": reason }));
    rpc
}

fn vault_error(error: VaultError) -> RpcError {
    RpcError::new(codes::CREDENTIAL_STORE, error.to_string())
}

fn store_error(error: StoreError) -> RpcError {
    match error {
        StoreError::Conflict(message) => RpcError::new(codes::CONFLICT, message),
        other => crate::session::internal(other),
    }
}

fn conflict(email: &str) -> RpcError {
    RpcError::new(codes::CONFLICT, format!("an account for {email} already exists"))
}

fn invalid(message: &str) -> RpcError {
    RpcError::new(codes::INVALID_PARAMS, message)
}

pub(crate) async fn mailboxes(state: &State, params: AccountRef) -> Result<Value, RpcError> {
    ensure_exists(state, &params.account_id).await?;
    let id = params.account_id.clone();
    let stored = state
        .store
        .read(move |conn| mailboxes::list(conn, &id))
        .await
        .map_err(store_error)?;
    Ok(json!(MailboxListResult {
        mailboxes: stored
            .into_iter()
            .map(|m| MailboxSummary {
                id: m.id,
                name: m.name,
                delimiter: m.delimiter,
                role: m.role,
                selectable: m.selectable,
            })
            .collect(),
        sync: state.sync.get(&params.account_id),
    }))
}

pub(crate) async fn sync_now(state: &State, params: AccountRef) -> Result<Value, RpcError> {
    ensure_exists(state, &params.account_id).await?;
    match sync::sync_account(state, &params.account_id).await {
        None => Ok(json!(SyncResult {
            already_running: true,
            mailboxes_added: 0,
            mailboxes_updated: 0,
            mailboxes_removed: 0,
        })),
        Some(Ok(changes)) => Ok(json!(SyncResult {
            already_running: false,
            mailboxes_added: changes.added,
            mailboxes_updated: changes.updated,
            mailboxes_removed: changes.removed,
        })),
        Some(Err(error)) => Err(RpcError::new(codes::SYNC_FAILED, error.to_string())),
    }
}

async fn ensure_exists(state: &State, account_id: &str) -> Result<(), RpcError> {
    let id = account_id.to_owned();
    match state
        .store
        .read(move |conn| accounts::get(conn, &id))
        .await
        .map_err(store_error)?
    {
        Some(_) => Ok(()),
        None => Err(RpcError::new(codes::NOT_FOUND, "no such account")),
    }
}
