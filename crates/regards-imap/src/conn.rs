//! A small command/response layer over imap-next's client state machine.

use crate::ImapError;
use imap_next::client::{Client, Event, Options};
use imap_next::stream::{Error as StreamError, Stream};
use imap_types::auth::{AuthMechanism, AuthenticateData};
use imap_types::command::{Command, CommandBody};
use imap_types::core::{Tag, TagGenerator};
use imap_types::response::{Data, Greeting, Status, StatusBody, Tagged};
use imap_types::secret::Secret;
use std::borrow::Cow;
use std::future::Future;
use std::time::Duration;

/// How long one command may take before the connection is considered stuck.
const COMMAND_TIMEOUT: Duration = Duration::from_secs(120);

/// Everything the server sent for one command.
pub(crate) struct Reply {
    pub data: Vec<Data<'static>>,
    /// Untagged `OK`/`NO` lines, which carry codes such as UIDVALIDITY.
    pub untagged: Vec<StatusBody<'static>>,
    pub done: StatusBody<'static>,
    /// Responses that could not be parsed and were skipped (see [`Conn::run_skipping`]).
    pub skipped: usize,
}

pub(crate) struct Conn {
    stream: Stream,
    client: Client,
    tags: TagGenerator,
}

impl Conn {
    /// Wrap a connected stream and wait for the server greeting.
    pub(crate) async fn open(mut stream: Stream) -> Result<(Self, Greeting<'static>), ImapError> {
        let mut client = Client::new(Options::default());
        let greeting = tokio::time::timeout(COMMAND_TIMEOUT, async {
            loop {
                match stream.next(&mut client).await {
                    Ok(Event::GreetingReceived { greeting }) => return Ok(greeting),
                    Ok(_) => {}
                    // Something other than an IMAP greeting: probably the wrong port or service.
                    Err(StreamError::State(_)) => return Err(ImapError::NotImap("unexpected greeting".into())),
                    Err(error) => return Err(stream_error(error)),
                }
            }
        })
        .await
        .map_err(|_| ImapError::Timeout("waiting for the server greeting"))??;
        Ok((
            Self {
                stream,
                client,
                tags: TagGenerator::new(),
            },
            greeting,
        ))
    }

    /// Send one command and collect everything up to its tagged completion.
    /// A malformed response fails the command.
    pub(crate) async fn run(&mut self, body: CommandBody<'static>) -> Result<Reply, ImapError> {
        self.run_with(body, false).await
    }

    /// Like [`Conn::run`], but a malformed response is skipped and counted
    /// instead of failing the command, so one unreadable message does not stop
    /// a sync (spike 07 §7).
    pub(crate) async fn run_skipping(&mut self, body: CommandBody<'static>) -> Result<Reply, ImapError> {
        self.run_with(body, true).await
    }

    /// AUTHENTICATE XOAUTH2 with `sasl` (sent at once with SASL-IR, else
    /// when asked). A failed sign-in comes as a challenge carrying the
    /// error, answered with nothing to end it (RFC 7628 §3.2.3).
    pub(crate) async fn authenticate_xoauth2(
        &mut self,
        sasl: Vec<u8>,
        initial: bool,
    ) -> Result<StatusBody<'static>, ImapError> {
        let tag = self.tags.generate();
        let initial_response = initial.then(|| Secret::new(Cow::Owned(sasl.clone())));
        self.client.enqueue_command(Command {
            tag: tag.clone(),
            body: CommandBody::Authenticate {
                mechanism: AuthMechanism::XOAuth2,
                initial_response,
            },
        });
        let mut sent = initial;
        let exchange = async {
            loop {
                match self.stream.next(&mut self.client).await.map_err(stream_error)? {
                    Event::AuthenticateContinuationRequestReceived { .. } => {
                        let data = if sent { Vec::new() } else { sasl.clone() };
                        sent = true;
                        let _ = self
                            .client
                            .set_authenticate_data(AuthenticateData::Continue(Secret::new(Cow::Owned(data))));
                    }
                    Event::AuthenticateStatusReceived {
                        status: Status::Tagged(Tagged { tag: done_tag, body }),
                        ..
                    } if done_tag == tag => return Ok(body),
                    Event::AuthenticateStatusReceived { status, .. } => {
                        return Err(ImapError::Protocol(format!("unexpected answer to sign-in: {status:?}")));
                    }
                    _ => {}
                }
            }
        };
        tokio::time::timeout(COMMAND_TIMEOUT, exchange)
            .await
            .map_err(|_| ImapError::Timeout("AUTHENTICATE"))?
    }

    async fn run_with(&mut self, body: CommandBody<'static>, skip_malformed: bool) -> Result<Reply, ImapError> {
        let tag = self.tags.generate();
        let name = body.name();
        self.client.enqueue_command(Command { tag: tag.clone(), body });
        tokio::time::timeout(COMMAND_TIMEOUT, self.finish(&tag, skip_malformed))
            .await
            .map_err(|_| ImapError::Timeout(name))?
    }

    async fn finish(&mut self, tag: &Tag<'static>, skip_malformed: bool) -> Result<Reply, ImapError> {
        let (mut data, mut untagged, mut skipped) = (Vec::new(), Vec::new(), 0);
        loop {
            let event = match self.stream.next(&mut self.client).await {
                Ok(event) => event,
                Err(StreamError::State(_)) if skip_malformed => {
                    skipped += 1;
                    continue;
                }
                Err(error) => return Err(stream_error(error)),
            };
            match event {
                Event::DataReceived { data: item } => data.push(item),
                Event::StatusReceived {
                    status: Status::Untagged(body),
                } => untagged.push(body),
                Event::StatusReceived {
                    status: Status::Tagged(Tagged { tag: done_tag, body }),
                } if &done_tag == tag => {
                    return Ok(Reply {
                        data,
                        untagged,
                        done: body,
                        skipped,
                    });
                }
                // BYE precedes the tagged OK of LOGOUT; otherwise the stream closes next.
                Event::StatusReceived { status: Status::Bye(_) } => {}
                Event::CommandRejected { command, status, .. } if &command.tag == tag => {
                    return Err(ImapError::Protocol(format!(
                        "the server rejected the command: {status:?}"
                    )));
                }
                _ => {}
            }
        }
    }
}

