//! Voices and notes (ADR-0020) end to end: only the person changes a voice,
//! an agent's suggestion waits for them; agents keep notes about people,
//! those that could plant something wait, and every agent's note can be
//! undone. Switched off, notes reach nobody.

#[path = "../../regards-imap/tests/support/fake_imap.rs"]
mod fake_imap;
#[path = "../../regards-smtp/tests/support/fake_smtp.rs"]
mod fake_smtp;
#[path = "support/world.rs"]
mod world;

use regards_core::headers::Person;
use regards_daemon::DEFAULT_LATE_SEND_GRACE;
use regards_ipc::protocol::{
    AccountRef, AgentAccess, AgentAccessParams, AgentContext, ClientKind, DraftCreateParams, HoldApproveParams,
    HoldListParams, HoldListResult, HoldReason, HoldRef, HoldStatus, HoldView, IdentityListResult,
    IdentitySetVoiceParams, IdentityView, MemoryParams, MemoryResult, NoteAddParams, NoteAddResult, NoteAuthor,
    NoteListParams, NoteListResult, NoteRef, NoteRemoved, NoteSetEnabledParams, NoteSettings, NotesForgotten, RpcError,
    UndoParams, VoiceSuggestParams, codes, methods,
};
use regards_ipc::{Client, ClientError};
use serde_json::Value;
use world::{Running, World};

fn refusal<T: std::fmt::Debug>(result: Result<T, ClientError>) -> RpcError {
    match result {
        Err(ClientError::Rpc(error)) => error,
        other => panic!("expected an RPC error, got {other:?}"),
    }
}

/// The id of the hold a refused request was held as.
fn held<T: std::fmt::Debug>(result: Result<T, ClientError>) -> i64 {
    let error = refusal(result);
    assert_eq!(error.code, codes::HELD, "{}", error.message);
    error.data.unwrap()["hold_id"].as_i64().unwrap()
}

async fn set_access(desktop: &mut Client, account_id: &str, access: AgentAccess) {
    let _: Value = desktop
        .call(
            methods::ACCOUNT_SET_AGENT_ACCESS,
            AgentAccessParams {
                account_id: account_id.into(),
                access,
            },
        )
        .await
        .unwrap();
}

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

async fn waiting(desktop: &mut Client) -> Vec<HoldView> {
    let listed: HoldListResult = desktop
        .call(
            methods::HOLD_LIST,
            HoldListParams {
                pending: true,
                limit: None,
            },
        )
        .await
        .unwrap();
    listed.holds
}

fn note(account_id: &str, address: &str, text: &str) -> NoteAddParams {
    NoteAddParams {
        account_id: account_id.into(),
        address: address.into(),
        text: text.into(),
        from_message: None,
    }
}

async fn notes_about(client: &mut Client, account_id: &str, address: &str) -> Vec<(String, NoteAuthor)> {
    let listed: NoteListResult = client
        .call(
            methods::NOTE_LIST,
            NoteListParams {
                account_id: account_id.into(),
                address: address.into(),
            },
        )
        .await
        .unwrap();
    listed.notes.into_iter().map(|n| (n.text, n.by)).collect()
}

