//! Account methods end to end: pipe, principals, IMAP sign-in check, store and vault.

#[path = "../../regards-imap/tests/support/fake_imap.rs"]
mod fake_imap;

use fake_imap::{PASSWORD, fake_server, pki};
use regards_core::paths::DataDirs;
use regards_core::secret::Secret;
use regards_daemon::Daemon;
use regards_imap::TlsOptions;
use regards_ipc::protocol::{
    AccountAddParams, AccountRemoveParams, AccountSummary, ClientKind, ConnectionSecurity, ServerSettings, codes,
    methods,
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
        .with_imap_tls(tls);
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
async fn agents_cannot_list_add_or_remove_accounts() {
    let running = start(TlsOptions::platform()).await;
    let mut agent = Client::connect(&running.endpoint, ClientKind::Cli).await.unwrap();
    let add = serde_json::to_value(params("ada@example.test", 1, PASSWORD)).unwrap();
    for (method, body) in [
        (methods::ACCOUNT_LIST, Value::Null),
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
    let (port, sent) = fake_server(&pki, "* OK ready\r\n", PASSWORD).await;
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
    assert_eq!(sent.await.unwrap().as_deref(), Some(PASSWORD));
    assert_eq!(added.email, "Ada@Example.test");
    assert_eq!(
        added.username, "Ada@Example.test",
        "the user name defaults to the address"
    );
    assert_eq!(added.imap_capabilities, ["IMAP4REV1", "IDLE", "MOVE", "UIDPLUS"]);

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
