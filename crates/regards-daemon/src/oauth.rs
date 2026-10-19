//! Signing in with Microsoft or Google (ADR-0018): the browser flow that adds
//! an account, and the access tokens its IMAP and SMTP connections use.
//!
//! - **Adding:** `account.oauth_start` listens on a loopback port and gives
//!   the provider's page to open in the browser. `account.oauth_finish` waits
//!   for the browser to come back, trades the code for tokens (with PKCE,
//!   so nothing else on this computer could), checks IMAP with the access
//!   token, and keeps the refresh token in the OS credential store. The same
//!   flow renews the sign-in of an account that is already there.
//! - **Using:** an access token lasts about an hour. [`access_token`] keeps
//!   it in memory only, and gets a new one with the refresh token when it
//!   runs out or a server turns it down.

use crate::accounts::{self as account_methods, store_error};
use crate::{State, sync};
use regards_core::oauth::{self, Client, OauthError, Pkce, Provider, Tokens};
use regards_core::secret::Secret;
use regards_imap::{Auth, ImapServer, TlsOptions};
use regards_ipc::protocol::{
    ConnectionSecurity, OauthFailure, OauthFlowRef, OauthProvidersResult, OauthStartParams, OauthStartResult, RpcError,
    ServerSettings, codes,
};
use regards_net::NetError;
use regards_net::https::post_form;
use regards_net::loopback::Redirect;
use regards_store::accounts::{self, Account, AuthKind, NewAccount};
use regards_vault::{SecretKey, SecretKind};
use serde_json::{Value, json};
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};
use tokio::task::{AbortHandle, JoinHandle};

/// How long the browser has to come back.
const BROWSER_WAIT: Duration = Duration::from_secs(10 * 60);
/// Sign-ins waiting for the browser at once; starting another drops the oldest.
const MAX_FLOWS: usize = 4;
/// An access token is renewed this long before it runs out.
const EARLY: Duration = Duration::from_secs(2 * 60);

/// A provider's sign-in as the daemon uses it: the app registered with the
/// provider, and the mail servers its accounts use.
#[derive(Debug, Clone)]
pub struct OauthApp {
    pub client: Client,
    /// TLS from the first byte.
    pub imap: ServerSettings,
    pub smtp: ServerSettings,
}

impl OauthApp {
    /// The provider's own servers, with the client id from the environment
    /// (`REGARDS_MICROSOFT_CLIENT_ID`, `REGARDS_GOOGLE_CLIENT_ID` and
    /// `REGARDS_GOOGLE_CLIENT_SECRET`) or, failing that, the one this build
    /// was made with. None: no app is registered with the provider.
    pub fn for_provider(provider: Provider) -> Option<Self> {
        let (id, secret) = match provider {
            Provider::Microsoft => (
                setting(
                    "REGARDS_MICROSOFT_CLIENT_ID",
                    option_env!("REGARDS_MICROSOFT_CLIENT_ID"),
                ),
                None,
            ),
            Provider::Google => (
                setting("REGARDS_GOOGLE_CLIENT_ID", option_env!("REGARDS_GOOGLE_CLIENT_ID")),
                setting(
                    "REGARDS_GOOGLE_CLIENT_SECRET",
                    option_env!("REGARDS_GOOGLE_CLIENT_SECRET"),
                ),
            ),
        };
        let ((imap_host, imap_port), (smtp_host, smtp_port)) = provider.servers();
        Some(Self {
            client: Client::new(provider, id?, secret),
            imap: ServerSettings {
                host: imap_host.to_owned(),
                port: imap_port,
                security: ConnectionSecurity::Tls,
            },
            smtp: ServerSettings {
                host: smtp_host.to_owned(),
                port: smtp_port,
                security: ConnectionSecurity::Starttls,
            },
        })
    }

    /// Every provider this build can sign in with.
    pub fn configured() -> Vec<Self> {
        [Provider::Microsoft, Provider::Google]
            .into_iter()
            .filter_map(Self::for_provider)
            .collect()
    }
}

