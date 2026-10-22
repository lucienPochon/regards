//! Signatures (ADR-0021) end to end: a list for every account, one default
//! per address that new drafts get, and the signature text the address
//! editor still sends becoming one of the list. Only the app manages them.

#[path = "../../regards-imap/tests/support/fake_imap.rs"]
mod fake_imap;
#[path = "../../regards-smtp/tests/support/fake_smtp.rs"]
mod fake_smtp;
#[path = "support/world.rs"]
mod world;

use regards_core::headers::Person;
use regards_daemon::DEFAULT_LATE_SEND_GRACE;
use regards_ipc::protocol::{
    AccountRef, ClientKind, DraftCreateParams, DraftView, IdentityListResult, IdentitySaveParams,
    IdentitySetSignatureParams, IdentityView, SignatureListResult, SignatureRef, SignatureSaveParams, SignatureView,
    codes, methods,
};
use regards_ipc::{Client, ClientError};
use serde_json::Value;
use world::{Running, World};

async fn own_identity(desktop: &mut Client, account_id: &str) -> IdentityView {
    let listed: IdentityListResult = desktop
        .call(
            methods::IDENTITY_LIST,
            AccountRef {
                account_id: account_id.into(),
            },
        )
        .await
        .unwrap();
    listed.identities.into_iter().next().unwrap()
}

async fn signatures(desktop: &mut Client) -> Vec<(String, String)> {
    let listed: SignatureListResult = desktop.call(methods::SIGNATURE_LIST, Value::Null).await.unwrap();
    listed.signatures.into_iter().map(|s| (s.name, s.body)).collect()
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
                body: "Hello".into(),
                ..DraftCreateParams::default()
            },
        )
        .await
        .unwrap()
}

#[tokio::test]
async fn an_address_uses_one_of_the_list_and_new_drafts_get_it() {
    let world = World::new(DEFAULT_LATE_SEND_GRACE).await;
    let Running {
        endpoint,
        mut desktop,
        handle,
    } = world.start().await;
    let account = world.add_account(&mut desktop).await;
    let save = |id: Option<i64>, name: &str, body: &str| SignatureSaveParams {
        id,
        name: name.into(),
        body: body.into(),
    };

    // Kept by name; a name and some text are needed.
    let formal: SignatureView = desktop
        .call(
            methods::SIGNATURE_SAVE,
            save(None, " Formal ", "Ada Lovelace\nEngines Ltd\n\n"),
        )
        .await
        .unwrap();
    assert_eq!(
        (formal.name.as_str(), formal.body.as_str()),
        ("Formal", "Ada Lovelace\nEngines Ltd")
    );
    let short: SignatureView = desktop
        .call(methods::SIGNATURE_SAVE, save(None, "Short", "Ada"))
        .await
        .unwrap();
    assert_eq!(
        signatures(&mut desktop).await,
        [
            ("Formal".to_owned(), "Ada Lovelace\nEngines Ltd".to_owned()),
            ("Short".to_owned(), "Ada".to_owned()),
        ]
    );
    for empty in [save(None, "", "Ada"), save(None, "Blank", "  ")] {
        let refused = desktop.call::<_, SignatureView>(methods::SIGNATURE_SAVE, empty).await;
        assert!(matches!(refused, Err(ClientError::Rpc(e)) if e.code == codes::INVALID_PARAMS));
    }

    // The address's default: new drafts get it.
    let own = own_identity(&mut desktop, &account.id).await;
    let chosen: IdentityView = desktop
        .call(
            methods::IDENTITY_SET_SIGNATURE,
            IdentitySetSignatureParams {
                id: own.id,
                signature_id: Some(formal.id),
            },
        )
        .await
        .unwrap();
    assert_eq!(
        (chosen.signature_id, chosen.signature.as_str()),
        (Some(formal.id), "Ada Lovelace\nEngines Ltd")
    );
    let draft = new_draft(&mut desktop, &account.id).await;
    assert_eq!(draft.body, "Hello\n\nAda Lovelace\nEngines Ltd");

    // The address editor still sends text: the signature with that text is used, not a copy.
    let saved: IdentityView = desktop
        .call(
            methods::IDENTITY_SAVE,
            IdentitySaveParams {
                id: Some(own.id),
                account_id: account.id.clone(),
                name: own.name.clone(),
                email: own.email.clone(),
                signature_id: None,
                signature: "Ada".into(),
            },
        )
        .await
        .unwrap();
    assert_eq!(saved.signature_id, Some(short.id));
    assert_eq!(signatures(&mut desktop).await.len(), 2);

    // Deleted from the list: the address has none.
    let _: Value = desktop
        .call(methods::SIGNATURE_REMOVE, SignatureRef { id: short.id })
        .await
        .unwrap();
    let own = own_identity(&mut desktop, &account.id).await;
    assert_eq!((own.signature_id, own.signature.as_str()), (None, ""));
    assert_eq!(new_draft(&mut desktop, &account.id).await.body, "Hello");

    // Agents never manage them.
    let mut agent = Client::connect(&endpoint, ClientKind::Mcp).await.unwrap();
    let refused = agent
        .call::<_, SignatureListResult>(methods::SIGNATURE_LIST, Value::Null)
        .await;
    assert!(matches!(refused, Err(ClientError::Rpc(e)) if e.code == codes::FORBIDDEN));
    handle.abort();
}
