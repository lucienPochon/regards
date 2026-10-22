//! Account methods. Only the `ui` principal reaches these (ADR-0006), but
//! for the list and an account's folders, which agents see of the accounts
//! open to them (ADR-0012).

use crate::{State, sync};
use regards_imap::{ImapError, ImapServer};
use regards_ipc::protocol::{
    AccountAddParams, AccountCheckFailure, AccountRef, AccountRemoveParams, AccountResyncResult, AccountSummary,
    AccountUpdateParams, AgentAccess, AgentAccessParams, AgentSendParams, AutoCopyParams, ConnectionSecurity,
    MailboxListResult, MailboxSummary, RpcError, ServerSettings, SignIn, SyncResult, codes,
};
use regards_store::StoreError;
use regards_store::accounts::{self, Account, AuthKind, NewAccount, Security, Server};
use regards_store::mailboxes;
use regards_vault::{SecretKey, SecretKind, Vault, VaultError};
use serde_json::{Value, json};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// The accounts; to an agent, only those open to it.
pub(crate) async fn list(state: &State, agent: bool) -> Result<Value, RpcError> {
    let (accounts, levels, windows, copies) = state
        .store
        .read(|conn| {
            let accounts = accounts::list(conn)?;
            let copies = accounts
                .iter()
                .map(|account| Ok((account.id.clone(), accounts::auto_copy(conn, &account.id)?)))
                .collect::<Result<std::collections::HashMap<_, _>, StoreError>>()?;
            Ok((
                accounts,
                accounts::agent_access(conn)?,
                accounts::agent_send_windows(conn)?,
                copies,
            ))
        })
        .await
        .map_err(store_error)?;
    let levels: std::collections::HashMap<String, AgentAccess> = levels.into_iter().collect();
    let windows: std::collections::HashMap<String, Option<u32>> = windows.into_iter().collect();
    let summaries: Vec<AccountSummary> = accounts
        .into_iter()
        .map(|account| {
            let access = levels.get(&account.id).copied().unwrap_or_default();
            AccountSummary {
                agent_access: access,
                agent_send_window: windows.get(&account.id).copied().flatten(),
                auto_copy: copies.get(&account.id).cloned().flatten(),
                needs_sign_in: state.oauth.needs_sign_in(&account.id),
                ..summary(account)
            }
        })
        .filter(|account| !agent || account.agent_access >= AgentAccess::Read)
        .collect();
    Ok(json!(summaries))
}

