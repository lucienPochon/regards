//! The assistant's settings (ADR-0015 §5): off until chosen, a key kept in
//! the credential store and never shown back, forgotten when the server
//! changes, and nothing of it open to agents. Summaries and replies are
//! tested against a real IMAP server, in `live.rs`.

#[path = "../../regards-imap/tests/support/fake_imap.rs"]
mod fake_imap;
#[path = "../../regards-smtp/tests/support/fake_smtp.rs"]
mod fake_smtp;
#[path = "support/world.rs"]
mod world;

use regards_core::secret::Secret;
use regards_daemon::DEFAULT_LATE_SEND_GRACE;
use regards_ipc::protocol::{
    AssistantKind, AssistantParams, AssistantPreview, AssistantSetSettingsParams, AssistantSettings, AssistantTask,
    ClientKind, codes, methods,
};
use regards_ipc::{Client, ClientError};
use regards_vault::SecretKey;
use serde_json::Value;
use world::{Running, World};

fn code<T: std::fmt::Debug>(result: Result<T, ClientError>) -> i32 {
    match result {
        Err(ClientError::Rpc(error)) => error.code,
        other => panic!("expected an RPC error, got {other:?}"),
    }
}

fn choose(kind: Option<AssistantKind>, base_url: Option<&str>, api_key: Option<&str>) -> AssistantSetSettingsParams {
    AssistantSetSettingsParams {
        kind,
        base_url: base_url.map(Into::into),
        model: None,
        api_key: api_key.map(Secret::new),
    }
}

#[tokio::test]
async fn the_assistant_is_off_until_chosen_and_its_key_stays_where_it_was_given() {
    let world = World::new(DEFAULT_LATE_SEND_GRACE).await;
    let Running {
        endpoint,
        mut desktop,
        handle,
    } = world.start().await;
    let key = SecretKey::from_stored("assistant/api-key");

    let settings: AssistantSettings = desktop.call(methods::ASSISTANT_SETTINGS, Value::Null).await.unwrap();
    assert_eq!(settings.kind, None, "off by default");
    let ask = AssistantParams {
        message_id: 1,
        task: AssistantTask::Summary,
        all: false,
    };
    assert_eq!(
        code(
            desktop
                .call::<_, AssistantPreview>(methods::ASSISTANT_PREVIEW, &ask)
                .await
        ),
        codes::CONFLICT,
        "nothing goes while it is off"
    );

    // Anthropic needs a key; the address and model are its own.
    assert_eq!(
        code(
            desktop
                .call::<_, AssistantSettings>(
                    methods::ASSISTANT_SET_SETTINGS,
                    choose(Some(AssistantKind::Anthropic), None, None)
                )
                .await
        ),
        codes::INVALID_PARAMS
    );
    let chosen: AssistantSettings = desktop
        .call(
            methods::ASSISTANT_SET_SETTINGS,
            choose(Some(AssistantKind::Anthropic), None, Some(" sk-ant-test ")),
        )
        .await
        .unwrap();
    assert_eq!(chosen.base_url.as_deref(), Some("https://api.anthropic.com"));
    assert_eq!(chosen.model.as_deref(), Some("claude-opus-5-5"));
    assert!(chosen.has_key);
    let text = serde_json::to_string(&chosen).unwrap();
    assert!(!text.contains("sk-ant-test"), "the key never comes back");
    assert_eq!(world.vault.get(&key).await.unwrap().unwrap().expose(), "sk-ant-test");
    // Chosen again without a key: the one kept stays.
    let again: AssistantSettings = desktop
        .call(
            methods::ASSISTANT_SET_SETTINGS,
            choose(Some(AssistantKind::Anthropic), None, None),
        )
        .await
        .unwrap();
    assert!(again.has_key);

    // Another server does not get it.
    for refused in ["http://example.org/v1", "https://user:pw@example.org/v1", "file:///etc"] {
        assert_eq!(
            code(
                desktop
                    .call::<_, AssistantSettings>(
                        methods::ASSISTANT_SET_SETTINGS,
                        choose(Some(AssistantKind::OpenaiCompatible), Some(refused), None)
                    )
                    .await
            ),
            codes::INVALID_PARAMS,
            "{refused}"
        );
    }
    let local: AssistantSettings = desktop
        .call(
            methods::ASSISTANT_SET_SETTINGS,
            AssistantSetSettingsParams {
                model: Some("llama3.1".into()),
                ..choose(
                    Some(AssistantKind::OpenaiCompatible),
                    Some("http://localhost:11434/v1/"),
                    None,
                )
            },
        )
        .await
        .unwrap();
    assert_eq!(local.base_url.as_deref(), Some("http://localhost:11434/v1"));
    assert!(!local.has_key, "the Anthropic key was forgotten, not sent elsewhere");
    assert!(world.vault.get(&key).await.unwrap().is_none());
    assert_eq!(
        code(
            desktop
                .call::<_, AssistantSettings>(
                    methods::ASSISTANT_SET_SETTINGS,
                    choose(
                        Some(AssistantKind::OpenaiCompatible),
                        Some("https://example.org/v1"),
                        None
                    )
                )
                .await
        ),
        codes::INVALID_PARAMS,
        "an OpenAI-compatible server needs a model named"
    );

    // Nothing of it is open to agents.
    let mut agent = Client::connect(&endpoint, ClientKind::Cli).await.unwrap();
    assert_eq!(
        code(
            agent
                .call::<_, AssistantSettings>(methods::ASSISTANT_SETTINGS, Value::Null)
                .await
        ),
        codes::FORBIDDEN
    );
    assert_eq!(
        code(agent.call::<_, Value>(methods::ASSISTANT_RUN, &ask).await),
        codes::FORBIDDEN
    );

    // Off forgets everything.
    let off: AssistantSettings = desktop
        .call(methods::ASSISTANT_SET_SETTINGS, choose(None, None, None))
        .await
        .unwrap();
    assert_eq!(
        off,
        AssistantSettings {
            kind: None,
            base_url: None,
            model: None,
            has_key: false
        }
    );
    handle.abort();
}
