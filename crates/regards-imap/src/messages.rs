//! Working inside one mailbox: SELECT, UID SEARCH, header and flag FETCH,
//! and the few writes sync and tests need (APPEND, STORE, EXPUNGE).

use crate::conn::Reply;
use crate::{ImapError, Session, clean};
use imap_types::command::{CommandBody, FetchModifier, SelectParameter};
use imap_types::core::{AString, Literal, Vec1};
use imap_types::extensions::binary::LiteralOrLiteral8;
use imap_types::fetch::{MessageDataItem, MessageDataItemName, Section};
use imap_types::flag::{Flag, FlagFetch, StoreResponse, StoreType};
use imap_types::mailbox::Mailbox;
use imap_types::response::{Code, Data, StatusKind};
use imap_types::search::SearchKey;
use imap_types::sequence::SequenceSet;
use std::num::NonZeroU64;

/// The headers Regards keeps for lists and threading.
const HEADER_FIELDS: &[&str] = &[
    "DATE",
    "FROM",
    "TO",
    "CC",
    "SUBJECT",
    "MESSAGE-ID",
    "IN-REPLY-TO",
    "REFERENCES",
];

/// What SELECT reported about a mailbox.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Selected {
    pub uid_validity: u32,
    pub uid_next: Option<u32>,
    /// `None` without CONDSTORE, and sometimes on a new mailbox (Dovecot omits it).
    pub highest_modseq: Option<u64>,
    pub exists: u32,
}

/// IMAP system flags plus keywords.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Flags {
    pub seen: bool,
    pub answered: bool,
    pub flagged: bool,
    pub deleted: bool,
    pub draft: bool,
    pub keywords: Vec<String>,
}

/// A system flag to set or clear.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SystemFlag {
    Seen,
    Answered,
    Flagged,
    Deleted,
    Draft,
}

/// A message's metadata and raw header block.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FetchedHeaders {
    pub uid: u32,
    pub flags: Flags,
    /// INTERNALDATE, Unix time in milliseconds.
    pub internal_date: Option<i64>,
    pub size: Option<u32>,
    pub modseq: Option<u64>,
    /// `BODY[HEADER.FIELDS (…)]`: always a literal, so any bytes survive.
    pub header: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FlagUpdate {
    pub uid: u32,
    pub flags: Flags,
    pub modseq: Option<u64>,
}

/// Results of a FETCH, and how many responses could not be read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fetched<T> {
    pub items: Vec<T>,
    pub skipped: usize,
}

/// Which messages a FETCH covers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MessageSet {
    /// UIDs, for example `"120:*"` or `"3,7,9"`.
    Uids(String),
    /// Sequence numbers, for example `"1:*"`.
    Sequence(String),
}

impl Session {
    /// Open a mailbox read-write, with CONDSTORE when the server has it.
    pub async fn select(&mut self, remote_name: &[u8]) -> Result<Selected, ImapError> {
        let parameters = if self.has_capability("CONDSTORE") {
            vec![SelectParameter::CondStore]
        } else {
            vec![]
        };
        let reply = self
            .conn
            .run(CommandBody::Select {
                mailbox: mailbox(remote_name)?,
                parameters,
            })
            .await?;
        ok(&reply, "SELECT")?;
        let codes = || {
            reply
                .untagged
                .iter()
                .chain([&reply.done])
                .filter_map(|s| s.code.as_ref())
        };
        let uid_validity = codes()
            .find_map(|code| match code {
                Code::UidValidity(v) => Some(v.get()),
                _ => None,
            })
            .ok_or_else(|| ImapError::Protocol("the server did not report UIDVALIDITY".into()))?;
        Ok(Selected {
            uid_validity,
            uid_next: codes().find_map(|code| match code {
                Code::UidNext(v) => Some(v.get()),
                _ => None,
            }),
            highest_modseq: codes().find_map(|code| match code {
                Code::HighestModSeq(v) => Some(v.get()),
                _ => None,
            }),
            exists: reply
                .data
                .iter()
                .find_map(|data| match data {
                    Data::Exists(n) => Some(*n),
                    _ => None,
                })
                .unwrap_or(0),
        })
    }

