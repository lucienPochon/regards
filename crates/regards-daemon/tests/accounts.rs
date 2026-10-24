//! Account methods end to end: pipe, principals, IMAP sign-in check, store and vault.

#[path = "../../regards-imap/tests/support/fake_imap.rs"]
mod fake_imap;
#[path = "../../regards-smtp/tests/support/fake_smtp.rs"]
mod fake_smtp;

use fake_imap::{PASSWORD, fake_server, pki};
use regards_core::mailbox::Role;
use regards_core::paths::DataDirs;
use regards_core::secret::Secret;
use regards_daemon::Daemon;
use regards_imap::TlsOptions;
use regards_ipc::protocol::{
    AccountAddParams, AccountCheckFailure, AccountLoginParams, AccountRef, AccountRemoveParams, AccountSummary,
    CheckedServer, ClientKind, ConnectionSecurity, DiscoverParams, DiscoverResult, DiscoverSource, MailboxListResult,
    Provider, ServerCheckParams, ServerCheckResult, ServerSettings, SyncResult, codes, methods,
};
use regards_ipc::{Client, ClientError, Endpoint, UiToken};
use regards_vault::{SecretKey, SecretKind, Vault};
use serde_json::{Value, json};
use tokio::task::JoinHandle;

struct Running {
    _temp: tempfile::TempDir,
    endpoint: Endpoint,
    token: UiToken,
    vault: Vault,
    handle: JoinHandle<()>,
}

impl Drop for Running {
    fn drop(&mut self) {
        self.handle.abort();
    }
}

async fn start(tls: TlsOptions) -> Running {
    let temp = tempfile::tempdir().unwrap();
    let token = UiToken::generate().unwrap();
    let vault = Vault::in_memory();
    let daemon = Daemon::bind(DataDirs::at(temp.path()))
        .await
        .unwrap()
        .with_ui_token(token.clone())
        .with_vault(vault.clone())
        .with_tls(tls);
    let endpoint = daemon.endpoint().clone();
    let handle = tokio::spawn(daemon.serve(std::future::pending()));
    Running {
        _temp: temp,
        endpoint,
        token,
        vault,
        handle,
    }
}

impl Running {
    async fn desktop(&self) -> Client {
        Client::connect_with(&self.endpoint, ClientKind::Desktop, Some(&self.token))
            .await
            .unwrap()
    }
}

fn params(email: &str, port: u16, password: &str) -> AccountAddParams {
    AccountAddParams {
        display_name: "Ada Lovelace".into(),
        email: email.into(),
        username: None,
        password: Secret::new(password),
        imap: ServerSettings {
            host: "localhost".into(),
            port,
            security: ConnectionSecurity::Tls,
        },
        smtp: ServerSettings {
            host: "localhost".into(),
            port: 587,
            security: ConnectionSecurity::Starttls,
        },
        // Nothing answers on 587 here; `sending_is_checked_too` has a server.
        skip_send_check: true,
    }
}

fn rpc_error(result: Result<Value, ClientError>) -> (i32, Option<Value>) {
    match result {
        Err(ClientError::Rpc(error)) => (error.code, error.data),
        other => panic!("expected an RPC error, got {other:?}"),
    }
}

#[tokio::test]
async fn agents_cannot_add_or_remove_accounts() {
    let running = start(TlsOptions::platform()).await;
    let mut agent = Client::connect(&running.endpoint, ClientKind::Cli).await.unwrap();
    // They list only the accounts open to them (ADR-0012; tests/agents.rs).
    let listed: Vec<AccountSummary> = agent.call(methods::ACCOUNT_LIST, Value::Null).await.unwrap();
    assert!(listed.is_empty());
    let add = serde_json::to_value(params("ada@example.test", 1, PASSWORD)).unwrap();
    for (method, body) in [
        (methods::ACCOUNT_ADD, add),
        (methods::ACCOUNT_REMOVE, json!({ "id": "x" })),
        (
            methods::ACCOUNT_SET_LOGIN,
            json!({ "id": "x", "imap": { "host": "h", "port": 993, "security": "tls" },
                    "smtp": { "host": "h", "port": 465, "security": "tls" } }),
        ),
    ] {
        let (code, _) = rpc_error(agent.call::<_, Value>(method, body).await);
        assert_eq!(code, codes::FORBIDDEN, "{method} must be refused for agents");
    }
}

