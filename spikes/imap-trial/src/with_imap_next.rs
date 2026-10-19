//! The trial scenario with imap-next + imap-codec (typed, sans-I/O protocol state machine).

use std::{
    num::{NonZeroU32, NonZeroU64},
    time::{Duration, Instant},
};

use anyhow::{Context, Result, bail};
use imap_next::{
    client::{Client, Event, Options},
    imap_types::{
        command::{Command, CommandBody, FetchModifier, SelectParameter},
        core::{Literal, Tag, TagGenerator, Vec1},
        extensions::binary::LiteralOrLiteral8,
        extensions::enable::CapabilityEnable,
        fetch::{MessageDataItem, MessageDataItemName},
        flag::{Flag, StoreResponse, StoreType},
        mailbox::Mailbox,
        response::{Code, Data, Status, StatusBody, StatusKind, Tagged},
        sequence::SequenceSet,
    },
    stream::Stream,
};
use tokio::{select, time::sleep};

use crate::common::{Config, Report, WANTED_CAPS, connect_tls, message, tls_version};

pub struct Reply {
    pub data: Vec<Data<'static>>,
    pub untagged: Vec<StatusBody<'static>>,
    pub done: StatusBody<'static>,
}

impl Reply {
    fn codes(&self) -> impl Iterator<Item = &Code<'static>> {
        self.untagged.iter().chain([&self.done]).filter_map(|s| s.code.as_ref())
    }
}

pub struct Conn {
    pub stream: Stream,
    pub client: Client,
    tags: TagGenerator,
}

impl Conn {
    /// Wraps a stream and waits for the server greeting.
    pub async fn open(mut stream: Stream) -> Result<Self> {
        let mut client = Client::new(Options::default());
        loop {
            if let Event::GreetingReceived { .. } = stream.next(&mut client).await? {
                break;
            }
        }
        Ok(Self {
            stream,
            client,
            tags: TagGenerator::new(),
        })
    }

    pub fn enqueue(&mut self, body: CommandBody<'static>) -> Tag<'static> {
        let tag = self.tags.generate();
        self.client.enqueue_command(Command { tag: tag.clone(), body });
        tag
    }

    /// Drives the connection until the tagged reply for `tag`.
    pub async fn finish(&mut self, tag: &Tag<'static>) -> Result<Reply> {
        let (mut data, mut untagged) = (Vec::new(), Vec::new());
        loop {
            match self.stream.next(&mut self.client).await? {
                Event::DataReceived { data: d } => data.push(d),
                Event::StatusReceived {
                    status: Status::Untagged(body),
                } => untagged.push(body),
                Event::StatusReceived {
                    status: Status::Tagged(Tagged { tag: t, body }),
                } if &t == tag => {
                    return Ok(Reply {
                        data,
                        untagged,
                        done: body,
                    });
                }
                // BYE precedes the tagged OK of LOGOUT; an unexpected one ends the stream next.
                Event::StatusReceived { status: Status::Bye(_) } => {}
                Event::CommandRejected { command, status, .. } if &command.tag == tag => {
                    bail!("command rejected: {status:?}")
                }
                _ => {}
            }
        }
    }

    pub async fn run(&mut self, body: CommandBody<'static>) -> Result<Reply> {
        let tag = self.enqueue(body);
        self.finish(&tag).await
    }

    pub async fn ok(&mut self, body: CommandBody<'static>) -> Result<Reply> {
        let reply = self.run(body).await?;
        if reply.done.kind != StatusKind::Ok {
            bail!("{:?}: {:?}", reply.done.kind, reply.done.text);
        }
        Ok(reply)
    }
}

async fn login(cfg: &Config, user: &str) -> Result<(Conn, String)> {
    let tls = connect_tls(cfg).await?;
    let version = tls_version(&tls);
    let mut conn = Conn::open(Stream::tls(tls.into())).await?;
    conn.ok(CommandBody::login(user.to_owned(), cfg.pass.clone())?).await?;
    Ok((conn, version))
}

fn set(s: &str) -> Result<SequenceSet> {
    SequenceSet::try_from(s).map_err(|e| anyhow::anyhow!("sequence set {s}: {e:?}"))
}

fn archive() -> Mailbox<'static> {
    Mailbox::try_from("Archive").expect("valid mailbox name")
}