    /// Every UID in the selected mailbox. Only a completed command counts, so
    /// a cut-off answer is an error, never a shorter list (spike 07 §7).
    pub async fn uid_search_all(&mut self) -> Result<Vec<u32>, ImapError> {
        let reply = self
            .conn
            .run(CommandBody::search(None, Vec1::from(SearchKey::All), true))
            .await?;
        ok(&reply, "UID SEARCH")?;
        let mut uids: Vec<u32> = reply
            .data
            .iter()
            .flat_map(|data| match data {
                Data::Search(uids, ..) => uids.iter().map(|uid| uid.get()).collect(),
                _ => Vec::new(),
            })
            .collect();
        uids.sort_unstable();
        Ok(uids)
    }

    /// Flags, dates, size and the header block of `set`.
    pub async fn fetch_headers(&mut self, set: MessageSet) -> Result<Fetched<FetchedHeaders>, ImapError> {
        let fields: Vec<AString<'static>> = HEADER_FIELDS
            .iter()
            .map(|field| AString::try_from(*field).expect("header names are atoms"))
            .collect();
        let mut items = vec![
            MessageDataItemName::Uid,
            MessageDataItemName::Flags,
            MessageDataItemName::InternalDate,
            MessageDataItemName::Rfc822Size,
            MessageDataItemName::BodyExt {
                section: Some(Section::HeaderFields(
                    None,
                    Vec1::try_from(fields).expect("the field list is not empty"),
                )),
                partial: None,
                peek: true,
            },
        ];
        if self.has_capability("CONDSTORE") {
            items.push(MessageDataItemName::ModSeq);
        }
        let (sequence_set, uid) = match set {
            MessageSet::Uids(set) => (sequence_set(&set)?, true),
            MessageSet::Sequence(set) => (sequence_set(&set)?, false),
        };
        let reply = self
            .conn
            .run_skipping(CommandBody::Fetch {
                sequence_set,
                macro_or_item_names: items.into(),
                uid,
                modifiers: vec![],
            })
            .await?;
        ok(&reply, "FETCH")?;
        let mut fetched = Vec::new();
        for data in &reply.data {
            let Data::Fetch { items, .. } = data else { continue };
            let mut message = FetchedHeaders {
                uid: 0,
                flags: Flags::default(),
                internal_date: None,
                size: None,
                modseq: None,
                header: Vec::new(),
            };
            let mut has_header = false;
            for item in items.as_ref() {
                match item {
                    MessageDataItem::Uid(uid) => message.uid = uid.get(),
                    MessageDataItem::Flags(flags) => message.flags = flags_of(flags),
                    MessageDataItem::InternalDate(date) => {
                        message.internal_date = Some(date.as_ref().timestamp_millis())
                    }
                    MessageDataItem::Rfc822Size(size) => message.size = Some(*size),
                    MessageDataItem::ModSeq(modseq) => message.modseq = Some(modseq.get()),
                    MessageDataItem::BodyExt { data, .. } => {
                        has_header = true;
                        message.header = data.0.as_ref().map(|s| s.as_ref().to_vec()).unwrap_or_default();
                    }
                    _ => {}
                }
            }
            // Flag-only FETCH responses (changes by another client) belong to flag sync.
            if message.uid != 0 && has_header {
                fetched.push(message);
            }
        }
        Ok(Fetched {
            items: fetched,
            skipped: reply.skipped,
        })
    }

    /// Flags of `uids`; with `changed_since` (CONDSTORE), only those changed after that modseq.
    pub async fn fetch_flags(
        &mut self,
        uids: &str,
        changed_since: Option<u64>,
    ) -> Result<Fetched<FlagUpdate>, ImapError> {
        let condstore = self.has_capability("CONDSTORE");
        let mut items = vec![MessageDataItemName::Uid, MessageDataItemName::Flags];
        if condstore {
            items.push(MessageDataItemName::ModSeq);
        }
        let modifiers = match changed_since.and_then(NonZeroU64::new) {
            Some(modseq) if condstore => vec![FetchModifier::ChangedSince(modseq)],
            _ => vec![],
        };
        let reply = self
            .conn
            .run_skipping(CommandBody::Fetch {
                sequence_set: sequence_set(uids)?,
                macro_or_item_names: items.into(),
                uid: true,
                modifiers,
            })
            .await?;
        ok(&reply, "FETCH")?;
        let updates = reply
            .data
            .iter()
            .filter_map(|data| {
                let Data::Fetch { items, .. } = data else { return None };
                let mut update = FlagUpdate {
                    uid: 0,
                    flags: Flags::default(),
                    modseq: None,
                };
                let mut has_flags = false;
                for item in items.as_ref() {
                    match item {
                        MessageDataItem::Uid(uid) => update.uid = uid.get(),
                        MessageDataItem::Flags(flags) => {
                            has_flags = true;
                            update.flags = flags_of(flags);
                        }
                        MessageDataItem::ModSeq(modseq) => update.modseq = Some(modseq.get()),
                        _ => {}
                    }
                }
                (update.uid != 0 && has_flags).then_some(update)
            })
            .collect();
        Ok(Fetched {
            items: updates,
            skipped: reply.skipped,
        })
    }

