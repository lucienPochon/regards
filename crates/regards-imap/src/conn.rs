//! A small command/response layer over imap-next's client state machine.

use crate::ImapError;
use imap_next::client::{Client, Event, Options};
use imap_next::stream::{Error as StreamError, Stream};
use imap_types::command::{Command, CommandBody};
use imap_types::core::{Tag, TagGenerator};
use imap_types::response::{Data, Greeting, Status, StatusBody, Tagged};
use std::time::Duration;

/// How long one command may take before the connection is considered stuck.
const COMMAND_TIMEOUT: Duration = Duration::from_secs(60);

/// Everything the server sent for one command.
pub(crate) struct Reply {
    pub data: Vec<Data<'static>>,
    pub done: StatusBody<'static>,
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
    pub(crate) async fn run(&mut self, body: CommandBody<'static>) -> Result<Reply, ImapError> {
        let tag = self.tags.generate();
        let name = body.name();
        self.client.enqueue_command(Command { tag: tag.clone(), body });
        tokio::time::timeout(COMMAND_TIMEOUT, self.finish(&tag))
            .await
            .map_err(|_| ImapError::Timeout(name))?
    }

    async fn finish(&mut self, tag: &Tag<'static>) -> Result<Reply, ImapError> {
        let mut data = Vec::new();
        loop {
            match self.stream.next(&mut self.client).await.map_err(stream_error)? {
                Event::DataReceived { data: item } => data.push(item),
                Event::StatusReceived {
                    status: Status::Tagged(Tagged { tag: done_tag, body }),
                } if &done_tag == tag => return Ok(Reply { data, done: body }),
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

fn stream_error(error: StreamError<imap_next::client::Error>) -> ImapError {
    match error {
        StreamError::Closed => ImapError::Protocol("the server closed the connection".into()),
        StreamError::Io(error) => ImapError::Connect(format!("connection lost: {error}")),
        StreamError::Tls(error) => ImapError::Tls(error.to_string()),
        // Never include the discarded bytes: they may contain mail.
        StreamError::State(error) => ImapError::Protocol(format!("malformed server response: {error}")),
    }
}
