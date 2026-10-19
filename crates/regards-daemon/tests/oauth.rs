//! Signing in with a provider end to end (ADR-0018), against a fake token
//! endpoint and fake IMAP and SMTP servers that take XOAUTH2: the browser
//! flow adds the account; IMAP and SMTP use its access token; a restart
//! renews it with the refresh token; a withdrawn sign-in asks to sign in
//! again, which keeps the account.

#[path = "../../regards-imap/tests/support/fake_imap.rs"]
mod fake_imap;
#[path = "support/fake_oauth.rs"]
mod fake_oauth;
#[path = "../../regards-smtp/tests/support/fake_smtp.rs"]
mod fake_smtp;
#[path = "support/world.rs"]
mod world;

use fake_oauth::TokenServer;
use regards_core::headers::Person;
use regards_core::oauth::{Client as OauthClient, Provider};
use regards_daemon::{DEFAULT_LATE_SEND_GRACE, OauthApp};
use regards_ipc::protocol::{
    AccountRef, AccountSummary, ClientKind, ConnectionSecurity, DraftCreateParams, DraftSendParams, DraftView,
    OauthFlowRef, OauthProvidersResult, OauthStartParams, OauthStartResult, OutboxEntryView, OutboxListResult,
    OutboxState, ServerSettings, SignIn, SyncResult, codes, methods,
};
use regards_ipc::{Client, ClientError};
use regards_net::loopback::parse_query;
use regards_vault::{SecretKey, SecretKind};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use world::{Running, World};

const EMAIL: &str = "ada@example.test";

async fn world() -> (World, TokenServer) {
    let mut world = World::new(DEFAULT_LATE_SEND_GRACE).await;
    let tokens = fake_oauth::serve(fake_imap::PASSWORD, EMAIL).await;
    world.tls = world
        .tls
        .clone()
        .with_extra_roots_pem(tokens.ca_pem.as_bytes())
        .unwrap();
    let mut client = OauthClient::new(Provider::Microsoft, "test-client", None);
    client.authorize_endpoint = "https://login.example.test/authorize".into();
    client.token_endpoint = tokens.url();
    world.oauth_apps = vec![OauthApp {
        client,
        imap: ServerSettings {
            host: "localhost".into(),
            port: world.imap_port,
            security: ConnectionSecurity::Tls,
        },
        smtp: ServerSettings {
            host: "127.0.0.1".into(),
            port: world.smtp_port,
            security: ConnectionSecurity::Starttls,
        },
    }];
    (world, tokens)
}

/// The browser asking for the redirect page with this query.
async fn browser(port: u16, query: &str) -> String {
    let mut stream = tokio::net::TcpStream::connect(("127.0.0.1", port)).await.unwrap();
    stream
        .write_all(format!("GET /?{query} HTTP/1.1\r\nHost: localhost:{port}\r\n\r\n").as_bytes())
        .await
        .unwrap();
    let mut page = String::new();
    stream.read_to_string(&mut page).await.unwrap();
    page
}

/// A started sign-in, with what its page asks of the provider.
struct Started {
    flow_id: String,
    port: u16,
    query: HashMap<String, String>,
}

async fn start(desktop: &mut Client) -> Result<Started, ClientError> {
    let started: OauthStartResult = desktop
        .call(
            methods::ACCOUNT_OAUTH_START,
            OauthStartParams {
                provider: Provider::Microsoft,
                email: Some(EMAIL.into()),
            },
        )
        .await?;
    let (page, query) = started.url.split_once('?').unwrap();
    assert_eq!(page, "https://login.example.test/authorize");
    let query: HashMap<String, String> = parse_query(query).into_iter().collect();
    let port = query["redirect_uri"]
        .strip_prefix("http://localhost:")
        .and_then(|rest| rest.strip_suffix('/'))
        .unwrap()
        .parse()
        .unwrap();
    Ok(Started {
        flow_id: started.flow_id,
        port,
        query,
    })
}