#[tokio::test]
async fn adding_verifies_the_sign_in_then_stores_the_account_and_password() {
    let pki = pki(&["localhost"]);
    let (port, mut logins) = fake_server(&pki, "* OK ready\r\n", PASSWORD).await;
    let running = start(
        TlsOptions::platform()
            .with_extra_roots_pem(pki.ca_pem.as_bytes())
            .unwrap(),
    )
    .await;
    let mut desktop = running.desktop().await;

    let added: AccountSummary = desktop
        .call(methods::ACCOUNT_ADD, params("  Ada@Example.test ", port, PASSWORD))
        .await
        .unwrap();
    assert_eq!(logins.next().await.as_deref(), Some(PASSWORD));
    assert_eq!(added.email, "Ada@Example.test");
    assert_eq!(
        added.username, "Ada@Example.test",
        "the user name defaults to the address"
    );
    assert_eq!(
        added.imap_capabilities,
        ["IMAP4REV1", "IDLE", "MOVE", "UIDPLUS", "SPECIAL-USE"]
    );

    let listed: Vec<AccountSummary> = desktop.call(methods::ACCOUNT_LIST, Value::Null).await.unwrap();
    assert_eq!(listed, vec![added.clone()]);

    let key = SecretKey::new(&added.id, SecretKind::MailPassword);
    assert_eq!(running.vault.get(&key).await.unwrap().unwrap().expose(), PASSWORD);

    // Same address in another case: refused before contacting any server.
    let (code, _) = rpc_error(
        desktop
            .call::<_, Value>(methods::ACCOUNT_ADD, params("ada@example.TEST", 1, PASSWORD))
            .await,
    );
    assert_eq!(code, codes::CONFLICT);

    let removed: Value = desktop
        .call(methods::ACCOUNT_REMOVE, AccountRemoveParams { id: added.id.clone() })
        .await
        .unwrap();
    assert_eq!(removed, json!({ "removed": true }));
    assert!(
        running.vault.get(&key).await.unwrap().is_none(),
        "the password is deleted too"
    );
    let listed: Vec<AccountSummary> = desktop.call(methods::ACCOUNT_LIST, Value::Null).await.unwrap();
    assert!(listed.is_empty());

    let (code, _) = rpc_error(
        desktop
            .call::<_, Value>(methods::ACCOUNT_REMOVE, AccountRemoveParams { id: added.id })
            .await,
    );
    assert_eq!(code, codes::NOT_FOUND);
}

#[tokio::test]
async fn a_failed_sign_in_stores_nothing_and_says_why() {
    let pki = pki(&["localhost"]);
    let (port, _) = fake_server(&pki, "* OK ready\r\n", PASSWORD).await;
    let running = start(
        TlsOptions::platform()
            .with_extra_roots_pem(pki.ca_pem.as_bytes())
            .unwrap(),
    )
    .await;
    let mut desktop = running.desktop().await;

    let (code, data) = rpc_error(
        desktop
            .call::<_, Value>(methods::ACCOUNT_ADD, params("ada@example.test", port, "wrong"))
            .await,
    );
    assert_eq!(code, codes::ACCOUNT_CHECK_FAILED);
    assert_eq!(
        data,
        Some(json!({ "reason": "auth_failed", "server": "imap", "tried": ["ada@example.test", "ada"] })),
        "the address, then the part before its @"
    );

    // An untrusted certificate is a different, named failure.
    let (port, _) = fake_server(&pki, "* OK ready\r\n", PASSWORD).await;
    let running_untrusting = start(TlsOptions::platform()).await;
    let mut other = running_untrusting.desktop().await;
    let (code, data) = rpc_error(
        other
            .call::<_, Value>(methods::ACCOUNT_ADD, params("ada@example.test", port, PASSWORD))
            .await,
    );
    assert_eq!(code, codes::ACCOUNT_CHECK_FAILED);
    assert_eq!(
        data,
        Some(json!({ "reason": "tls", "server": "imap", "tried": ["ada@example.test"] }))
    );

    let listed: Vec<AccountSummary> = desktop.call(methods::ACCOUNT_LIST, Value::Null).await.unwrap();
    assert!(listed.is_empty());
}

