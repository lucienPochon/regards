//! Identities end to end (plan §5.9), against fake IMAP and SMTP servers: an
//! account sends as its own address or an alias, each with its signature,
//! and a draft's signature follows when it changes who it is from.

#[path = "../../regards-imap/tests/support/fake_imap.rs"]
mod fake_imap;
#[path = "../../regards-smtp/tests/support/fake_smtp.rs"]
mod fake_smtp;
#[path = "support/world.rs"]
mod world;

use regards_core::headers::{self, Person};
use regards_daemon::DEFAULT_LATE_SEND_GRACE;
use regards_ipc::protocol::{
    AccountRef, DraftCreateParams, DraftSendParams, DraftSendResult, DraftSetFromParams, DraftUpdateParams, DraftView,
    IdentityListResult, IdentityRef, IdentitySaveParams, IdentityView, codes, methods,
};
use regards_ipc::{Client, ClientError};
use serde_json::Value;
use std::time::{Duration, Instant};
use world::{Running, World};

fn code<T: std::fmt::Debug>(result: Result<T, ClientError>) -> i32 {
    match result {
        Err(ClientError::Rpc(error)) => error.code,
        other => panic!("expected an RPC error, got {other:?}"),
    }
}

async fn identities(desktop: &mut Client, account_id: &str) -> Vec<IdentityView> {
    let listed: IdentityListResult = desktop
        .call(
            methods::IDENTITY_LIST,
            AccountRef {
                account_id: account_id.into(),
            },
        )
        .await
        .unwrap();
    listed.identities
}

async fn save(desktop: &mut Client, params: IdentitySaveParams) -> Result<IdentityView, ClientError> {
    desktop.call(methods::IDENTITY_SAVE, params).await
}

async fn new_draft(desktop: &mut Client, account_id: &str) -> DraftView {
    desktop
        .call(
            methods::DRAFT_CREATE,
            DraftCreateParams {
                account_id: account_id.into(),
                to: vec![Person {
                    name: None,
                    address: Some("bob@example.test".into()),
                }],
                subject: "Plans".into(),
                ..DraftCreateParams::default()
            },
        )
        .await
        .unwrap()
}

#[tokio::test]
async fn a_message_goes_out_from_the_identity_chosen_with_its_signature() {
    let mut world = World::new(DEFAULT_LATE_SEND_GRACE).await;
    let Running {
        mut desktop, handle, ..
    } = world.start().await;
    let account = world.add_account(&mut desktop).await;

    // The account's own address comes first, as the default.
    let listed = identities(&mut desktop, &account.id).await;
    assert_eq!(listed.len(), 1);
    assert!(listed[0].is_default);
    assert_eq!(
        (listed[0].name.as_str(), listed[0].email.as_str()),
        ("Ada", "ada@example.test")
    );

    // It gets a signature, and an alias gets its own.
    let own = save(
        &mut desktop,
        IdentitySaveParams {
            id: Some(listed[0].id),
            account_id: account.id.clone(),
            name: "Ada".into(),
            email: "ada@example.test".into(),
            signature: "Ada\nRegards test\n\n".into(),
        },
    )
    .await
    .unwrap();
    assert_eq!(own.signature, "Ada\nRegards test", "trailing blank lines go");
    let alias = save(
        &mut desktop,
        IdentitySaveParams {
            id: None,
            account_id: account.id.clone(),
            name: "Ada at work".into(),
            email: "ada@work.test".into(),
            signature: "Ada Lovelace\nAnalytical Engines".into(),
        },
    )
    .await
    .unwrap();
    assert!(!alias.is_default);
    let bad = |email: &str| IdentitySaveParams {
        id: None,
        account_id: account.id.clone(),
        name: String::new(),
        email: email.into(),
        signature: String::new(),
    };
    assert_eq!(
        code(save(&mut desktop, bad("not an address")).await),
        codes::INVALID_PARAMS
    );
    assert_eq!(
        code(save(&mut desktop, bad("ADA@work.test")).await),
        codes::CONFLICT,
        "taken"
    );

    // A new message starts with the default's signature.
    let draft = new_draft(&mut desktop, &account.id).await;
    assert_eq!(draft.body, "\n\nAda\nRegards test");
    assert_eq!(draft.identity_id, Some(own.id));
    assert_eq!(draft.from.address.as_deref(), Some("ada@example.test"));

    // Written, then sent from the alias: its signature takes the other's place.
    let _: DraftView = desktop
        .call(
            methods::DRAFT_UPDATE,
            DraftUpdateParams {
                id: draft.id,
                to: draft.to.clone(),
                cc: vec![],
                bcc: vec![],
                subject: "Plans".into(),
                body: "See you at 10.\n\nAda\nRegards test".into(),
                html: None,
            },
        )
        .await
        .unwrap();
    let moved: DraftView = desktop
        .call(
            methods::DRAFT_SET_FROM,
            DraftSetFromParams {
                id: draft.id,
                identity_id: alias.id,
            },
        )
        .await
        .unwrap();
    assert_eq!(moved.body, "See you at 10.\n\nAda Lovelace\nAnalytical Engines");
    assert_eq!(
        (moved.from.name.as_deref(), moved.from.address.as_deref()),
        (Some("Ada at work"), Some("ada@work.test"))
    );
    let elsewhere = desktop
        .call::<_, DraftView>(
            methods::DRAFT_SET_FROM,
            DraftSetFromParams {
                id: draft.id,
                identity_id: alias.id + 1000,
            },
        )
        .await;
    assert_eq!(code(elsewhere), codes::CONFLICT);

    // It goes out as the alias: envelope, From and Message-ID.
    let _: DraftSendResult = desktop
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
    let deadline = Instant::now() + Duration::from_secs(15);
    let session = loop {
        let left = deadline.saturating_duration_since(Instant::now());
        let session = tokio::time::timeout(left, world.sent.next()).await.expect("sent");
        if session.data.is_some() {
            break session;
        }
    };
    assert_eq!(session.from.as_deref(), Some("ada@work.test"));
    let data = session.data.unwrap();
    let parsed = headers::parse(&data);
    assert_eq!(parsed.from.unwrap().address.as_deref(), Some("ada@work.test"));
    assert!(parsed.message_id.unwrap().ends_with("@work.test"));
    assert!(String::from_utf8_lossy(&data).contains("Analytical Engines"));

    // The default cannot go; once the alias is the default, it can.
    let ref_of = |id| IdentityRef { id };
    assert_eq!(
        code(desktop.call::<_, Value>(methods::IDENTITY_REMOVE, ref_of(own.id)).await),
        codes::CONFLICT
    );
    let _: Value = desktop
        .call(methods::IDENTITY_MAKE_DEFAULT, ref_of(alias.id))
        .await
        .unwrap();
    assert_eq!(identities(&mut desktop, &account.id).await[0].id, alias.id);
    let _: Value = desktop.call(methods::IDENTITY_REMOVE, ref_of(own.id)).await.unwrap();
    let next = new_draft(&mut desktop, &account.id).await;
    assert_eq!(next.body, "\n\nAda Lovelace\nAnalytical Engines");
    handle.abort();
}