fn setting(name: &str, built_in: Option<&'static str>) -> Option<String> {
    std::env::var(name)
        .ok()
        .or_else(|| built_in.map(str::to_owned))
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
}

/// The daemon's sign-in state: the apps, the flows waiting for the browser,
/// and the access tokens (in memory only).
pub(crate) struct Oauth {
    apps: HashMap<Provider, OauthApp>,
    flows: Mutex<HashMap<String, Flow>>,
    tokens: Mutex<HashMap<String, Arc<tokio::sync::Mutex<Option<Access>>>>>,
    /// Accounts whose sign-in expired or was withdrawn.
    expired: Mutex<HashSet<String>>,
}

struct Flow {
    app: OauthApp,
    pkce: Pkce,
    redirect_uri: String,
    started: Instant,
    /// Taken by the one `account.oauth_finish` that waits for it.
    browser: Option<Browser>,
    abort: AbortHandle,
}

/// Waits for the browser to come back, with the redirect's query.
type Browser = JoinHandle<Result<Vec<(String, String)>, NetError>>;

struct Access {
    token: Secret,
    renew_at: Instant,
}

impl Access {
    fn of(tokens: &Tokens) -> Self {
        Self {
            token: tokens.access.clone(),
            renew_at: Instant::now() + Duration::from_secs(tokens.expires_in).saturating_sub(EARLY),
        }
    }
}

impl Oauth {
    pub(crate) fn new(apps: Vec<OauthApp>) -> Self {
        Self {
            apps: apps.into_iter().map(|app| (app.client.provider, app)).collect(),
            flows: Mutex::default(),
            tokens: Mutex::default(),
            expired: Mutex::default(),
        }
    }

    fn slot(&self, account_id: &str) -> Arc<tokio::sync::Mutex<Option<Access>>> {
        Arc::clone(lock(&self.tokens).entry(account_id.to_owned()).or_default())
    }

    /// Whether the account must be signed in again.
    pub(crate) fn needs_sign_in(&self, account_id: &str) -> bool {
        lock(&self.expired).contains(account_id)
    }

    /// Drop what is kept for a removed account.
    pub(crate) fn forget(&self, account_id: &str) {
        lock(&self.tokens).remove(account_id);
        lock(&self.expired).remove(account_id);
    }

    /// Keep the access token a sign-in gave.
    async fn keep(&self, account_id: &str, tokens: &Tokens) {
        *self.slot(account_id).lock().await = Some(Access::of(tokens));
        lock(&self.expired).remove(account_id);
    }

