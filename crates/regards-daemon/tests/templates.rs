//! Templates end to end (plan §5.9): kept by name, their formatting
//! sanitised like an outgoing message's.

#[path = "../../regards-imap/tests/support/fake_imap.rs"]
mod fake_imap;
#[path = "../../regards-smtp/tests/support/fake_smtp.rs"]
mod fake_smtp;
#[path = "support/world.rs"]
mod world;

use regards_daemon::DEFAULT_LATE_SEND_GRACE;
use regards_ipc::protocol::{TemplateListResult, TemplateRef, TemplateSaveParams, TemplateView, codes, methods};
use regards_ipc::{Client, ClientError};
use serde_json::Value;
use world::{Running, World};

fn code<T: std::fmt::Debug>(result: Result<T, ClientError>) -> i32 {
    match result {
        Err(ClientError::Rpc(error)) => error.code,
        other => panic!("expected an RPC error, got {other:?}"),
    }
}

async fn templates(desktop: &mut Client) -> Vec<TemplateView> {
    desktop
        .call::<_, TemplateListResult>(methods::TEMPLATE_LIST, Value::Null)
        .await
        .unwrap()
        .templates
}

#[tokio::test]
async fn templates_are_kept_sanitised_and_by_name() {
    let world = World::new(DEFAULT_LATE_SEND_GRACE).await;
    let Running {
        mut desktop, handle, ..
    } = world.start().await;

    let saved: TemplateView = desktop
        .call(
            methods::TEMPLATE_SAVE,
            TemplateSaveParams {
                id: None,
                name: "  Booking reply ".into(),
                subject: "Your booking".into(),
                body: String::new(),
                html: Some("<p>Thanks, <b>see you</b> then.</p><script>steal()</script>".into()),
            },
        )
        .await
        .unwrap();
    assert_eq!(saved.name, "Booking reply");
    let html = saved.html.as_deref().unwrap();
    assert!(html.contains("<b>see you</b>") && !html.contains("script"), "{html}");
    assert!(saved.body.contains("Thanks, see you then."), "{:?}", saved.body);

    let plain: TemplateView = desktop
        .call(
            methods::TEMPLATE_SAVE,
            TemplateSaveParams {
                id: None,
                name: "Apology".into(),
                subject: String::new(),
                body: "Sorry for the wait.".into(),
                html: None,
            },
        )
        .await
        .unwrap();
    let names: Vec<_> = templates(&mut desktop).await.into_iter().map(|t| t.name).collect();
    assert_eq!(names, ["Apology", "Booking reply"]);

    // Changed in place; a name is needed; an unknown one cannot be changed.
    let renamed: TemplateView = desktop
        .call(
            methods::TEMPLATE_SAVE,
            TemplateSaveParams {
                id: Some(plain.id),
                name: "Apology (late)".into(),
                subject: String::new(),
                body: "Sorry for the wait.".into(),
                html: None,
            },
        )
        .await
        .unwrap();
    assert_eq!(renamed.id, plain.id);
    let nameless = TemplateSaveParams {
        id: None,
        name: "   ".into(),
        subject: String::new(),
        body: "x".into(),
        html: None,
    };
    assert_eq!(
        code(desktop.call::<_, TemplateView>(methods::TEMPLATE_SAVE, nameless).await),
        codes::INVALID_PARAMS
    );

    let _: Value = desktop
        .call(methods::TEMPLATE_REMOVE, TemplateRef { id: saved.id })
        .await
        .unwrap();
    assert_eq!(templates(&mut desktop).await.len(), 1);
    assert_eq!(
        code(
            desktop
                .call::<_, Value>(methods::TEMPLATE_REMOVE, TemplateRef { id: saved.id })
                .await
        ),
        codes::NOT_FOUND
    );
    handle.abort();
}