/// Why an IDLE ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IdleEnd {
    /// The server reported new, changed or removed messages.
    Changed,
    /// The time limit passed without changes; IDLE again to keep the session alive.
    Timeout,
    /// The caller asked to stop.
    Stopped,
}

impl Conn {
    /// Wait in IDLE (RFC 2177) until the server reports a change, `max`
    /// passes, or `stop` completes; then end IDLE cleanly.
    pub(crate) async fn idle(&mut self, max: Duration, stop: impl Future<Output = ()>) -> Result<IdleEnd, ImapError> {
        let tag = self.tags.generate();
        self.client.enqueue_command(Command {
            tag: tag.clone(),
            body: CommandBody::Idle,
        });
        tokio::time::timeout(COMMAND_TIMEOUT, async {
            loop {
                match self.stream.next(&mut self.client).await.map_err(stream_error)? {
                    Event::IdleAccepted { .. } => return Ok(()),
                    Event::IdleRejected { status, .. } => {
                        return Err(ImapError::Protocol(format!("the server refused IDLE: {status:?}")));
                    }
                    _ => {}
                }
            }
        })
        .await
        .map_err(|_| ImapError::Timeout("starting IDLE"))??;

        tokio::pin!(stop);
        let deadline = tokio::time::sleep(max);
        tokio::pin!(deadline);
        let end = loop {
            // Dropping an in-flight `next()` is safe: imap-next keeps its state
            // in `client` (verified in spike 07, "IDLE cancel").
            tokio::select! {
                event = self.stream.next(&mut self.client) => match event.map_err(stream_error)? {
                    Event::DataReceived { data } if is_change(&data) => break IdleEnd::Changed,
                    Event::StatusReceived { status: Status::Bye(_) } => {
                        return Err(ImapError::Protocol("the server ended the connection".into()));
                    }
                    _ => {}
                },
                () = &mut deadline => break IdleEnd::Timeout,
                () = &mut stop => break IdleEnd::Stopped,
            }
        };
        self.client.set_idle_done();
        let reply = tokio::time::timeout(COMMAND_TIMEOUT, self.finish(&tag, false))
            .await
            .map_err(|_| ImapError::Timeout("ending IDLE"))??;
        if reply.done.kind != imap_types::response::StatusKind::Ok {
            return Err(ImapError::Protocol("IDLE did not end cleanly".into()));
        }
        Ok(end)
    }
}

fn is_change(data: &Data<'_>) -> bool {
    matches!(
        data,
        Data::Exists(_) | Data::Expunge(_) | Data::Fetch { .. } | Data::Vanished { .. }
    )
}

fn stream_error(error: StreamError<imap_next::client::Error>) -> ImapError {
    match error {
        StreamError::Closed => ImapError::Protocol("the server closed the connection".into()),
        StreamError::Io(error) => ImapError::Connect(format!("connection lost: {error}")),
        StreamError::Tls(error) => ImapError::Tls(error.to_string()),
        // Never include the discarded bytes: they may contain mail.
        StreamError::State(error) => ImapError::Protocol(format!("malformed server response: {error}")),
    }
}