    /// Mark an account as signed out; true if it was not yet.
    fn expire(&self, account_id: &str) -> bool {
        lock(&self.expired).insert(account_id.to_owned())
    }
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// The provider an account signs in with, if it uses OAuth.
pub(crate) fn provider_of(kind: AuthKind) -> Option<Provider> {
    match kind {
        AuthKind::Password => None,
        AuthKind::OauthMicrosoft => Some(Provider::Microsoft),
        AuthKind::OauthGoogle => Some(Provider::Google),
    }
}

fn kind_of(provider: Provider) -> AuthKind {
    match provider {
        Provider::Microsoft => AuthKind::OauthMicrosoft,
        Provider::Google => AuthKind::OauthGoogle,
    }
}

fn failed(reason: OauthFailure, message: impl Into<String>) -> RpcError {
    let mut error = RpcError::new(codes::OAUTH_FAILED, message);
    error.data = Some(json!({ "reason": reason }));
    error
}

fn not_available(provider: Provider) -> RpcError {
    failed(
        OauthFailure::NotAvailable,
        format!(
            "signing in with {} is not available in this version of Regards",
            provider.label()
        ),
    )
}

/// `account.oauth_providers`.
pub(crate) fn providers(state: &State) -> Value {
    let providers = [Provider::Microsoft, Provider::Google]
        .into_iter()
        .filter(|provider| state.oauth.apps.contains_key(provider))
        .collect();
    json!(OauthProvidersResult { providers })
}

/// `account.oauth_start`.
pub(crate) async fn start(state: &State, params: OauthStartParams) -> Result<Value, RpcError> {
    let provider = params.provider;
    let app = state
        .oauth
        .apps
        .get(&provider)
        .cloned()
        .ok_or_else(|| not_available(provider))?;
    let redirect = Redirect::listen().await.map_err(crate::session::internal)?;
    let redirect_uri = provider.redirect_uri(redirect.port());
    let pkce = Pkce::new();
    let csrf = oauth::random_token();
    let hint = params
        .email
        .as_deref()
        .map(str::trim)
        .filter(|address| address.contains('@'));
    let url = oauth::authorize_url(&app.client, &redirect_uri, &csrf, &pkce, hint);
    let browser = tokio::spawn(async move { redirect.wait(&csrf, BROWSER_WAIT).await });
    let flow_id = oauth::random_token();
    {
        let mut flows = lock(&state.oauth.flows);
        // Those nobody finished are gone with their listener.
        flows.retain(|_, flow| flow.started.elapsed() < BROWSER_WAIT + Duration::from_secs(60));
        while flows.len() >= MAX_FLOWS {
            let oldest = flows
                .iter()
                .min_by_key(|(_, flow)| flow.started)
                .map(|(id, _)| id.clone())
                .expect("not empty");
            if let Some(flow) = flows.remove(&oldest) {
                flow.abort.abort();
            }
        }
        flows.insert(
            flow_id.clone(),
            Flow {
                app,
                pkce,
                redirect_uri,
                started: Instant::now(),
                abort: browser.abort_handle(),
                browser: Some(browser),
            },
        );
    }
    tracing::info!(provider = provider.label(), "sign-in started");
    Ok(json!(OauthStartResult { flow_id, url }))
}

/// `account.oauth_cancel`.
pub(crate) fn cancel(state: &State, params: OauthFlowRef) -> Value {
    let flow = lock(&state.oauth.flows).remove(&params.flow_id);
    if let Some(flow) = &flow {
        flow.abort.abort();
    }
    json!({ "cancelled": flow.is_some() })
}

/// `account.oauth_finish`.
pub(crate) async fn finish(state: &Arc<State>, params: OauthFlowRef) -> Result<Value, RpcError> {
    let (app, pkce, redirect_uri, browser) = {
        let mut flows = lock(&state.oauth.flows);
        let flow = flows
            .get_mut(&params.flow_id)
            .ok_or_else(|| RpcError::new(codes::NOT_FOUND, "no such sign-in; start it again"))?;
        let browser = flow
            .browser
            .take()
            .ok_or_else(|| RpcError::new(codes::CONFLICT, "this sign-in is already being finished"))?;
        (flow.app.clone(), flow.pkce.clone(), flow.redirect_uri.clone(), browser)
    };
    let came_back = browser.await;
    lock(&state.oauth.flows).remove(&params.flow_id);
    let provider = app.client.provider;
    let label = provider.label();
    let query = match came_back {
        Ok(Ok(query)) => query,
        Ok(Err(_)) => {
            return Err(failed(
                OauthFailure::TimedOut,
                "nobody came back from the browser in time; start the sign-in again",
            ));
        }
        Err(_) => return Err(failed(OauthFailure::Cancelled, "the sign-in was given up")),
    };
    let param = |name: &str| {
        query
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.as_str())
    };
    if let Some(error) = param("error") {
        return Err(match error {
            "access_denied" => failed(OauthFailure::Cancelled, "the sign-in was cancelled in the browser"),
            _ => failed(
                OauthFailure::Refused,
                format!(
                    "{label} refused the sign-in: {}",
                    first_line(param("error_description").unwrap_or(error))
                ),
            ),
        });
    }
    let code = param("code")
        .filter(|code| !code.is_empty())
        .ok_or_else(|| failed(OauthFailure::Refused, "the browser came back without a sign-in code"))?;