    /// Store a message; returns its UID when the server reports it (UIDPLUS).
    pub async fn append(
        &mut self,
        remote_name: &[u8],
        flags: &[SystemFlag],
        message: &[u8],
    ) -> Result<Option<u32>, ImapError> {
        let literal = Literal::try_from(message.to_vec())
            .map_err(|_| ImapError::Protocol("the message cannot be sent as an IMAP literal".into()))?;
        let reply = self
            .conn
            .run(CommandBody::Append {
                mailbox: mailbox(remote_name)?,
                flags: flags.iter().map(|flag| system_flag(*flag)).collect(),
                date: None,
                message: LiteralOrLiteral8::Literal(literal),
            })
            .await?;
        ok(&reply, "APPEND")?;
        Ok(match &reply.done.code {
            Some(Code::AppendUid { uid, .. }) => Some(uid.get()),
            _ => None,
        })
    }

    /// Add or remove flags on `uids` in the selected mailbox.
    pub async fn store_flags(&mut self, uids: &str, add: bool, flags: &[SystemFlag]) -> Result<(), ImapError> {
        let reply = self
            .conn
            .run(CommandBody::Store {
                sequence_set: sequence_set(uids)?,
                kind: if add { StoreType::Add } else { StoreType::Remove },
                response: StoreResponse::Silent,
                flags: flags.iter().map(|flag| system_flag(*flag)).collect(),
                uid: true,
                modifiers: vec![],
            })
            .await?;
        ok(&reply, "STORE")
    }

    /// Remove messages flagged `\Deleted` from the selected mailbox.
    pub async fn expunge(&mut self) -> Result<(), ImapError> {
        let reply = self.conn.run(CommandBody::Expunge).await?;
        ok(&reply, "EXPUNGE")
    }

    pub async fn create_mailbox(&mut self, remote_name: &[u8]) -> Result<(), ImapError> {
        let reply = self
            .conn
            .run(CommandBody::Create {
                mailbox: mailbox(remote_name)?,
            })
            .await?;
        ok(&reply, "CREATE")
    }

    pub async fn delete_mailbox(&mut self, remote_name: &[u8]) -> Result<(), ImapError> {
        let reply = self
            .conn
            .run(CommandBody::Delete {
                mailbox: mailbox(remote_name)?,
            })
            .await?;
        ok(&reply, "DELETE")
    }
}

fn ok(reply: &Reply, command: &str) -> Result<(), ImapError> {
    if reply.done.kind == StatusKind::Ok {
        Ok(())
    } else {
        Err(ImapError::Protocol(format!(
            "{command} failed: {}",
            clean(reply.done.text.as_ref())
        )))
    }
}

fn mailbox(remote_name: &[u8]) -> Result<Mailbox<'static>, ImapError> {
    Mailbox::try_from(remote_name.to_vec()).map_err(|_| ImapError::Protocol("invalid mailbox name".into()))
}

fn sequence_set(set: &str) -> Result<SequenceSet, ImapError> {
    SequenceSet::try_from(set).map_err(|_| ImapError::Protocol(format!("invalid message set {set:?}")))
}

fn flags_of(flags: &[FlagFetch<'_>]) -> Flags {
    let mut result = Flags::default();
    for flag in flags {
        match flag {
            FlagFetch::Flag(Flag::Seen) => result.seen = true,
            FlagFetch::Flag(Flag::Answered) => result.answered = true,
            FlagFetch::Flag(Flag::Flagged) => result.flagged = true,
            FlagFetch::Flag(Flag::Deleted) => result.deleted = true,
            FlagFetch::Flag(Flag::Draft) => result.draft = true,
            FlagFetch::Flag(other) => result.keywords.push(other.to_string()),
            FlagFetch::Recent => {}
        }
    }
    result
}

fn system_flag(flag: SystemFlag) -> Flag<'static> {
    match flag {
        SystemFlag::Seen => Flag::Seen,
        SystemFlag::Answered => Flag::Answered,
        SystemFlag::Flagged => Flag::Flagged,
        SystemFlag::Deleted => Flag::Deleted,
        SystemFlag::Draft => Flag::Draft,
    }
}