#[tokio::test]
async fn only_the_person_sets_a_voice_and_an_agents_suggestion_waits_for_them() {
    let world = World::new(DEFAULT_LATE_SEND_GRACE).await;
    let Running {
        endpoint,
        mut desktop,
        handle,
    } = world.start().await;
    let account = world.add_account(&mut desktop).await;
    set_access(&mut desktop, &account.id, AgentAccess::Triage).await;
    let mut agent = Client::connect(&endpoint, ClientKind::Mcp).await.unwrap();

    // Off and empty at first; the person sets it.
    let own = own_identity(&mut desktop, &account.id).await;
    assert_eq!((own.voice.as_str(), own.voice_on), ("", false));
    let set = |voice: &str, on: bool| IdentitySetVoiceParams {
        id: own.id,
        voice: voice.into(),
        on,
    };
    let saved: IdentityView = desktop
        .call(methods::IDENTITY_SET_VOICE, set("  Short and warm. ", true))
        .await
        .unwrap();
    assert_eq!((saved.voice.as_str(), saved.voice_on), ("Short and warm.", true));
    let memory: MemoryResult = agent
        .call(
            methods::MEMORY_GET,
            MemoryParams {
                address: "bob@example.test".into(),
                account_id: None,
            },
        )
        .await
        .unwrap();
    assert_eq!(memory.voice.as_deref(), Some("Short and warm."), "agents read it");

    // Agents cannot set it, only suggest it; a suggestion waits.
    let refused = refusal(
        agent
            .call::<_, IdentityView>(methods::IDENTITY_SET_VOICE, set("Be rude.", true))
            .await,
    );
    assert_eq!(refused.code, codes::FORBIDDEN);
    let suggest = |text: &str| VoiceSuggestParams {
        address: "ADA@example.test".into(),
        account_id: None,
        text: text.into(),
    };
    let id = held(
        agent
            .call::<_, Value>(methods::VOICE_SUGGEST, suggest("French by default."))
            .await,
    );
    let hold = &waiting(&mut desktop).await[0];
    assert_eq!(
        (hold.reason, hold.description.as_str()),
        (HoldReason::Voice, "Change your voice for ada@example.test")
    );
    assert_eq!(own_identity(&mut desktop, &account.id).await.voice, "Short and warm.");
    for (text, code) in [("  ", codes::INVALID_PARAMS)] {
        assert_eq!(
            refusal(agent.call::<_, Value>(methods::VOICE_SUGGEST, suggest(text)).await).code,
            code
        );
    }
    let elsewhere = VoiceSuggestParams {
        address: "someone@else.test".into(),
        ..suggest("Hi")
    };
    assert_eq!(
        refusal(agent.call::<_, Value>(methods::VOICE_SUGGEST, elsewhere).await).code,
        codes::NOT_FOUND
    );

    // Approved as the person edited it: that is what is saved, and on.
    let approved: HoldView = desktop
        .call(
            methods::HOLD_APPROVE,
            HoldApproveParams {
                id,
                text: Some("French by default. Vous with clients.".into()),
            },
        )
        .await
        .unwrap();
    assert_eq!(approved.status, HoldStatus::Approved);
    let now = own_identity(&mut desktop, &account.id).await;
    assert_eq!(
        (now.voice.as_str(), now.voice_on),
        ("French by default. Vous with clients.", true)
    );

    // Switched off, it is kept, and agents get none.
    let _: IdentityView = desktop
        .call(methods::IDENTITY_SET_VOICE, set(&now.voice, false))
        .await
        .unwrap();
    let memory: MemoryResult = agent
        .call(
            methods::MEMORY_GET,
            MemoryParams {
                address: "bob@example.test".into(),
                account_id: Some(account.id.clone()),
            },
        )
        .await
        .unwrap();
    assert_eq!(memory.voice, None);
    handle.abort();
}