#[tokio::test]
async fn a_host_that_wants_the_name_before_the_at_gets_it() {
    let pki = pki(&["localhost"]);
    let (port, _) = fake_imap::serve(
        &pki,
        fake_imap::Script {
            username: Some("ada"),
            ..fake_imap::Script::new(PASSWORD)
        },
    )
    .await;
    let running = start(
        TlsOptions::platform()
            .with_extra_roots_pem(pki.ca_pem.as_bytes())
            .unwrap(),
    )
    .await;
    let mut desktop = running.desktop().await;

    let added: AccountSummary = desktop
        .call(methods::ACCOUNT_ADD, params("ada@example.test", port, PASSWORD))
        .await
        .unwrap();
    assert_eq!(added.username, "ada", "kept: the account syncs with it");

    // A user name the person typed is the only one tried.
    let typed = AccountAddParams {
        username: Some("ada@example.test".into()),
        ..params("other@example.test", port, PASSWORD)
    };
    let (code, data) = rpc_error(desktop.call::<_, Value>(methods::ACCOUNT_ADD, typed).await);
    assert_eq!(code, codes::ACCOUNT_CHECK_FAILED);
    assert_eq!(
        data,
        Some(json!({ "reason": "auth_failed", "server": "imap", "tried": ["ada@example.test"] }))
    );
}

#[tokio::test]
async fn sending_is_checked_too() {
    let imap_pki = pki(&["localhost"]);
    let (imap_port, _) = fake_server(&imap_pki, "* OK ready\r\n", PASSWORD).await;
    let smtp_pki = fake_smtp::pki();
    let smtp_script = |password| fake_smtp::Script {
        password,
        ..fake_smtp::Script::new(fake_smtp::Mode::StartTls {
            offer: true,
            inject: false,
        })
    };
    let (smtp_port, mut sessions) = fake_smtp::serve(&smtp_pki, smtp_script(PASSWORD)).await;
    let (refusing_port, _) = fake_smtp::serve(&smtp_pki, smtp_script("another password")).await;
    let running = start(
        TlsOptions::platform()
            .with_extra_roots_pem(imap_pki.ca_pem.as_bytes())
            .unwrap()
            .with_extra_roots_pem(smtp_pki.ca_pem.as_bytes())
            .unwrap(),
    )
    .await;
    let mut desktop = running.desktop().await;
    let sending_to = |email: &str, port| AccountAddParams {
        smtp: ServerSettings {
            host: "127.0.0.1".into(),
            port,
            security: ConnectionSecurity::Starttls,
        },
        skip_send_check: false,
        ..params(email, imap_port, PASSWORD)
    };

    // Receiving works, sending refuses the password: nothing is kept.
    let (code, data) = rpc_error(
        desktop
            .call::<_, Value>(methods::ACCOUNT_ADD, sending_to("ada@example.test", refusing_port))
            .await,
    );
    assert_eq!(code, codes::ACCOUNT_CHECK_FAILED);
    assert_eq!(
        data,
        Some(json!({ "reason": "auth_failed", "server": "smtp", "tried": ["ada@example.test"] }))
    );
    let listed: Vec<AccountSummary> = desktop.call(methods::ACCOUNT_LIST, Value::Null).await.unwrap();
    assert!(listed.is_empty());

    // Nothing answers: named apart from a refused password.
    let closed = std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let (_, data) = rpc_error(
        desktop
            .call::<_, Value>(methods::ACCOUNT_ADD, sending_to("ada@example.test", closed))
            .await,
    );
    assert_eq!(data.unwrap()["reason"], "connect");

    // Both work: added, after a sign-in that sent nothing.
    let added: AccountSummary = desktop
        .call(methods::ACCOUNT_ADD, sending_to("ada@example.test", smtp_port))
        .await
        .unwrap();
    assert_eq!(added.smtp.port, smtp_port);
    let session = sessions.next().await;
    assert!(session.login.is_some() && session.data.is_none());
}

fn login(id: &str, port: u16, password: Option<&str>) -> AccountLoginParams {
    AccountLoginParams {
        id: id.into(),
        username: None,
        password: password.map(Secret::new),
        imap: ServerSettings {
            host: "localhost".into(),
            port,
            security: ConnectionSecurity::Tls,
        },
        smtp: ServerSettings {
            host: "localhost".into(),
            port: 465,
            security: ConnectionSecurity::Tls,
        },
    }
}

