//! Deleting a draft can be undone for a while: it comes back as it was.

#[path = "../../regards-imap/tests/support/fake_imap.rs"]
mod fake_imap;
#[path = "../../regards-smtp/tests/support/fake_smtp.rs"]
mod fake_smtp;
#[path = "support/world.rs"]
mod world;

use regards_core::headers::Person;
use regards_daemon::DEFAULT_LATE_SEND_GRACE;
use regards_ipc::protocol::{
    DraftCreateParams, DraftListParams, DraftListResult, DraftRef, DraftUpdateParams, DraftView, UndoInfo, UndoParams,
    UndoResult, methods,
};
use serde::Deserialize;
use world::{Running, World};

#[derive(Deserialize)]
struct Deleted {
    deleted: bool,
    undo: Option<UndoInfo>,
}

#[tokio::test]
async fn a_deleted_draft_comes_back_as_it_was_when_undone() {
    let world = World::new(DEFAULT_LATE_SEND_GRACE).await;
    let Running {
        mut desktop, handle, ..
    } = world.start().await;
    let account = world.add_account(&mut desktop).await;
    let bob = Person {
        name: Some("Bob".into()),
        address: Some("bob@example.test".into()),
    };
    let draft: DraftView = desktop
        .call(
            methods::DRAFT_CREATE,
            DraftCreateParams {
                account_id: account.id.clone(),
                ..DraftCreateParams::default()
            },
        )
        .await
        .unwrap();
    let _: DraftView = desktop
        .call(
            methods::DRAFT_UPDATE,
            DraftUpdateParams {
                id: draft.id,
                to: vec![bob.clone()],
                cc: vec![],
                bcc: vec![],
                subject: "Plans".into(),
                body: String::new(),
                html: Some("<p>See you <b>Friday</b></p>".into()),
            },
        )
        .await
        .unwrap();

    let deleted: Deleted = desktop
        .call(methods::DRAFT_DELETE, DraftRef { id: draft.id })
        .await
        .unwrap();
    assert!(deleted.deleted);
    let undo = deleted.undo.expect("it can be undone");
    assert_eq!(undo.description, "Deleted a draft");
    let drafts = |listed: DraftListResult| listed.drafts;
    let listed: DraftListResult = desktop
        .call(methods::DRAFT_LIST, DraftListParams { account_id: None })
        .await
        .unwrap();
    assert!(drafts(listed).is_empty());

    let undone: UndoResult = desktop
        .call(methods::UNDO_APPLY, UndoParams { id: undo.id })
        .await
        .unwrap();
    assert_eq!(undone.undone, 1);
    let back = drafts(
        desktop
            .call(methods::DRAFT_LIST, DraftListParams { account_id: None })
            .await
            .unwrap(),
    );
    assert_eq!(back.len(), 1);
    assert_eq!(
        (back[0].subject.as_str(), back[0].to.as_slice(), back[0].html.as_deref()),
        (
            "Plans",
            std::slice::from_ref(&bob),
            Some("<p>See you <b>Friday</b></p>")
        )
    );

    // Once only.
    let again = desktop
        .call::<_, UndoResult>(methods::UNDO_APPLY, UndoParams { id: undo.id })
        .await;
    assert!(again.is_err());
    handle.abort();
}
