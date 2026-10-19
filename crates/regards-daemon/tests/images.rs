//! Images in a message's text (plan §5.9): kept with a Content-ID, shown
//! back for writing, and sent with the HTML as multipart/related.

#[path = "../../regards-imap/tests/support/fake_imap.rs"]
mod fake_imap;
#[path = "../../regards-smtp/tests/support/fake_smtp.rs"]
mod fake_smtp;
#[path = "support/world.rs"]
mod world;

use mail_parser::{MessageParser, MimeHeaders};
use regards_core::encoding::base64;
use regards_core::headers::Person;
use regards_daemon::DEFAULT_LATE_SEND_GRACE;
use regards_ipc::ClientError;
use regards_ipc::protocol::{
    DraftAttachImageParams, DraftCreateParams, DraftImageAttached, DraftImagesResult, DraftRef, DraftSendParams,
    DraftUpdateParams, DraftView, codes, methods,
};
use serde_json::Value;
use world::{Running, World};

const PNG: &[u8] = b"\x89PNG\r\n\x1a\n not really an image, but its bytes";

#[tokio::test]
async fn an_image_in_the_text_is_kept_shown_back_and_sent_with_the_html() {
    let mut world = World::new(DEFAULT_LATE_SEND_GRACE).await;
    let Running {
        mut desktop, handle, ..
    } = world.start().await;
    let account = world.add_account(&mut desktop).await;
    let draft: DraftView = desktop
        .call(
            methods::DRAFT_CREATE,
            DraftCreateParams {
                account_id: account.id.clone(),
                to: vec![Person {
                    name: Some("Bob".into()),
                    address: Some("bob@example.test".into()),
                }],
                subject: "The lake".into(),
                ..DraftCreateParams::default()
            },
        )
        .await
        .unwrap();
    let image = |mime: &str, data: String| DraftAttachImageParams {
        draft_id: draft.id,
        name: "lake.png".into(),
        mime: mime.into(),
        data,
    };

    // Pasted: kept as an image of the text, not a file beside it.
    let pasted: DraftImageAttached = desktop
        .call(methods::DRAFT_ATTACH_IMAGE, image("image/png", base64(PNG)))
        .await
        .unwrap();
    let cid = pasted.content_id.clone();
    assert!(cid.ends_with("@regards"));
    assert_eq!(pasted.draft.attachments[0].content_id.as_deref(), Some(cid.as_str()));

    // Only images, and not too big.
    let refuse = |result: Result<DraftImageAttached, ClientError>| match result {
        Err(ClientError::Rpc(error)) => error.code,
        other => panic!("expected a refusal, got {other:?}"),
    };
    assert_eq!(
        refuse(
            desktop
                .call(methods::DRAFT_ATTACH_IMAGE, image("application/pdf", base64(PNG)))
                .await
        ),
        codes::INVALID_PARAMS
    );
    let huge = base64(&vec![0u8; 6 * 1024 * 1024]);
    assert_eq!(
        refuse(
            desktop
                .call(methods::DRAFT_ATTACH_IMAGE, image("image/png", huge))
                .await
        ),
        codes::TOO_LARGE
    );
    assert_eq!(
        refuse(
            desktop
                .call(methods::DRAFT_ATTACH_IMAGE, image("image/png", "not base64!".into()))
                .await
        ),
        codes::INVALID_PARAMS
    );

    // The text refers to it; writing shows it from its bytes.
    let html = format!(r#"<p>Look at this</p><p><img src="cid:{cid}" alt="The lake"></p>"#);
    let _: DraftView = desktop
        .call(
            methods::DRAFT_UPDATE,
            DraftUpdateParams {
                id: draft.id,
                to: draft.to.clone(),
                cc: vec![],
                bcc: vec![],
                subject: "The lake".into(),
                body: String::new(),
                html: Some(html),
            },
        )
        .await
        .unwrap();
    let shown: DraftImagesResult = desktop
        .call(methods::DRAFT_IMAGES, DraftRef { id: draft.id })
        .await
        .unwrap();
    assert_eq!(shown.images.len(), 1, "a save just after pasting keeps it");
    assert_eq!(
        (shown.images[0].content_id.as_str(), shown.images[0].data.as_str()),
        (cid.as_str(), base64(PNG).as_str())
    );

    // Sent: the image goes with the HTML, which refers to it.
    let _: Value = desktop
        .call(
            methods::DRAFT_SEND,
            DraftSendParams {
                id: draft.id,
                undo_seconds: Some(0),
                send_at: None,
                remind_at: None,
            },
        )
        .await
        .unwrap();
    let sent = world.next_message().await;
    let parsed = MessageParser::new().parse(&sent).unwrap();
    assert!(parsed.body_html(0).unwrap().contains(&format!("cid:{cid}")));
    let part = parsed
        .parts
        .iter()
        .find(|part| part.content_id() == Some(cid.as_str()))
        .expect("the image went with the message");
    assert_eq!(part.contents(), PNG);
    let related = parsed
        .parts
        .iter()
        .any(|part| part.content_type().is_some_and(|t| t.subtype() == Some("related")));
    assert!(related, "the HTML and its image together");
    handle.abort();
}