    let form = oauth::code_form(&app.client, code, &pkce, &redirect_uri);
    let tokens = request(&app.client, &form, &state.tls)
        .await
        .map_err(|error| match error {
            Request::Offline(reason) => failed(
                OauthFailure::Offline,
                format!("could not reach {label} to finish the sign-in: {reason}"),
            ),
            Request::Refused(error) => failed(OauthFailure::Refused, format!("{label}: {error}")),
        })?;
    let email = tokens.email.clone().ok_or_else(|| {
        failed(
            OauthFailure::NoAddress,
            format!("{label} did not say which address signed in"),
        )
    })?;
    let refresh = tokens.refresh.clone().ok_or_else(|| {
        failed(
            OauthFailure::Refused,
            format!("{label} gave no lasting sign-in (no refresh token)"),
        )
    })?;

    let existing = state
        .store
        .read(accounts::list)
        .await
        .map_err(store_error)?
        .into_iter()
        .find(|account| account.email.eq_ignore_ascii_case(&email));
    if let Some(account) = &existing
        && account.auth_kind != kind_of(provider)
    {
        return Err(RpcError::new(
            codes::CONFLICT,
            format!(
                "an account for {email} already exists and signs in another way; remove it first to sign in with {label}"
            ),
        ));
    }

    let server = ImapServer {
        host: app.imap.host.clone(),
        port: app.imap.port,
    };
    let check = regards_imap::verify_sign_in(&server, &email, Auth::Bearer(&tokens.access), &state.tls)
        .await
        .map_err(account_methods::check_failed)?;
    let vault = account_methods::vault(state).await?;

    if let Some(account) = existing {
        // Signing in again: the account stays, with its new sign-in.
        let key = SecretKey::from_stored(account.secret_key.clone());
        vault.set(&key, &refresh).await.map_err(account_methods::vault_error)?;
        state.oauth.keep(&account.id, &tokens).await;
        tracing::info!(account = %account.id, provider = label, "signed in again");
        // What waited for the sign-in goes now; the write tells clients the account changed.
        let id = account.id.clone();
        state
            .store
            .write(move |tx| regards_store::outbox::retry_now(tx, &id, crate::compose::now_ms()))
            .await
            .map_err(store_error)?;
        state.outbox.wake();
        sync::spawn_one(Arc::clone(state), account.id.clone());
        return account_methods::summary_of(state, account).await;
    }

    let id = accounts::new_id();
    let key = SecretKey::new(&id, SecretKind::OauthRefresh);
    vault.set(&key, &refresh).await.map_err(account_methods::vault_error)?;
    let record = NewAccount {
        id: id.clone(),
        display_name: tokens.name.clone().unwrap_or_else(|| email.clone()),
        email: email.clone(),
        auth_kind: kind_of(provider),
        username: email,
        secret_key: key.as_str().to_owned(),
        imap: account_methods::server(app.imap, "IMAP")?,
        smtp: account_methods::server(app.smtp, "SMTP")?,
        imap_capabilities: check.capabilities,
    };
    match state.store.write(move |tx| accounts::insert(tx, record)).await {
        Ok(committed) => {
            state.oauth.keep(&id, &tokens).await;
            tracing::info!(account = %id, provider = label, "account added");
            sync::spawn_one(Arc::clone(state), id);
            Ok(json!(account_methods::summary(committed.value)))
        }
        Err(error) => {
            if let Err(cleanup) = vault.delete(&key).await {
                tracing::warn!(error = %cleanup, "could not remove the sign-in of an account that was not added");
            }
            Err(store_error(error))
        }
    }
}

fn first_line(text: &str) -> String {
    text.lines().next().unwrap_or("").chars().take(300).collect()
}

enum Request {
    Offline(String),
    Refused(OauthError),
}