#[tokio::test]
async fn a_changed_password_or_server_is_checked_then_kept() {
    let pki = pki(&["localhost"]);
    let (old_port, _) = fake_server(&pki, "* OK ready\r\n", PASSWORD).await;
    let (new_port, mut new_logins) = fake_server(&pki, "* OK ready\r\n", "rotated").await;
    let running = start(
        TlsOptions::platform()
            .with_extra_roots_pem(pki.ca_pem.as_bytes())
            .unwrap(),
    )
    .await;
    let mut desktop = running.desktop().await;
    let added: AccountSummary = desktop
        .call(methods::ACCOUNT_ADD, params("ada@example.test", old_port, PASSWORD))
        .await
        .unwrap();
    let key = SecretKey::new(&added.id, SecretKind::MailPassword);

    // Refused by the server: nothing changes.
    let (code, data) = rpc_error(
        desktop
            .call::<_, Value>(methods::ACCOUNT_SET_LOGIN, login(&added.id, new_port, Some("wrong")))
            .await,
    );
    assert_eq!(code, codes::ACCOUNT_CHECK_FAILED);
    assert_eq!(
        data,
        Some(json!({ "reason": "auth_failed", "server": "imap", "tried": ["ada@example.test"] }))
    );
    assert_eq!(running.vault.get(&key).await.unwrap().unwrap().expose(), PASSWORD);

    // The new password on the new server: both kept, the account stays.
    let changed: AccountSummary = desktop
        .call(methods::ACCOUNT_SET_LOGIN, login(&added.id, new_port, Some("rotated")))
        .await
        .unwrap();
    assert_eq!(changed.id, added.id);
    assert_eq!((changed.imap.port, changed.smtp.port), (new_port, 465));
    assert_eq!(changed.username, "ada@example.test");
    assert_eq!(running.vault.get(&key).await.unwrap().unwrap().expose(), "rotated");
    assert!(new_logins.next().await.is_some());

    // No password given: the stored one is checked with the servers.
    let again: AccountSummary = desktop
        .call(methods::ACCOUNT_SET_LOGIN, login(&added.id, new_port, None))
        .await
        .unwrap();
    assert_eq!(again.imap.port, new_port);

    let (code, _) = rpc_error(
        desktop
            .call::<_, Value>(methods::ACCOUNT_SET_LOGIN, login("no-such-account", new_port, None))
            .await,
    );
    assert_eq!(code, codes::NOT_FOUND);
    let (code, _) = rpc_error(
        desktop
            .call::<_, Value>(methods::ACCOUNT_SET_LOGIN, login(&added.id, new_port, Some("")))
            .await,
    );
    assert_eq!(code, codes::INVALID_PARAMS);
}

#[tokio::test]
async fn invalid_input_is_rejected_before_any_network_access() {
    let running = start(TlsOptions::platform()).await;
    let mut desktop = running.desktop().await;
    let mut starttls_imap = params("ada@example.test", 143, PASSWORD);
    starttls_imap.imap.security = ConnectionSecurity::Starttls;
    let mut no_host = params("ada@example.test", 993, PASSWORD);
    no_host.imap.host = " ".into();
    for bad in [
        params("not-an-address", 993, PASSWORD),
        params("ada@example.test", 993, ""),
        starttls_imap,
        no_host,
    ] {
        let (code, _) = rpc_error(desktop.call::<_, Value>(methods::ACCOUNT_ADD, bad).await);
        assert_eq!(code, codes::INVALID_PARAMS);
    }
}