/// What agents may do with an account (ADR-0012). Only the app gets here.
pub(crate) async fn set_agent_access(state: &State, params: AgentAccessParams) -> Result<Value, RpcError> {
    let (id, access) = (params.account_id, params.access);
    let found = state
        .store
        .write(move |tx| accounts::set_agent_access(tx, &id, access))
        .await
        .map_err(store_error)?
        .value;
    if !found {
        return Err(RpcError::new(codes::NOT_FOUND, "no such account"));
    }
    tracing::info!(access = access.as_str(), "agent access changed");
    Ok(json!({ "agent_access": access }))
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
    let check = regards_imap::verify_login(&server, &new.username, &new.password, &state.tls)
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

/// `account.set_auto_copy`: the copy every new draft of an account gets.
pub(crate) async fn set_auto_copy(state: &State, params: AutoCopyParams) -> Result<Value, RpcError> {
    let id = params.account_id;
    let copy = params.auto_copy.map(|copy| regards_ipc::protocol::AutoCopy {
        address: copy.address.trim().to_owned(),
        ..copy
    });
    if let Some(copy) = &copy {
        let valid = copy.address.contains('@')
            && copy.address.len() <= 320
            && !copy
                .address
                .chars()
                .any(|c| c.is_whitespace() || matches!(c, '<' | '>' | ','));
        if !valid {
            return Err(invalid("enter one email address to copy"));
        }
    }
    let stored = copy.clone();
    let found = state
        .store
        .write(move |tx| accounts::set_auto_copy(tx, &id, stored.as_ref()))
        .await
        .map_err(store_error)?
        .value;
    if !found {
        return Err(RpcError::new(codes::NOT_FOUND, "no such account"));
    }
    Ok(json!({ "auto_copy": copy }))
}

/// `account.update`: rename an account.
pub(crate) async fn update(state: &State, params: AccountUpdateParams) -> Result<Value, RpcError> {
    let id = params.id;
    let name = params.display_name.trim().to_owned();
    if name.chars().count() > 200 {
        return Err(invalid("the name is too long"));
    }
    let renamed = state
        .store
        .write(move |tx| {
            let name = if name.is_empty() {
                match accounts::get(tx, &id)? {
                    Some(account) => account.email,
                    None => return Ok(None),
                }
            } else {
                name
            };
            accounts::rename(tx, &id, &name)
        })
        .await
        .map_err(store_error)?
        .value
        .ok_or_else(|| RpcError::new(codes::NOT_FOUND, "no such account"))?;
    summary_of(state, renamed).await
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
    state.oauth.forget(&id);
    let _ = state.watchers.stop(&id);
    state.backfills.stop(&id);
    tracing::info!(account = %id, "account removed");
    Ok(json!({ "removed": true }))
}

/// How long `account.resync` waits for a sync already running.
const RESYNC_WAIT: Duration = Duration::from_secs(30);

/// `account.resync`: the account's mail on this device dropped and fetched
/// again from the server (Support › Troubleshooting). The account and its
/// settings, drafts, outbox, pins, snoozes and reminders stay.
pub(crate) async fn resync(state: &Arc<State>, params: AccountRef) -> Result<Value, RpcError> {
    let id = params.account_id;
    let known = {
        let id = id.clone();
        state
            .store
            .read(move |conn| accounts::get(conn, &id))
            .await
            .map_err(store_error)?
    };
    if known.is_none() {
        return Err(RpcError::new(codes::NOT_FOUND, "no such account"));
    }
    // No sync of it runs while its mail is dropped.
    let deadline = Instant::now() + RESYNC_WAIT;
    while !state.sync.claim(&id) {
        if Instant::now() >= deadline {
            return Err(RpcError::new(
                codes::CONFLICT,
                "the account is syncing; try again in a moment",
            ));
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    // A push sync still running would write back what is dropped here.
    state.watchers.stop_and_wait(&id, RESYNC_WAIT).await;
    state.backfills.stop(&id);
    let forgotten = {
        let id = id.clone();
        state.store.write(move |tx| accounts::forget_mail(tx, &id)).await
    };
    state.sync.release(&id);
    // Whatever happened, it syncs again (and keeps its push connection).
    sync::spawn_one(Arc::clone(state), id.clone());
    let dropped = forgotten.map_err(store_error)?.value;
    tracing::info!(account = %id, dropped, "account's mail dropped, to be fetched again");
    Ok(json!(AccountResyncResult {
        dropped: u32::try_from(dropped).unwrap_or(u32::MAX),
    }))
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

pub(crate) fn server(settings: ServerSettings, label: &str) -> Result<Server, RpcError> {
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

/// One account as `account.list` gives it.
pub(crate) async fn summary_of(state: &State, account: Account) -> Result<Value, RpcError> {
    let id = account.id.clone();
    let (access, window, copy) = state
        .store
        .read(move |conn| {
            let access = accounts::agent_access_of(conn, &id)?.map(|(_, access)| access);
            Ok((
                access,
                accounts::agent_send_window(conn, &id)?,
                accounts::auto_copy(conn, &id)?,
            ))
        })
        .await
        .map_err(store_error)?;
    Ok(json!(AccountSummary {
        agent_access: access.unwrap_or_default(),
        agent_send_window: window,
        auto_copy: copy,
        needs_sign_in: state.oauth.needs_sign_in(&account.id),
        ..summary(account)
    }))
}

/// Shortest and longest undo window for an agent's sends, in seconds.
const AGENT_SEND_WINDOW: std::ops::RangeInclusive<u32> = 10..=600;

/// Whether an agent's sends wait for approval or go after an undo window
/// (ADR-0012 §3). Only the app gets here.
pub(crate) async fn set_agent_send(state: &State, params: AgentSendParams) -> Result<Value, RpcError> {
    let (id, window) = (params.account_id, params.window_seconds);
    if window.is_some_and(|seconds| !AGENT_SEND_WINDOW.contains(&seconds)) {
        return Err(invalid(&format!(
            "the undo window is {} to {} seconds",
            AGENT_SEND_WINDOW.start(),
            AGENT_SEND_WINDOW.end()
        )));
    }
    let found = state
        .store
        .write(move |tx| accounts::set_agent_send_window(tx, &id, window))
        .await
        .map_err(store_error)?
        .value;
    if !found {
        return Err(RpcError::new(codes::NOT_FOUND, "no such account"));
    }
    tracing::info!(window = ?window, "agent sending changed");
    Ok(json!({ "agent_send_window": window }))
}

pub(crate) fn summary(account: Account) -> AccountSummary {
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
        agent_access: AgentAccess::Off,
        agent_send_window: None,
        auto_copy: None,
        sign_in: match account.auth_kind {
            AuthKind::Password => SignIn::Password,
            AuthKind::OauthMicrosoft => SignIn::Microsoft,
            AuthKind::OauthGoogle => SignIn::Google,
        },
        needs_sign_in: false,
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

pub(crate) fn check_failed(error: ImapError) -> RpcError {
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

pub(crate) fn vault_error(error: VaultError) -> RpcError {
    RpcError::new(codes::CREDENTIAL_STORE, error.to_string())
}

pub(crate) fn store_error(error: StoreError) -> RpcError {
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
    let now = crate::compose::now_ms();
    let (stored, unread) = state
        .store
        .read(move |conn| {
            let stored = mailboxes::list(conn, &id)?;
            let inbox = stored
                .iter()
                .find(|m| m.role == Some(regards_core::mailbox::Role::Inbox))
                .map(|m| m.id);
            let unread = regards_store::messages::unread_counts(conn, &id, inbox, now)?;
            Ok((stored, unread))
        })
        .await
        .map_err(store_error)?;
    Ok(json!(MailboxListResult {
        mailboxes: stored
            .into_iter()
            .map(|m| MailboxSummary {
                unread: unread.get(&m.id).copied().unwrap_or(0),
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

pub(crate) async fn sync_now(state: &Arc<State>, params: AccountRef) -> Result<Value, RpcError> {
    ensure_exists(state, &params.account_id).await?;
    let outcome = sync::sync_account(state, &params.account_id).await;
    if let Some(Ok(_)) = &outcome {
        // After a folder was rebuilt, only its newest messages are back.
        state.backfills.start(state, &params.account_id);
    }
    match outcome {
        None => Ok(json!(SyncResult {
            already_running: true,
            mailboxes_added: 0,
            mailboxes_updated: 0,
            mailboxes_removed: 0,
            messages_added: 0,
            flag_changes: 0,
            messages_removed: 0,
            unreadable: 0,
        })),
        Some(Ok(summary)) => Ok(json!(SyncResult {
            already_running: false,
            mailboxes_added: summary.mailboxes.added,
            mailboxes_updated: summary.mailboxes.updated,
            mailboxes_removed: summary.mailboxes.removed,
            messages_added: summary.messages_added,
            flag_changes: summary.flag_changes,
            messages_removed: summary.messages_removed,
            unreadable: summary.unreadable,
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
