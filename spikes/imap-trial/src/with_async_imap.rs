//! The trial scenario with async-imap (high-level, string-command API).

use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use async_imap::{
    Client, Session,
    extensions::idle::IdleResponse,
    imap_proto::{Response, ResponseCode},
    types::UnsolicitedResponse,
};
use futures::TryStreamExt;
use tokio::{net::TcpStream, time::sleep};
use tokio_rustls::client::TlsStream;

use crate::common::{Config, Report, WANTED_CAPS, connect_tls, message, tls_version};

type S = Session<TlsStream<TcpStream>>;

async fn login(cfg: &Config, user: &str) -> Result<(S, String)> {
    let tls = connect_tls(cfg).await?;
    let version = tls_version(&tls);
    let mut client = Client::new(tls);
    client.read_response().await?.context("no greeting")?;
    let session = client.login(user, &cfg.pass).await.map_err(|(e, _)| e)?;
    Ok((session, version))
}

/// Everything the library routed to the unsolicited channel since the last drain.
fn drain(s: &S) -> Vec<UnsolicitedResponse> {
    std::iter::from_fn(|| s.unsolicited_responses.try_recv().ok()).collect()
}

fn count_other(items: &[UnsolicitedResponse], pred: impl Fn(&Response<'_>) -> bool) -> usize {
    items
        .iter()
        .filter(|u| matches!(u, UnsolicitedResponse::Other(d) if pred(d.parsed())))
        .count()
}

fn has_code(items: &[UnsolicitedResponse], pred: impl Fn(&ResponseCode<'_>) -> bool) -> bool {
    items.iter().any(|u| match u {
        UnsolicitedResponse::Other(d) => match d.parsed() {
            Response::Data { outcome, .. } | Response::Done { outcome, .. } => outcome.code.as_ref().is_some_and(&pred),
            _ => false,
        },
        _ => false,
    })
}

pub async fn run(cfg: &Config, r: &mut Report) -> Result<()> {
    let user = cfg.user("ai");

    let t = Instant::now();
    let (mut s, version) = login(cfg, &user).await?;
    r.pass("TLS + greeting + LOGIN", t, version);

    let t = Instant::now();
    let caps = s.capabilities().await?;
    let missing: Vec<_> = WANTED_CAPS.iter().filter(|c| !caps.has_str(c)).collect();
    r.check(
        "CAPABILITY",
        t,
        missing.is_empty(),
        format!("{} capabilities, missing {missing:?}", caps.len()),
    );

    let t = Instant::now();
    s.run_command_and_check_ok("ENABLE QRESYNC").await?;
    let seen = drain(&s);
    let enabled = count_other(&seen, |p| matches!(p, Response::Capabilities(_)));
    r.pass(
        "ENABLE QRESYNC",
        t,
        format!("no typed API: raw command string; ENABLED reply typed: {}", enabled > 0),
    );

    let t = Instant::now();
    s.create("Archive").await?;
    r.pass("CREATE Archive", t, "");

    let t = Instant::now();
    for i in 1..=3 {
        s.append("INBOX", Some("(\\Flagged)"), None, message(i)).await?;
    }
    r.pass("APPEND x3", t, "flags are passed as raw protocol text");
    let seen = drain(&s);
    if has_code(&seen, |c| matches!(c, ResponseCode::AppendUid(..))) {
        r.pass("APPENDUID visible", Instant::now(), "via unsolicited channel");
    } else {
        r.gap(
            "APPENDUID visible",
            "append() returns (); the tagged OK code is consumed internally",
        );
    }

    let t = Instant::now();
    let mbox = s.select_condstore("INBOX").await?;
    let uidvalidity = mbox.uid_validity.context("no UIDVALIDITY")?;
    r.check(
        "SELECT (CONDSTORE)",
        t,
        mbox.exists == 3 && mbox.uid_next.is_some(),
        format!(
            "exists={} uidvalidity={uidvalidity} uidnext={:?} highestmodseq={:?}",
            mbox.exists, mbox.uid_next, mbox.highest_modseq
        ),
    );

    let t = Instant::now();
    let fetches: Vec<_> = s
        .uid_fetch("1:*", "(UID FLAGS ENVELOPE BODYSTRUCTURE MODSEQ)")
        .await?
        .try_collect()
        .await?;
    let typed = fetches
        .iter()
        .filter(|f| f.uid.is_some() && f.modseq.is_some() && f.envelope().is_some() && f.bodystructure().is_some())
        .count();
    r.check(
        "UID FETCH metadata",
        t,
        typed == 3,
        format!(
            "{} fetched, {typed} with uid+modseq+envelope+bodystructure",
            fetches.len()
        ),
    );
    // Dovecot omits HIGHESTMODSEQ on the first CONDSTORE SELECT of a new mailbox.
    let hms = match mbox.highest_modseq {
        Some(h) => h,
        None => fetches
            .iter()
            .filter_map(|f| f.modseq)
            .max()
            .context("no MODSEQ at all")?,
    };

    let t = Instant::now();
    let stored: Vec<_> = s.uid_store("1", "+FLAGS (\\Seen)").await?.try_collect().await?;
    r.check(
        "UID STORE +FLAGS",
        t,
        stored.len() == 1 && stored[0].modseq.is_some(),
        format!(
            "{} FETCH back, modseq {:?}",
            stored.len(),
            stored.first().and_then(|f| f.modseq)
        ),
    );

    let t = Instant::now();
    let changed: Vec<_> = s
        .uid_fetch("1:*", format!("(UID FLAGS) (CHANGEDSINCE {hms})"))
        .await?
        .try_collect()
        .await?;
    r.check(
        "FETCH CHANGEDSINCE",
        t,
        changed.len() == 1,
        format!("{} changed (expected 1); modifier as raw text", changed.len()),
    );

    drain(&s);
    let t = Instant::now();
    s.uid_mv("2", "Archive").await?;
    let seen = drain(&s);
    let vanished = count_other(&seen, |p| matches!(p, Response::Vanished { .. }));
    r.check(
        "UID MOVE",
        t,
        vanished == 1,
        format!("VANISHED seen {vanished}x (as untyped 'Other')"),
    );
    if has_code(&seen, |c| matches!(c, ResponseCode::CopyUid(..))) {
        r.pass(
            "COPYUID visible (MOVE)",
            Instant::now(),
            "via unsolicited channel (untagged OK)",
        );
    } else {
        r.gap("COPYUID visible (MOVE)", "uid_mv() returns ()");
    }

    // The path for servers without MOVE (Infomaniak before login shows none).
    let t = Instant::now();
    s.uid_copy("3", "Archive").await?;
    s.uid_store("3", "+FLAGS.SILENT (\\Deleted)")
        .await?
        .try_collect::<Vec<_>>()
        .await?;
    let expunged: Vec<_> = s.uid_expunge("3").await?.try_collect().await?;
    let seen = drain(&s);
    let vanished = count_other(&seen, |p| matches!(p, Response::Vanished { .. }));
    r.check(
        "COPY + \\Deleted + UID EXPUNGE",
        t,
        true,
        format!(
            "expunge stream {} item(s); VANISHED via channel {vanished}x; COPYUID not returned by uid_copy()",
            expunged.len()
        ),
    );

    bulk_resync(&mut s, r, uidvalidity).await?;
    let s = idle_push(cfg, s, &user, r).await?;
    let mut s = idle_cancel(s, r).await?;

    let t = Instant::now();
    s.logout().await?;
    r.pass("LOGOUT", t, "");
    Ok(())
}

/// QRESYNC resync after many changes: every changed message must reach the caller.
async fn bulk_resync(s: &mut S, r: &mut Report, uidvalidity: u32) -> Result<()> {
    const N: usize = 150;
    let t = Instant::now();
    for i in 0..N {
        s.append("INBOX", None, None, message(100 + i)).await?;
    }
    r.pass("APPEND x150 (sequential)", t, "one round trip per message");

    let mbox = s.select_condstore("INBOX").await?;
    let before = mbox.highest_modseq.context("no HIGHESTMODSEQ")?;
    s.uid_store("1:*", "+FLAGS.SILENT (\\Answered)")
        .await?
        .try_collect::<Vec<_>>()
        .await?;
    drain(s);

    let t = Instant::now();
    s.run_command_and_check_ok(format!("SELECT INBOX (QRESYNC ({uidvalidity} {before}))"))
        .await?;
    let seen = drain(s);
    let fetches = count_other(&seen, |p| matches!(p, Response::Fetch(..)));
    r.check(
        "QRESYNC SELECT after bulk change",
        t,
        fetches == mbox.exists as usize,
        format!("{fetches} of {} changes reached the caller (raw command; unsolicited channel is bounded at 100, overflow dropped)", mbox.exists),
    );
    Ok(())
}

async fn idle_push(cfg: &Config, s: S, user: &str, r: &mut Report) -> Result<S> {
    let mut idle = s.idle();
    idle.init().await?;

    let (second, _) = login(cfg, user).await?;
    let trigger = tokio::spawn(async move {
        let mut second = second;
        sleep(Duration::from_millis(300)).await;
        let sent = Instant::now();
        second.append("INBOX", None, None, message(999)).await?;
        second.logout().await?;
        anyhow::Ok(sent)
    });

    let (wait, _stop) = idle.wait_with_timeout(Duration::from_secs(10));
    let response = wait.await?;
    let seen = Instant::now();
    let sent = trigger.await??;
    match response {
        IdleResponse::NewData(_) => r.pass(
            "IDLE push",
            sent,
            format!(
                "EXISTS {:.1} ms after the other client's APPEND started",
                (seen - sent).as_secs_f64() * 1000.0
            ),
        ),
        other => bail!("IDLE ended without data: {other:?}"),
    }
    Ok(idle.done().await?)
}

async fn idle_cancel(s: S, r: &mut Report) -> Result<S> {
    let mut idle = s.idle();
    idle.init().await?;
    let t = Instant::now();
    let (wait, stop) = idle.wait_with_timeout(Duration::from_secs(60));
    let stopper = tokio::spawn(async move {
        sleep(Duration::from_millis(200)).await;
        drop(stop);
    });
    let response = wait.await?;
    stopper.await?;
    let mut s = idle.done().await?;
    s.noop().await?;
    r.check(
        "IDLE cancel + DONE + NOOP",
        t,
        matches!(response, IdleResponse::ManualInterrupt),
        format!("{response:?}; cancel requested at 200 ms"),
    );
    Ok(s)
}