/// Poll `mailbox.list` until the first sync finished (or failed).
async fn synced_mailboxes(desktop: &mut Client, account_id: &str) -> MailboxListResult {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
    loop {
        let result: MailboxListResult = desktop
            .call(
                methods::MAILBOX_LIST,
                AccountRef {
                    account_id: account_id.into(),
                },
            )
            .await
            .unwrap();
        let done = !result.sync.running && (result.sync.last_success_at.is_some() || result.sync.last_error.is_some());
        if done {
            return result;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "no sync result in time: {result:?}"
        );
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
}

#[tokio::test]
async fn mailboxes_are_synced_after_adding_with_roles_and_on_request() {
    let pki = pki(&["localhost"]);
    let (port, _) = fake_server(&pki, "* OK ready\r\n", PASSWORD).await;
    let running = start(
        TlsOptions::platform()
            .with_extra_roots_pem(pki.ca_pem.as_bytes())
            .unwrap(),
    )
    .await;
    let mut desktop = running.desktop().await;
    let added: AccountSummary = desktop
        .call(methods::ACCOUNT_ADD, params("ada@example.test", port, PASSWORD))
        .await
        .unwrap();

    let listed = synced_mailboxes(&mut desktop, &added.id).await;
    assert_eq!(listed.sync.last_error, None);
    let summary: Vec<(&str, Option<Role>, bool)> = listed
        .mailboxes
        .iter()
        .map(|m| (m.name.as_str(), m.role, m.selectable))
        .collect();
    assert_eq!(
        summary,
        [
            ("INBOX", Some(Role::Inbox), true),
            ("Drafts", Some(Role::Drafts), true),
            ("Sent", Some(Role::Sent), true),
            ("Spam", Some(Role::Junk), true),
            ("Trash", Some(Role::Trash), true),
            ("Projects", None, false),
            ("Projects/Résumé", None, true),
        ]
    );

    let again: SyncResult = desktop
        .call(
            methods::ACCOUNT_SYNC,
            AccountRef {
                account_id: added.id.clone(),
            },
        )
        .await
        .unwrap();
    assert_eq!(
        (again.mailboxes_added, again.mailboxes_updated, again.mailboxes_removed),
        (0, 0, 0)
    );

    // Without its password, a sync fails with a message the user can act on.
    running
        .vault
        .delete(&SecretKey::new(&added.id, SecretKind::MailPassword))
        .await
        .unwrap();
    let error = desktop
        .call::<_, Value>(
            methods::ACCOUNT_SYNC,
            AccountRef {
                account_id: added.id.clone(),
            },
        )
        .await
        .unwrap_err();
    match error {
        ClientError::Rpc(error) => {
            assert_eq!(error.code, codes::SYNC_FAILED);
            assert!(error.message.contains("password is missing"), "{}", error.message);
        }
        other => panic!("unexpected {other:?}"),
    }

    let (code, _) = rpc_error(
        desktop
            .call::<_, Value>(
                methods::MAILBOX_LIST,
                AccountRef {
                    account_id: "no-such-account".into(),
                },
            )
            .await,
    );
    assert_eq!(code, codes::NOT_FOUND);
}

#[tokio::test]
async fn a_restarted_daemon_syncs_existing_accounts() {
    let pki = pki(&["localhost"]);
    let (port, _) = fake_server(&pki, "* OK ready\r\n", PASSWORD).await;
    let tls = TlsOptions::platform()
        .with_extra_roots_pem(pki.ca_pem.as_bytes())
        .unwrap();
    let temp = tempfile::tempdir().unwrap();
    let vault = Vault::in_memory();
    let token = UiToken::generate().unwrap();
    let start = |tls: TlsOptions| {
        let (temp, vault, token) = (temp.path().to_owned(), vault.clone(), token.clone());
        async move {
            let daemon = Daemon::bind(DataDirs::at(temp))
                .await
                .unwrap()
                .with_ui_token(token)
                .with_vault(vault)
                .with_tls(tls);
            let endpoint = daemon.endpoint().clone();
            (endpoint, tokio::spawn(daemon.serve(std::future::pending())))
        }
    };

    let (endpoint, first) = start(tls.clone()).await;
    let mut desktop = Client::connect_with(&endpoint, ClientKind::Desktop, Some(&token))
        .await
        .unwrap();
    let added: AccountSummary = desktop
        .call(methods::ACCOUNT_ADD, params("ada@example.test", port, PASSWORD))
        .await
        .unwrap();
    synced_mailboxes(&mut desktop, &added.id).await;
    drop(desktop);
    desktop_shutdown(&endpoint, &token).await;
    first.await.unwrap();

    let (endpoint, second) = start(tls).await;
    let mut desktop = Client::connect_with(&endpoint, ClientKind::Desktop, Some(&token))
        .await
        .unwrap();
    let listed = synced_mailboxes(&mut desktop, &added.id).await;
    assert!(
        listed.sync.last_success_at.is_some(),
        "the startup sync ran: {listed:?}"
    );
    assert_eq!(listed.mailboxes.len(), 7);
    second.abort();
}

async fn desktop_shutdown(endpoint: &Endpoint, token: &UiToken) {
    let mut client = Client::connect_with(endpoint, ClientKind::Desktop, Some(token))
        .await
        .unwrap();
    client.shutdown().await.unwrap();
}

#[tokio::test]
async fn subscribers_are_told_about_changes_and_agents_cannot_subscribe() {
    let pki = pki(&["localhost"]);
    let (port, _) = fake_server(&pki, "* OK ready\r\n", PASSWORD).await;
    let running = start(
        TlsOptions::platform()
            .with_extra_roots_pem(pki.ca_pem.as_bytes())
            .unwrap(),
    )
    .await;

    let agent = Client::connect(&running.endpoint, ClientKind::Cli).await.unwrap();
    match agent.subscribe_changes().await {
        Err(ClientError::Rpc(error)) => assert_eq!(error.code, codes::FORBIDDEN),
        Err(other) => panic!("unexpected {other:?}"),
        Ok(_) => panic!("an agent must not get the change feed"),
    }

    let mut feed = running.desktop().await.subscribe_changes().await.unwrap();
    let start = feed.current();
    let mut desktop = running.desktop().await;
    let _: AccountSummary = desktop
        .call(methods::ACCOUNT_ADD, params("ada@example.test", port, PASSWORD))
        .await
        .unwrap();

    // The account row (a database write), then sync status and mailboxes.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    let mut latest = start;
    while latest.revision <= start.revision + 1 {
        let change = tokio::time::timeout_at(deadline.into(), feed.next())
            .await
            .expect("changes are announced")
            .unwrap();
        assert!(change.version > latest.version, "versions only grow");
        latest = change;
    }
    assert!(latest.revision > start.revision);
}

async fn discover(client: &mut Client, email: &str) -> Result<DiscoverResult, ClientError> {
    client
        .call(methods::ACCOUNT_DISCOVER, DiscoverParams { email: email.into() })
        .await
}

#[tokio::test]
async fn an_address_tells_its_servers_before_any_are_typed() {
    let running = start(TlsOptions::platform()).await;
    let mut desktop = running.desktop().await;

    // A known provider: at once, no lookup, with what to say of the password.
    let gmail = discover(&mut desktop, "ada@gmail.com").await.unwrap();
    assert_eq!(gmail.source, Some(DiscoverSource::Known));
    assert_eq!(gmail.provider_name.as_deref(), Some("Gmail"));
    let (imap, smtp) = (gmail.imap.unwrap(), gmail.smtp.unwrap());
    assert_eq!(
        (imap.host.as_str(), imap.port, imap.security),
        ("imap.gmail.com", 993, ConnectionSecurity::Tls)
    );
    assert_eq!(
        (smtp.host.as_str(), smtp.port, smtp.security),
        ("smtp.gmail.com", 587, ConnectionSecurity::Starttls)
    );
    assert_eq!(gmail.oauth, Some(Provider::Google));
    assert!(gmail.password_help.unwrap().contains("app password"));
    assert!(discover(&mut desktop, "ada@hotmail.com").await.unwrap().oauth_only);
    // A provider with no IMAP: said so, and no servers to try.
    let proton = discover(&mut desktop, "ada@proton.me").await.unwrap();
    assert!(proton.no_imap.is_some() && proton.imap.is_none() && proton.smtp.is_none());
    assert_eq!(proton.provider_name.as_deref(), Some("Proton Mail"));

    let refused = discover(&mut desktop, "not an address").await.unwrap_err();
    assert!(matches!(refused, ClientError::Rpc(ref e) if e.code == codes::INVALID_PARAMS));
    // It reaches out to servers: the app's to ask, not an agent's.
    let mut agent = Client::connect(&running.endpoint, ClientKind::Cli).await.unwrap();
    let refused = discover(&mut agent, "ada@gmail.com").await.unwrap_err();
    assert!(matches!(refused, ClientError::Rpc(ref e) if e.code == codes::FORBIDDEN));
}

/// Against the real internet: DNS and public mail servers, no sign-in.
#[tokio::test]
#[ignore = "network: cargo test -p regards-daemon --test accounts -- --ignored real_"]
async fn real_domains_by_their_mx_and_their_usual_names() {
    let running = start(TlsOptions::platform()).await;
    let mut desktop = running.desktop().await;
    // Microsoft 365 behind the organisation's own domain.
    let hosted = discover(&mut desktop, "someone@microsoft.com").await.unwrap();
    assert_eq!(hosted.source, Some(DiscoverSource::Mx), "{hosted:?}");
    assert_eq!(hosted.oauth, Some(Provider::Microsoft));
    // Microsoft's own address in another country, not in the list.
    let regional = discover(&mut desktop, "someone@hotmail.it").await.unwrap();
    assert!(
        regional.source == Some(DiscoverSource::Mx) && regional.oauth_only,
        "{regional:?}"
    );
    // A provider not in the list, found by DNS or its usual names.
    let other = discover(&mut desktop, "someone@hispeed.ch").await.unwrap();
    assert!(
        matches!(other.source, Some(DiscoverSource::Dns | DiscoverSource::Guess)),
        "{other:?}"
    );
    assert!(other.imap.as_ref().unwrap().host.ends_with("hispeed.ch"));
    println!("{hosted:?}\n{regional:?}\n{other:?}");
}

#[tokio::test]
async fn one_server_is_checked_before_any_password() {
    let imap_pki = pki(&["localhost"]);
    let (imap_port, mut logins) = fake_server(&imap_pki, "* OK ready\r\n", PASSWORD).await;
    let smtp_pki = fake_smtp::pki();
    let (smtp_port, _) = fake_smtp::serve(
        &smtp_pki,
        fake_smtp::Script::new(fake_smtp::Mode::StartTls {
            offer: true,
            inject: false,
        }),
    )
    .await;
    let running = start(
        TlsOptions::platform()
            .with_extra_roots_pem(imap_pki.ca_pem.as_bytes())
            .unwrap()
            .with_extra_roots_pem(smtp_pki.ca_pem.as_bytes())
            .unwrap(),
    )
    .await;
    let mut desktop = running.desktop().await;
    let settings = |host: &str, port, security| ServerSettings {
        host: host.into(),
        port,
        security,
    };
    let imap = |port| ServerCheckParams {
        server: CheckedServer::Imap,
        settings: settings("localhost", port, ConnectionSecurity::Tls),
    };
    let smtp = |security| ServerCheckParams {
        server: CheckedServer::Smtp,
        settings: settings("127.0.0.1", smtp_port, security),
    };

    // It answers, and nothing signs in.
    let answered: ServerCheckResult = desktop
        .call(methods::ACCOUNT_CHECK_SERVER, imap(imap_port))
        .await
        .unwrap();
    assert_eq!(answered, ServerCheckResult::default());
    assert_eq!(logins.next().await, None, "no LOGIN");

    // Nothing there.
    let closed = std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let nothing: ServerCheckResult = desktop.call(methods::ACCOUNT_CHECK_SERVER, imap(closed)).await.unwrap();
    assert_eq!(nothing.reason, Some(AccountCheckFailure::Connect));

    // The right sending port with the wrong security: the right one is offered.
    let wrong: ServerCheckResult = desktop
        .call(methods::ACCOUNT_CHECK_SERVER, smtp(ConnectionSecurity::Tls))
        .await
        .unwrap();
    assert!(wrong.reason.is_some());
    assert_eq!(
        wrong.suggestion,
        Some(settings("127.0.0.1", smtp_port, ConnectionSecurity::Starttls))
    );
    let right: ServerCheckResult = desktop
        .call(methods::ACCOUNT_CHECK_SERVER, smtp(ConnectionSecurity::Starttls))
        .await
        .unwrap();
    assert_eq!(right, ServerCheckResult::default(), "nothing to offer when it answers");

    // Receiving is TLS only; and it reaches out to servers: the app's to ask.
    let starttls = ServerCheckParams {
        settings: settings("localhost", imap_port, ConnectionSecurity::Starttls),
        ..imap(imap_port)
    };
    let (code, _) = rpc_error(desktop.call::<_, Value>(methods::ACCOUNT_CHECK_SERVER, starttls).await);
    assert_eq!(code, codes::INVALID_PARAMS);
    let mut agent = Client::connect(&running.endpoint, ClientKind::Cli).await.unwrap();
    let (code, _) = rpc_error(
        agent
            .call::<_, Value>(methods::ACCOUNT_CHECK_SERVER, imap(imap_port))
            .await,
    );
    assert_eq!(code, codes::FORBIDDEN);
}
