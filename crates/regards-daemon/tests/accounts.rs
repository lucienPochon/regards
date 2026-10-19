//! Account methods end to end: pipe, principals, IMAP sign-in check, store and vault.

#[path = "../../regards-imap/tests/support/fake_imap.rs"]
mod fake_imap;

use fake_imap::{PASSWORD, fake_server, pki};
use regards_core::mailbox::Role;
use regards_core::paths::DataDirs;
use regards_core::secret::Secret;
use regards_daemon::Daemon;
use regards_imap::TlsOptions;
use regards_ipc::protocol::{
    AccountAddParams, AccountRef, AccountRemoveParams, AccountSummary, ClientKind, ConnectionSecurity,
    MailboxListResult, ServerSettings, SyncResult, codes, methods,
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
    assert_eq!(data, Some(json!({ "reason": "auth_failed" })));

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
    assert_eq!(data, Some(json!({ "reason": "tls" })));

    let listed: Vec<AccountSummary> = desktop.call(methods::ACCOUNT_LIST, Value::Null).await.unwrap();
    assert!(listed.is_empty());
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