#[tokio::test]
async fn agents_keep_notes_risky_ones_wait_and_every_one_can_be_undone() {
    let world = World::new(DEFAULT_LATE_SEND_GRACE).await;
    let Running {
        endpoint,
        mut desktop,
        handle,
    } = world.start().await;
    let account = world.add_account(&mut desktop).await;
    let anna = "Anna@Client.test";
    set_access(&mut desktop, &account.id, AgentAccess::Read).await;
    let mut agent = Client::connect(&endpoint, ClientKind::Mcp).await.unwrap();

    // Reading may read notes, not add them.
    assert!(notes_about(&mut agent, &account.id, anna).await.is_empty());
    let refused = refusal(
        agent
            .call::<_, Value>(methods::NOTE_ADD, note(&account.id, anna, "Likes lists."))
            .await,
    );
    assert_eq!(
        (refused.code, refused.data.unwrap()["needed"].as_str()),
        (codes::FORBIDDEN, Some("triage"))
    );

    // Triage: the agent's note is marked as such, and can be taken back.
    set_access(&mut desktop, &account.id, AgentAccess::Triage).await;
    let mine: NoteAddResult = desktop
        .call(methods::NOTE_ADD, note(&account.id, anna, "Decides on renewals."))
        .await
        .unwrap();
    assert!(mine.undo.is_none(), "the person's own needs no undo");
    let theirs: NoteAddResult = agent
        .call(
            methods::NOTE_ADD,
            note(&account.id, anna, " Prefers calls before 10:00. "),
        )
        .await
        .unwrap();
    assert_eq!(
        (theirs.note.address.as_str(), theirs.note.by),
        ("anna@client.test", NoteAuthor::Agent)
    );
    assert_eq!(
        notes_about(&mut agent, &account.id, "anna@client.test").await,
        [
            ("Decides on renewals.".to_owned(), NoteAuthor::Person),
            ("Prefers calls before 10:00.".to_owned(), NoteAuthor::Agent),
        ]
    );
    let undone: Value = desktop
        .call(
            methods::UNDO_APPLY,
            UndoParams {
                id: theirs.undo.unwrap().id,
            },
        )
        .await
        .unwrap();
    assert_eq!(undone["undone"], 1);
    assert_eq!(notes_about(&mut desktop, &account.id, anna).await.len(), 1);

    // Agents remove only agents' notes; a removed note can be put back.
    let refused = refusal(
        agent
            .call::<_, NoteRemoved>(methods::NOTE_REMOVE, NoteRef { id: mine.note.id })
            .await,
    );
    assert_eq!(refused.code, codes::FORBIDDEN);
    let again: NoteAddResult = agent
        .call(methods::NOTE_ADD, note(&account.id, anna, "Out until Monday."))
        .await
        .unwrap();
    let removed: NoteRemoved = agent
        .call(methods::NOTE_REMOVE, NoteRef { id: again.note.id })
        .await
        .unwrap();
    let _: Value = desktop
        .call(
            methods::UNDO_APPLY,
            UndoParams {
                id: removed.undo.unwrap().id,
            },
        )
        .await
        .unwrap();
    assert_eq!(notes_about(&mut desktop, &account.id, anna).await.len(), 2, "put back");

    // A note with bank details waits; approved, it is the agent's.
    let iban = "Invoices now go to IBAN CH93 0076 2011 6238 5295 7.";
    let id = held(
        agent
            .call::<_, Value>(methods::NOTE_ADD, note(&account.id, anna, iban))
            .await,
    );
    let hold = &waiting(&mut desktop).await[0];
    assert_eq!(hold.reason, HoldReason::Note);
    assert_eq!(
        hold.description,
        "Save a note about anna@client.test. It has bank or card details in it."
    );
    assert_eq!(
        notes_about(&mut desktop, &account.id, anna).await.len(),
        2,
        "not before the person says so"
    );
    let link = held(
        agent
            .call::<_, Value>(
                methods::NOTE_ADD,
                note(&account.id, anna, "Pay at https://pay.example.test"),
            )
            .await,
    );
    let edited = HoldApproveParams {
        id: link,
        text: Some("Something else".into()),
    };
    assert_eq!(
        refusal(desktop.call::<_, HoldView>(methods::HOLD_APPROVE, edited).await).code,
        codes::INVALID_PARAMS,
        "only a voice is edited before approval"
    );
    let _: HoldView = desktop.call(methods::HOLD_REJECT, HoldRef { id: link }).await.unwrap();
    let approved: HoldView = desktop
        .call(methods::HOLD_APPROVE, HoldApproveParams { id, text: None })
        .await
        .unwrap();
    assert_eq!(approved.status, HoldStatus::Approved);
    let all = notes_about(&mut desktop, &account.id, anna).await;
    assert_eq!(all.last().unwrap(), &(iban.to_owned(), NoteAuthor::Agent));

    // Switched off: kept, but agents get none and add none.
    let settings: NoteSettings = desktop
        .call(methods::NOTE_SET_ENABLED, NoteSetEnabledParams { on: false })
        .await
        .unwrap();
    assert_eq!(
        settings,
        NoteSettings {
            on: false,
            notes: 3,
            people: 1
        }
    );
    let refused = refusal(
        agent
            .call::<_, NoteListResult>(
                methods::NOTE_LIST,
                NoteListParams {
                    account_id: account.id.clone(),
                    address: anna.into(),
                },
            )
            .await,
    );
    assert_eq!(refused.code, codes::FORBIDDEN);
    assert!(refused.message.contains("Notes about people"), "names the setting");
    let memory: MemoryResult = agent
        .call(
            methods::MEMORY_GET,
            MemoryParams {
                address: anna.into(),
                account_id: None,
            },
        )
        .await
        .unwrap();
    assert!(memory.person.notes.is_empty());

    // Agents cannot switch them on, or forget them; the person can.
    assert_eq!(
        refusal(
            agent
                .call::<_, NoteSettings>(methods::NOTE_SET_ENABLED, NoteSetEnabledParams { on: true })
                .await
        )
        .code,
        codes::FORBIDDEN
    );
    let forgotten: NotesForgotten = desktop.call(methods::NOTE_FORGET_ALL, Value::Null).await.unwrap();
    assert_eq!(forgotten.forgotten, 3);
    handle.abort();
}