fn append(i: usize, flags: Vec<Flag<'static>>, non_sync: bool) -> Result<CommandBody<'static>> {
    let mut literal = Literal::try_from(message(i))?;
    if non_sync {
        literal = literal.into_non_sync();
    }
    Ok(CommandBody::Append {
        mailbox: Mailbox::Inbox,
        flags,
        date: None,
        message: LiteralOrLiteral8::Literal(literal),
    })
}

fn fetch(
    range: &str,
    items: Vec<MessageDataItemName<'static>>,
    modifiers: Vec<FetchModifier>,
) -> Result<CommandBody<'static>> {
    Ok(CommandBody::Fetch {
        sequence_set: set(range)?,
        macro_or_item_names: items.into(),
        uid: true,
        modifiers,
    })
}

fn store(range: &str, flags: Vec<Flag<'static>>, response: StoreResponse) -> Result<CommandBody<'static>> {
    Ok(CommandBody::Store {
        sequence_set: set(range)?,
        kind: StoreType::Add,
        response,
        flags,
        uid: true,
        modifiers: vec![],
    })
}

fn count_fetch(reply: &Reply) -> usize {
    reply.data.iter().filter(|d| matches!(d, Data::Fetch { .. })).count()
}

fn count_vanished(reply: &Reply) -> usize {
    reply.data.iter().filter(|d| matches!(d, Data::Vanished { .. })).count()
}

pub async fn run(cfg: &Config, r: &mut Report) -> Result<()> {
    let user = cfg.user("in");

    let t = Instant::now();
    let (mut c, version) = login(cfg, &user).await?;
    r.pass("TLS + greeting + LOGIN", t, version);

    let t = Instant::now();
    let reply = c.ok(CommandBody::Capability).await?;
    let caps: Vec<String> = reply
        .data
        .iter()
        .find_map(|d| {
            if let Data::Capability(c) = d {
                Some(c.as_ref().iter().map(|c| c.to_string()).collect())
            } else {
                None
            }
        })
        .context("no CAPABILITY data")?;
    let missing: Vec<_> = WANTED_CAPS
        .iter()
        .filter(|w| !caps.iter().any(|c| c.eq_ignore_ascii_case(w)))
        .collect();
    r.check(
        "CAPABILITY",
        t,
        missing.is_empty(),
        format!("{} capabilities, missing {missing:?}", caps.len()),
    );

    let t = Instant::now();
    let qresync = CapabilityEnable::try_from("QRESYNC").map_err(|e| anyhow::anyhow!("{e:?}"))?;
    let reply = c
        .ok(CommandBody::Enable {
            capabilities: Vec1::from(qresync),
        })
        .await?;
    let enabled = reply
        .data
        .iter()
        .any(|d| matches!(d, Data::Enabled { capabilities } if !capabilities.is_empty()));
    r.check(
        "ENABLE QRESYNC",
        t,
        enabled,
        "typed ENABLE; QRESYNC has no named variant (uses Other)",
    );

    let t = Instant::now();
    c.ok(CommandBody::create("Archive")?).await?;
    r.pass("CREATE Archive", t, "");

    let t = Instant::now();
    let mut appenduids = 0;
    for i in 1..=3 {
        let reply = c.ok(append(i, vec![Flag::Flagged], false)?).await?;
        appenduids += reply.codes().filter(|c| matches!(c, Code::AppendUid { .. })).count();
    }
    r.pass("APPEND x3", t, "typed flags and literal");
    r.check(
        "APPENDUID visible",
        Instant::now(),
        appenduids == 3,
        format!("{appenduids}/3 typed Code::AppendUid"),
    );

    let t = Instant::now();
    let reply = c
        .ok(CommandBody::Select {
            mailbox: Mailbox::Inbox,
            parameters: vec![SelectParameter::CondStore],
        })
        .await?;
    let exists = reply
        .data
        .iter()
        .find_map(|d| if let Data::Exists(n) = d { Some(*n) } else { None });
    let uidvalidity = reply
        .codes()
        .find_map(|c| if let Code::UidValidity(v) = c { Some(*v) } else { None })
        .context("no UIDVALIDITY")?;
    let uidnext = reply
        .codes()
        .find_map(|c| if let Code::UidNext(v) = c { Some(*v) } else { None });
    let selected_hms = reply
        .codes()
        .find_map(|c| if let Code::HighestModSeq(v) = c { Some(*v) } else { None });
    r.check(
        "SELECT (CONDSTORE)",
        t,
        exists == Some(3) && uidnext.is_some(),
        format!("exists={exists:?} uidvalidity={uidvalidity} uidnext={uidnext:?} highestmodseq={selected_hms:?}"),
    );

    let t = Instant::now();
    use MessageDataItemName as N;
    let reply = c
        .ok(fetch(
            "1:*",
            vec![N::Uid, N::Flags, N::Envelope, N::BodyStructure, N::ModSeq],
            vec![],
        )?)
        .await?;
    let typed = reply
        .data
        .iter()
        .filter(|d| match d {
            Data::Fetch { items, .. } => {
                let has = |f: fn(&MessageDataItem<'_>) -> bool| items.as_ref().iter().any(f);
                has(|i| matches!(i, MessageDataItem::Uid(_)))
                    && has(|i| matches!(i, MessageDataItem::ModSeq(_)))
                    && has(|i| matches!(i, MessageDataItem::Envelope(_)))
                    && has(|i| matches!(i, MessageDataItem::BodyStructure(_)))
            }
            _ => false,
        })
        .count();
    r.check(
        "UID FETCH metadata",
        t,
        typed == 3,
        format!(
            "{} fetched, {typed} with uid+modseq+envelope+bodystructure",
            count_fetch(&reply)
        ),
    );
    // Dovecot omits HIGHESTMODSEQ on the first CONDSTORE SELECT of a new mailbox.
    let max_modseq = reply
        .data
        .iter()
        .filter_map(|d| match d {
            Data::Fetch { items, .. } => items.as_ref().iter().find_map(|i| {
                if let MessageDataItem::ModSeq(m) = i {
                    Some(*m)
                } else {
                    None
                }
            }),
            _ => None,
        })
        .max();
    let hms = selected_hms.or(max_modseq).context("no MODSEQ at all")?;

    let t = Instant::now();
    let reply = c.ok(store("1", vec![Flag::Seen], StoreResponse::Answer)?).await?;
    r.check(
        "UID STORE +FLAGS",
        t,
        count_fetch(&reply) == 1,
        format!("{} FETCH back", count_fetch(&reply)),
    );

    let t = Instant::now();
    let reply = c
        .ok(fetch(
            "1:*",
            vec![N::Uid, N::Flags],
            vec![FetchModifier::ChangedSince(hms)],
        )?)
        .await?;
    r.check(
        "FETCH CHANGEDSINCE",
        t,
        count_fetch(&reply) == 1,
        format!("{} changed (expected 1); typed modifier", count_fetch(&reply)),
    );

    let t = Instant::now();
    let reply = c
        .ok(CommandBody::Move {
            sequence_set: set("2")?,
            mailbox: archive(),
            uid: true,
        })
        .await?;
    let copyuid = reply.codes().any(|c| matches!(c, Code::CopyUid { .. }));
    r.check(
        "UID MOVE",
        t,
        count_vanished(&reply) == 1,
        format!("VANISHED {}x as typed Data::Vanished", count_vanished(&reply)),
    );
    r.check("COPYUID visible (MOVE)", Instant::now(), copyuid, "typed Code::CopyUid");

    let t = Instant::now();
    let copy = c
        .ok(CommandBody::Copy {
            sequence_set: set("3")?,
            mailbox: archive(),
            uid: true,
        })
        .await?;
    c.ok(store("3", vec![Flag::Deleted], StoreResponse::Silent)?).await?;
    let expunge = c
        .ok(CommandBody::ExpungeUid {
            sequence_set: set("3")?,
        })
        .await?;
    let copyuid = copy.codes().any(|c| matches!(c, Code::CopyUid { .. }));
    r.check(
        "COPY + \\Deleted + UID EXPUNGE",
        t,
        copyuid && count_vanished(&expunge) == 1,
        format!("COPYUID typed: {copyuid}; VANISHED {}x", count_vanished(&expunge)),
    );

    bulk_resync(&mut c, r, uidvalidity).await?;
    idle_push(cfg, &mut c, &user, r).await?;
    idle_cancel(&mut c, r).await?;

    let t = Instant::now();
    c.run(CommandBody::Logout).await?;
    r.pass("LOGOUT", t, "");
    Ok(())
}

async fn bulk_resync(c: &mut Conn, r: &mut Report, uidvalidity: NonZeroU32) -> Result<()> {
    const N: usize = 150;
    let t = Instant::now();
    for i in 0..N {
        c.ok(append(100 + i, vec![], false)?).await?;
    }
    r.pass("APPEND x150 (sequential)", t, "one round trip per message");

    let t = Instant::now();
    let tags: Vec<_> = (0..N)
        .map(|i| append(300 + i, vec![], true).map(|body| c.enqueue(body)))
        .collect::<Result<_>>()?;
    for tag in &tags {
        let reply = c.finish(tag).await?;
        if reply.done.kind != StatusKind::Ok {
            bail!("pipelined APPEND failed: {:?}", reply.done.text);
        }
    }
    r.pass(
        "APPEND x150 (pipelined, LITERAL+)",
        t,
        "all enqueued at once, replies matched by tag",
    );

    let reply = c
        .ok(CommandBody::Select {
            mailbox: Mailbox::Inbox,
            parameters: vec![SelectParameter::CondStore],
        })
        .await?;
    let exists = reply
        .data
        .iter()
        .find_map(|d| if let Data::Exists(n) = d { Some(*n) } else { None })
        .unwrap_or(0);
    let before: NonZeroU64 = reply
        .codes()
        .find_map(|c| if let Code::HighestModSeq(v) = c { Some(*v) } else { None })
        .context("no HIGHESTMODSEQ")?;
    c.ok(store("1:*", vec![Flag::Answered], StoreResponse::Silent)?).await?;

    let t = Instant::now();
    let reply = c
        .ok(CommandBody::Select {
            mailbox: Mailbox::Inbox,
            parameters: vec![SelectParameter::QResync {
                uid_validity: uidvalidity,
                mod_sequence_value: before,
                known_uids: None,
                seq_match_data: None,
            }],
        })
        .await?;
    let fetches = count_fetch(&reply);
    r.check(
        "QRESYNC SELECT after bulk change",
        t,
        fetches == exists as usize,
        format!("{fetches} of {exists} changes reached the caller (typed SelectParameter::QResync)"),
    );
    Ok(())
}

async fn start_idle(c: &mut Conn) -> Result<Tag<'static>> {
    let tag = c.enqueue(CommandBody::Idle);
    loop {
        match c.stream.next(&mut c.client).await? {
            Event::IdleAccepted { .. } => return Ok(tag),
            Event::IdleRejected { status, .. } => bail!("IDLE rejected: {status:?}"),
            _ => {}
        }
    }
}

async fn stop_idle(c: &mut Conn, tag: &Tag<'static>) -> Result<()> {
    c.client.set_idle_done().context("IDLE was not active")?;
    let reply = c.finish(tag).await?;
    if reply.done.kind != StatusKind::Ok {
        bail!("IDLE ended with {:?}", reply.done.kind);
    }
    Ok(())
}

async fn idle_push(cfg: &Config, c: &mut Conn, user: &str, r: &mut Report) -> Result<()> {
    let tag = start_idle(c).await.context("start IDLE")?;

    let (mut second, _) = login(cfg, user).await.context("second login")?;
    let trigger = tokio::spawn(async move {
        sleep(Duration::from_millis(300)).await;
        let sent = Instant::now();
        second.ok(append(999, vec![], false)?).await.context("second APPEND")?;
        second.run(CommandBody::Logout).await.context("second LOGOUT")?;
        anyhow::Ok(sent)
    });

    let deadline = sleep(Duration::from_secs(10));
    tokio::pin!(deadline);
    let seen = loop {
        select! {
            event = c.stream.next(&mut c.client) => {
                if let Event::DataReceived { data: Data::Exists(_) } = event.context("waiting in IDLE")? {
                    break Instant::now();
                }
            }
            _ = &mut deadline => bail!("no EXISTS within 10 s"),
        }
    };
    let sent = trigger.await??;
    r.pass(
        "IDLE push",
        sent,
        format!(
            "EXISTS {:.1} ms after the other client's APPEND started",
            (seen - sent).as_secs_f64() * 1000.0
        ),
    );
    stop_idle(c, &tag).await.context("stop IDLE")
}

async fn idle_cancel(c: &mut Conn, r: &mut Report) -> Result<()> {
    let tag = start_idle(c).await?;
    let t = Instant::now();
    // Drop an in-flight `next()` future, as a UI "stop" or shutdown would.
    select! {
        event = c.stream.next(&mut c.client) => bail!("unexpected event while idle: {:?}", event.map(|e| format!("{e:?}"))),
        _ = sleep(Duration::from_millis(200)) => {}
    }
    stop_idle(c, &tag).await?;
    c.ok(CommandBody::Noop).await?;
    r.pass(
        "IDLE cancel + DONE + NOOP",
        t,
        "select! dropped next() mid-wait; connection still consistent",
    );
    Ok(())
}