async fn finish(desktop: &mut Client, flow_id: &str) -> Result<AccountSummary, ClientError> {
    desktop
        .call(
            methods::ACCOUNT_OAUTH_FINISH,
            OauthFlowRef {
                flow_id: flow_id.into(),
            },
        )
        .await
}

/// The person signs in at the provider, which sends the browser back with a code.
async fn sign_in(desktop: &mut Client, tokens: &TokenServer) -> Result<AccountSummary, ClientError> {
    let started = start(desktop).await.unwrap();
    assert_eq!(started.query["client_id"], "test-client");
    assert_eq!(started.query["login_hint"], EMAIL);
    assert_eq!(started.query["code_challenge_method"], "S256");
    tokens
        .provider
        .lock()
        .unwrap()
        .codes
        .insert("code-1".into(), started.query["code_challenge"].clone());
    // Something else on this computer cannot end the sign-in.
    let forged = browser(started.port, "code=code-1&state=guessed").await;
    assert!(forged.starts_with("HTTP/1.1 400"), "{forged}");
    let page = browser(started.port, &format!("code=code-1&state={}", started.query["state"])).await;
    assert!(page.starts_with("HTTP/1.1 200") && page.contains("go back to Regards"));
    finish(desktop, &started.flow_id).await
}

fn reason(error: &ClientError) -> (i32, Value) {
    match error {
        ClientError::Rpc(error) => (
            error.code,
            error
                .data
                .as_ref()
                .map(|data| data["reason"].clone())
                .unwrap_or_default(),
        ),
        other => panic!("{other:?}"),
    }
}