async fn request(client: &Client, form: &[(&'static str, String)], tls: &TlsOptions) -> Result<Tokens, Request> {
    let form: Vec<(&str, &str)> = form.iter().map(|(name, value)| (*name, value.as_str())).collect();
    let answer = post_form(&client.token_endpoint, &form, tls)
        .await
        .map_err(|error| Request::Offline(error.to_string()))?;
    oauth::read_tokens(answer.status, &answer.body).map_err(Request::Refused)
}

/// Why no access token could be had.
#[derive(Debug, thiserror::Error)]
pub(crate) enum TokenError {
    #[error("the sign-in with {0} has expired or was withdrawn; sign in again")]
    SignInAgain(&'static str),
    #[error("signing in with {0} is not available in this version of Regards")]
    NotAvailable(&'static str),
    #[error("the saved sign-in is missing; sign in again")]
    Missing,
    #[error("the system credential store is not available: {0}")]
    Vault(String),
    #[error("could not reach {0} to renew the sign-in: {1}")]
    Offline(&'static str, String),
    #[error("{0} did not renew the sign-in: {1}")]
    Refused(&'static str, String),
}

impl TokenError {
    /// Trying again later may work: by itself, or once the person signs in
    /// again (a message then waits in the outbox, as when offline).
    pub(crate) fn passes(&self) -> bool {
        matches!(self, Self::Offline(..) | Self::Vault(_) | Self::SignInAgain(_))
    }
}

/// An access token for an OAuth account, and whether it came from memory
/// (so a server turning it down is worth one renewal). `refused`: the token
/// a server just turned down; a newer one is used, or one is fetched.
pub(crate) async fn access_token(
    state: &State,
    account: &Account,
    refused: Option<&Secret>,
) -> Result<(Secret, bool), TokenError> {
    let provider = provider_of(account.auth_kind).expect("an account that signs in with OAuth");
    let label = provider.label();
    // Its refresh token was turned down: only signing in again helps.
    if state.oauth.needs_sign_in(&account.id) {
        return Err(TokenError::SignInAgain(label));
    }
    let slot = state.oauth.slot(&account.id);
    let mut kept = slot.lock().await;
    if let Some(access) = kept.as_ref() {
        match refused {
            None if Instant::now() < access.renew_at => return Ok((access.token.clone(), true)),
            // Renewed by another connection meanwhile.
            Some(refused) if refused.expose() != access.token.expose() => return Ok((access.token.clone(), false)),
            _ => {}
        }
    }
    let app = state.oauth.apps.get(&provider).ok_or(TokenError::NotAvailable(label))?;
    let vault = account_methods::vault(state)
        .await
        .map_err(|error| TokenError::Vault(error.message))?;
    let key = SecretKey::from_stored(account.secret_key.clone());
    let refresh = vault
        .get(&key)
        .await
        .map_err(|error| TokenError::Vault(error.to_string()))?
        .ok_or(TokenError::Missing)?;
    let tokens = match request(&app.client, &oauth::refresh_form(&app.client, &refresh), &state.tls).await {
        Ok(tokens) => tokens,
        Err(Request::Offline(reason)) => return Err(TokenError::Offline(label, reason)),
        Err(Request::Refused(OauthError::SignInAgain)) => {
            *kept = None;
            if state.oauth.expire(&account.id) {
                // Not a database change: clients learn of it from the feed.
                state.feed.bump();
            }
            tracing::warn!(account = %account.id, provider = label, "the sign-in expired");
            return Err(TokenError::SignInAgain(label));
        }
        Err(Request::Refused(error)) => return Err(TokenError::Refused(label, error.to_string())),
    };
    // Some providers hand out a new refresh token each time; the old one may stop working.
    if let Some(renewed) = &tokens.refresh
        && renewed.expose() != refresh.expose()
        && let Err(error) = vault.set(&key, renewed).await
    {
        tracing::warn!(account = %account.id, %error, "could not keep the renewed sign-in");
    }
    *kept = Some(Access::of(&tokens));
    lock(&state.oauth.expired).remove(&account.id);
    Ok((tokens.access, false))
}