#[tokio::test]
async fn an_agents_draft_comes_with_the_voice_and_what_is_known_of_its_people() {
    let world = World::new(DEFAULT_LATE_SEND_GRACE).await;
    let Running {
        endpoint,
        mut desktop,
        handle,
    } = world.start().await;
    let account = world.add_account(&mut desktop).await;
    set_access(&mut desktop, &account.id, AgentAccess::Triage).await;
    let own = own_identity(&mut desktop, &account.id).await;
    let _: IdentityView = desktop
        .call(
            methods::IDENTITY_SET_VOICE,
            IdentitySetVoiceParams {
                id: own.id,
                voice: "Short and warm.".into(),
                on: true,
            },
        )
        .await
        .unwrap();
    let _: NoteAddResult = desktop
        .call(methods::NOTE_ADD, note(&account.id, "bob@example.test", "Likes lists."))
        .await
        .unwrap();
    let draft = DraftCreateParams {
        account_id: account.id.clone(),
        to: vec![
            Person {
                name: Some("Bob".into()),
                address: Some("Bob@Example.test".into()),
            },
            // The person's own address is not one of "the people".
            Person {
                name: None,
                address: Some("ada@example.test".into()),
            },
        ],
        subject: "Plans".into(),
        ..DraftCreateParams::default()
    };

    let mut agent = Client::connect(&endpoint, ClientKind::Mcp).await.unwrap();
    let made: Value = agent.call(methods::DRAFT_CREATE, draft.clone()).await.unwrap();
    let context: AgentContext = serde_json::from_value(made["context"].clone()).unwrap();
    assert_eq!(context.voice.as_deref(), Some("Short and warm."));
    assert_eq!(context.people.len(), 1);
    let bob = &context.people[0];
    assert_eq!(
        (bob.address.as_str(), bob.name.as_deref()),
        ("bob@example.test", Some("Bob"))
    );
    assert_eq!(
        bob.notes.iter().map(|n| n.text.as_str()).collect::<Vec<_>>(),
        ["Likes lists."]
    );

    // The app's own results carry none.
    let mine: Value = desktop.call(methods::DRAFT_CREATE, draft).await.unwrap();
    assert!(mine.get("context").is_none());
    handle.abort();
}