/// A sync of its own, once the one the daemon started with is over.
async fn sync(desktop: &mut Client, account: &AccountSummary) -> Result<SyncResult, ClientError> {
    for _ in 0..300 {
        let result: SyncResult = desktop
            .call(
                methods::ACCOUNT_SYNC,
                AccountRef {
                    account_id: account.id.clone(),
                },
            )
            .await?;
        if !result.already_running {
            return Ok(result);
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("the sync never ended");
}

/// Write a message and send it at once.
async fn send(desktop: &mut Client, account: &AccountSummary, subject: &str) {
    let draft: DraftView = desktop
        .call(
            methods::DRAFT_CREATE,
            DraftCreateParams {
                account_id: account.id.clone(),
                to: vec![Person {
                    name: None,
                    address: Some("bob@example.test".into()),
                }],
                subject: subject.into(),
                body: "Hi".into(),
                ..DraftCreateParams::default()
            },
        )
        .await
        .unwrap();
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
}

/// The next SMTP session that handed over a message.
async fn sent(world: &mut World) -> fake_smtp::Session {
    tokio::time::timeout(Duration::from_secs(15), async {
        loop {
            let session = world.sent.next().await;
            if session.data.is_some() {
                break session;
            }
        }
    })
    .await
    .expect("nothing was sent")
}

async fn outbox(desktop: &mut Client) -> Vec<OutboxEntryView> {
    desktop
        .call::<_, OutboxListResult>(methods::OUTBOX_LIST, Value::Null)
        .await
        .unwrap()
        .entries
}

async fn refresh_token(world: &World, account: &AccountSummary) -> Option<String> {
    world
        .vault
        .get(&SecretKey::new(&account.id, SecretKind::OauthRefresh))
        .await
        .unwrap()
        .map(|secret| secret.expose().to_owned())
}

async fn accounts(desktop: &mut Client) -> Vec<AccountSummary> {
    desktop.call(methods::ACCOUNT_LIST, Value::Null).await.unwrap()
}

#[tokio::test]
async fn signing_in_adds_an_account_that_syncs_and_sends_with_its_tokens() {
    let (mut world, tokens) = world().await;
    let Running {
        mut desktop, handle, ..
    } = world.start().await;
    let offered: OauthProvidersResult = desktop
        .call(methods::ACCOUNT_OAUTH_PROVIDERS, Value::Null)
        .await
        .unwrap();
    assert_eq!(offered.providers, [Provider::Microsoft]);

    let account = sign_in(&mut desktop, &tokens).await.unwrap();
    assert_eq!(
        (
            account.email.as_str(),
            account.username.as_str(),
            account.display_name.as_str()
        ),
        (EMAIL, EMAIL, "Ada Lovelace")
    );
    assert_eq!(account.sign_in, SignIn::Microsoft);
    assert_eq!(
        (account.imap.port, account.smtp.port),
        (world.imap_port, world.smtp_port)
    );
    assert_eq!(refresh_token(&world, &account).await.as_deref(), Some("refresh-1"));
    {
        let provider = tokens.provider.lock().unwrap();
        let form = &provider.requests[0];
        assert_eq!(form["grant_type"], "authorization_code");
        assert_eq!(form["client_id"], "test-client");
        assert!(!form.contains_key("client_secret"));
    }

    // IMAP signs in with the access token kept from the sign-in.
    sync(&mut desktop, &account).await.unwrap();
    assert_eq!(tokens.grants(), ["authorization_code"]);

    // So does SMTP.
    send(&mut desktop, &account, "Signed in").await;
    let session = sent(&mut world).await;
    assert_eq!(session.login, Some((EMAIL.into(), fake_imap::PASSWORD.into())));
    assert!(session.commands.iter().any(|c| c == "AUTH XOAUTH2 <secret>"));

    // After a restart the token is gone from memory: the refresh token gets
    // a new one, and the refresh token it came with is kept.
    desktop.shutdown().await.unwrap();
    handle.await.unwrap();
    let Running {
        mut desktop, handle, ..
    } = world.start().await;
    sync(&mut desktop, &account).await.unwrap();
    assert_eq!(tokens.grants(), ["authorization_code", "refresh_token"]);
    {
        let provider = tokens.provider.lock().unwrap();
        let form = &provider.requests[1];
        assert_eq!(form["refresh_token"], "refresh-1");
        assert!(form["scope"].contains("IMAP.AccessAsUser.All"));
    }
    assert_eq!(refresh_token(&world, &account).await.as_deref(), Some("refresh-2"));
    handle.abort();
}

#[tokio::test]
async fn a_withdrawn_sign_in_asks_to_sign_in_again_which_keeps_the_account() {
    let (mut world, tokens) = world().await;
    let first = world.start().await;
    let mut desktop = first.desktop;
    let account = sign_in(&mut desktop, &tokens).await.unwrap();
    desktop.shutdown().await.unwrap();
    first.handle.await.unwrap();

    // Withdrawn at the provider while Regards was closed.
    tokens.provider.lock().unwrap().refresh_tokens.clear();
    let Running {
        mut desktop, handle, ..
    } = world.start().await;
    let error = sync(&mut desktop, &account).await.unwrap_err();
    assert_eq!(reason(&error).0, codes::SYNC_FAILED);
    assert!(error.to_string().contains("sign in again"), "{error}");
    let listed = accounts(&mut desktop).await;
    assert!(listed[0].needs_sign_in);
    let asked = tokens.grants().len();
    assert!(sync(&mut desktop, &account).await.is_err());
    assert_eq!(
        tokens.grants().len(),
        asked,
        "a refused refresh token is not tried again"
    );

    // A message sent meanwhile waits in the outbox, as when offline.
    send(&mut desktop, &account, "Written while signed out").await;
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    let waiting = loop {
        let entries = outbox(&mut desktop).await;
        // Tried, and back to waiting (not still being tried).
        let tried = |entry: &OutboxEntryView| entry.attempts > 0 && entry.state == OutboxState::Queued;
        if entries.first().is_some_and(tried) || std::time::Instant::now() > deadline {
            break entries;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    };
    assert_eq!(waiting[0].state, OutboxState::Queued);
    assert!(
        waiting[0]
            .last_error
            .as_deref()
            .is_some_and(|error| error.contains("sign in again")),
        "{waiting:?}"
    );
    assert!(world.nothing_sent());

    let again = sign_in(&mut desktop, &tokens).await.unwrap();
    assert_eq!(again.id, account.id, "the same account, signed in anew");
    assert!(!again.needs_sign_in);
    // It goes at once, not at its next retry.
    let session = sent(&mut world).await;
    assert!(String::from_utf8_lossy(&session.data.unwrap()).contains("Written while signed out"));
    sync(&mut desktop, &account).await.unwrap();
    let listed = accounts(&mut desktop).await;
    assert_eq!(listed.len(), 1);
    assert!(!listed[0].needs_sign_in);
    handle.abort();
}

#[tokio::test]
async fn the_person_can_say_no_or_give_up() {
    let (world, tokens) = world().await;
    let Running {
        mut desktop,
        endpoint,
        handle,
    } = world.start().await;

    // No at the provider.
    let started = start(&mut desktop).await.unwrap();
    browser(
        started.port,
        &format!("error=access_denied&state={}", started.query["state"]),
    )
    .await;
    let error = finish(&mut desktop, &started.flow_id).await.unwrap_err();
    assert_eq!(reason(&error), (codes::OAUTH_FAILED, json!("cancelled")));

    // Given up before the browser came back, and while waiting for it.
    let started = start(&mut desktop).await.unwrap();
    let cancelled: Value = desktop
        .call(
            methods::ACCOUNT_OAUTH_CANCEL,
            OauthFlowRef {
                flow_id: started.flow_id.clone(),
            },
        )
        .await
        .unwrap();
    assert_eq!(cancelled, json!({ "cancelled": true }));
    let error = finish(&mut desktop, &started.flow_id).await.unwrap_err();
    assert_eq!(reason(&error).0, codes::NOT_FOUND);

    let started = start(&mut desktop).await.unwrap();
    let mut waiting = Client::connect_with(&endpoint, ClientKind::Desktop, Some(&world.token))
        .await
        .unwrap();
    let flow_id = started.flow_id.clone();
    let waited = tokio::spawn(async move { finish(&mut waiting, &flow_id).await });
    tokio::time::sleep(Duration::from_millis(200)).await;
    let _: Value = desktop
        .call(
            methods::ACCOUNT_OAUTH_CANCEL,
            OauthFlowRef {
                flow_id: started.flow_id,
            },
        )
        .await
        .unwrap();
    let error = waited.await.unwrap().unwrap_err();
    assert_eq!(reason(&error), (codes::OAUTH_FAILED, json!("cancelled")));

    assert!(accounts(&mut desktop).await.is_empty());
    assert_eq!(tokens.grants().len(), 0, "no code was ever traded");
    handle.abort();
}

#[tokio::test]
async fn only_the_app_signs_in_and_only_where_an_app_is_registered() {
    let (world, tokens) = world().await;
    let Running {
        mut desktop,
        endpoint,
        handle,
    } = world.start().await;

    let mut agent = Client::connect(&endpoint, ClientKind::Mcp).await.unwrap();
    let error = start(&mut agent).await.err().unwrap();
    assert_eq!(reason(&error).0, codes::FORBIDDEN);

    // A password account for the address is not taken over.
    world.add_account(&mut desktop).await;
    let error = sign_in(&mut desktop, &tokens).await.unwrap_err();
    assert_eq!(reason(&error).0, codes::CONFLICT);
    handle.abort();

    let bare = World::new(DEFAULT_LATE_SEND_GRACE).await;
    let Running {
        mut desktop, handle, ..
    } = bare.start().await;
    let offered: OauthProvidersResult = desktop
        .call(methods::ACCOUNT_OAUTH_PROVIDERS, Value::Null)
        .await
        .unwrap();
    assert!(offered.providers.is_empty());
    let error = start(&mut desktop).await.err().unwrap();
    assert_eq!(reason(&error), (codes::OAUTH_FAILED, json!("not_available")));
    handle.abort();
}
