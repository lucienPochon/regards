//! "Remove everything from this computer": every credential at once, then
//! the daemon stops and leaves its files to be removed once nothing holds
//! them (by the app, or by the next daemon to start: see `erase.rs`).

#[path = "../../regards-imap/tests/support/fake_imap.rs"]
mod fake_imap;
#[path = "../../regards-smtp/tests/support/fake_smtp.rs"]
mod fake_smtp;
#[path = "support/world.rs"]
mod world;

use regards_core::secret::Secret;
use regards_daemon::DEFAULT_LATE_SEND_GRACE;
use regards_ipc::protocol::{
    AssistantKind, AssistantSetSettingsParams, AssistantSettings, ClientKind, DaemonEraseParams, DaemonErased, codes,
    methods,
};
use regards_ipc::{Client, ClientError};
use regards_vault::{SecretKey, SecretKind};
use std::time::Duration;
use world::{Running, World};

#[tokio::test]
async fn every_credential_goes_at_once_and_the_daemon_stops_to_let_its_files_go() {
    let world = World::new(DEFAULT_LATE_SEND_GRACE).await;
    let Running {
        endpoint,
        mut desktop,
        handle,
    } = world.start().await;
    let account = world.add_account(&mut desktop).await;
    let _: AssistantSettings = desktop
        .call(
            methods::ASSISTANT_SET_SETTINGS,
            AssistantSetSettingsParams {
                kind: Some(AssistantKind::Anthropic),
                base_url: None,
                model: None,
                api_key: Some(Secret::new("sk-ant-test")),
            },
        )
        .await
        .unwrap();
    let password = SecretKey::new(&account.id, SecretKind::MailPassword);
    let assistant = SecretKey::from_stored("assistant/api-key");
    assert!(world.vault.get(&password).await.unwrap().is_some());

    let ask = |confirm: &str| DaemonEraseParams {
        confirm: confirm.into(),
    };
    // Never by mistake, and never by an agent.
    let refused = desktop.call::<_, DaemonErased>(methods::DAEMON_ERASE, ask("yes")).await;
    assert!(matches!(refused, Err(ClientError::Rpc(e)) if e.code == codes::INVALID_PARAMS));
    let mut agent = Client::connect(&endpoint, ClientKind::Cli).await.unwrap();
    let refused = agent
        .call::<_, DaemonErased>(methods::DAEMON_ERASE, DaemonEraseParams::confirmed())
        .await;
    assert!(matches!(refused, Err(ClientError::Rpc(e)) if e.code == codes::FORBIDDEN));
    drop(agent);

    let erased: DaemonErased = desktop
        .call(methods::DAEMON_ERASE, DaemonEraseParams::confirmed())
        .await
        .unwrap();
    assert_eq!(erased.credentials, 2, "the account's password and the assistant's key");
    assert!(world.vault.get(&password).await.unwrap().is_none());
    assert!(world.vault.get(&assistant).await.unwrap().is_none());
    tokio::time::timeout(Duration::from_secs(10), handle)
        .await
        .expect("the daemon stops")
        .unwrap();
    assert!(
        world.temp.path().join("erase-pending").exists(),
        "the next start finishes the job"
    );
}
