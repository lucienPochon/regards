//! Templates end to end (plan §5.9): kept by name, their formatting
//! sanitised like an outgoing message's.

#[path = "../../regards-imap/tests/support/fake_imap.rs"]
mod fake_imap;
#[path = "../../regards-smtp/tests/support/fake_smtp.rs"]
mod fake_smtp;
#[path = "support/world.rs"]
mod world;

use regards_core::headers::Person;
use regards_daemon::DEFAULT_LATE_SEND_GRACE;
use regards_ipc::protocol::{
    DraftAttachFileParams, DraftCreateParams, DraftUseTemplateParams, DraftView, TemplateListResult, TemplateRef,
    TemplateSaveParams, TemplateView, codes, methods,
};
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
                ..TemplateSaveParams::default()
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
                ..TemplateSaveParams::default()
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
                ..TemplateSaveParams::default()
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
        ..TemplateSaveParams::default()
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

#[tokio::test]
async fn a_template_keeps_recipients_and_files_and_puts_the_files_on_a_draft() {
    let world = World::new(DEFAULT_LATE_SEND_GRACE).await;
    let Running {
        mut desktop, handle, ..
    } = world.start().await;
    let account = world.add_account(&mut desktop).await;
    let draft = |subject: &str| DraftCreateParams {
        account_id: account.id.clone(),
        subject: subject.into(),
        ..DraftCreateParams::default()
    };
    let written: DraftView = desktop.call(methods::DRAFT_CREATE, draft("Booking")).await.unwrap();
    let terms = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(terms.path(), b"Terms and conditions").unwrap();
    let attached: DraftView = desktop
        .call(
            methods::DRAFT_ATTACH_FILE,
            DraftAttachFileParams {
                draft_id: written.id,
                path: terms.path().to_string_lossy().into_owned(),
                inline: false,
            },
        )
        .await
        .unwrap();
    assert_eq!(attached.attachments.len(), 1);

    // Saved from the draft: its recipients and its file.
    let desk = Person {
        name: Some("Desk".into()),
        address: Some("desk@example.test".into()),
    };
    let saved: TemplateView = desktop
        .call(
            methods::TEMPLATE_SAVE,
            TemplateSaveParams {
                name: "Booking".into(),
                body: "Here are the terms.".into(),
                to: vec![desk.clone()],
                files_of_draft: Some(written.id),
                ..TemplateSaveParams::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(saved.to, [desk]);
    assert_eq!(saved.files.len(), 1);
    assert!(!saved.files[0].inline);

    // Put on a new draft, once: the same file, twice asked.
    let fresh: DraftView = desktop.call(methods::DRAFT_CREATE, draft("Another")).await.unwrap();
    let used = DraftUseTemplateParams {
        draft_id: fresh.id,
        template_id: saved.id,
    };
    let with: DraftView = desktop.call(methods::DRAFT_USE_TEMPLATE, used).await.unwrap();
    assert_eq!(with.attachments.len(), 1);
    assert_eq!(with.attachments[0].size, 20);
    let again: DraftView = desktop.call(methods::DRAFT_USE_TEMPLATE, used).await.unwrap();
    assert_eq!(again.attachments.len(), 1, "not twice");

    // Taken off the template: drafts keep theirs.
    let trimmed: TemplateView = desktop
        .call(
            methods::TEMPLATE_SAVE,
            TemplateSaveParams {
                id: Some(saved.id),
                name: "Booking".into(),
                body: "Here are the terms.".into(),
                remove_files: vec![saved.files[0].id],
                ..TemplateSaveParams::default()
            },
        )
        .await
        .unwrap();
    assert!(trimmed.files.is_empty() && trimmed.to.is_empty());
    assert_eq!(
        code(
            desktop
                .call::<_, DraftView>(
                    methods::DRAFT_USE_TEMPLATE,
                    DraftUseTemplateParams {
                        draft_id: fresh.id,
                        template_id: 999
                    }
                )
                .await
        ),
        codes::NOT_FOUND
    );
    handle.abort();
}
