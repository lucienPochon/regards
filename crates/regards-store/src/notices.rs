//! What to tell the person (ADR-0011): a snooze ended, a reminder came due, a
//! scheduled message was held, a message was not sent. Each is shown once by
//! the desktop, then marked shown; notices are kept a week.

use crate::StoreError;
use regards_core::headers::Person;
use rusqlite::{Connection, Row, Transaction, params};

/// Notices older than this are deleted when a new one is added.
const KEPT_MS: i64 = 7 * 24 * 3600 * 1000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    SnoozeEnded,
    ReminderDue,
    SendHeld,
    SendFailed,
    SendUncertain,
    /// An agent asks something that waits for the person (ADR-0012 §3).
    AgentAsks,
}

impl Kind {
    fn as_str(self) -> &'static str {
        match self {
            Self::SnoozeEnded => "snooze_ended",
            Self::ReminderDue => "reminder_due",
            Self::SendHeld => "send_held",
            Self::SendFailed => "send_failed",
            Self::SendUncertain => "send_uncertain",
            Self::AgentAsks => "agent_asks",
        }
    }

    fn parse(text: &str) -> Result<Self, StoreError> {
        Ok(match text {
            "snooze_ended" => Self::SnoozeEnded,
            "reminder_due" => Self::ReminderDue,
            "send_held" => Self::SendHeld,
            "send_failed" => Self::SendFailed,
            "send_uncertain" => Self::SendUncertain,
            "agent_asks" => Self::AgentAsks,
            other => return Err(StoreError::Corrupt(format!("unknown notice kind {other:?}"))),
        })
    }
}

/// What a notice is about: its stored message or outbox entry, who, and the subject.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewNotice {
    pub account_id: String,
    pub kind: Kind,
    pub message_id: Option<i64>,
    pub outbox_id: Option<i64>,
    pub people: Vec<Person>,
    pub subject: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Notice {
    pub id: i64,
    pub account_id: String,
    pub kind: Kind,
    pub message_id: Option<i64>,
    pub outbox_id: Option<i64>,
    pub people: Vec<Person>,
    pub subject: String,
    pub created_at: i64,
}

/// Add a notice; the ones older than a week go.
pub fn add(tx: &Transaction<'_>, notice: &NewNotice, now: i64) -> Result<i64, StoreError> {
    tx.execute("DELETE FROM notices WHERE created_at < ?1", [now - KEPT_MS])?;
    let people = serde_json::to_string(&notice.people).expect("plain data always serializes");
    tx.execute(
        "INSERT INTO notices (account_id, kind, message_id, outbox_id, people_json, subject, created_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![
            notice.account_id,
            notice.kind.as_str(),
            notice.message_id,
            notice.outbox_id,
            people,
            notice.subject,
            now
        ],
    )?;
    Ok(tx.last_insert_rowid())
}

/// Notices not shown yet, oldest first.
pub fn unshown(conn: &Connection, limit: u32) -> Result<Vec<Notice>, StoreError> {
    let mut statement = conn.prepare_cached(
        "SELECT id, account_id, kind, message_id, outbox_id, people_json, subject, created_at \
         FROM notices WHERE shown = 0 ORDER BY id LIMIT ?1",
    )?;
    let rows = statement.query_map([limit], from_row)?;
    rows.map(|row| row?).collect()
}

/// Every notice up to `up_to` has been shown; returns how many changed.
pub fn mark_shown(tx: &Transaction<'_>, up_to: i64) -> Result<usize, StoreError> {
    Ok(tx.execute("UPDATE notices SET shown = 1 WHERE shown = 0 AND id <= ?1", [up_to])?)
}

fn from_row(row: &Row<'_>) -> rusqlite::Result<Result<Notice, StoreError>> {
    let (kind, people): (String, String) = (row.get(2)?, row.get(5)?);
    let parse = || -> Result<Notice, StoreError> {
        Ok(Notice {
            id: row.get(0)?,
            account_id: row.get(1)?,
            kind: Kind::parse(&kind)?,
            message_id: row.get(3)?,
            outbox_id: row.get(4)?,
            people: serde_json::from_str(&people).map_err(|error| StoreError::Corrupt(format!("notice: {error}")))?,
            subject: row.get(6)?,
            created_at: row.get(7)?,
        })
    };
    Ok(parse())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Store;
    use crate::accounts::{self, AuthKind, NewAccount, Security, Server};
    use regards_core::paths::DataDirs;

    #[tokio::test]
    async fn notices_are_shown_once_and_kept_a_week() {
        let temp = tempfile::tempdir().unwrap();
        let store = Store::open(&DataDirs::at(temp.path())).await.unwrap();
        let account = accounts::new_id();
        let a = account.clone();
        store
            .write(move |tx| {
                let server = Server {
                    host: "h".into(),
                    port: 993,
                    security: Security::Tls,
                };
                accounts::insert(
                    tx,
                    NewAccount {
                        id: a,
                        display_name: "A".into(),
                        email: "a@example.test".into(),
                        auth_kind: AuthKind::Password,
                        username: "a".into(),
                        secret_key: "k".into(),
                        imap: server.clone(),
                        smtp: server,
                        imap_capabilities: vec![],
                    },
                )
            })
            .await
            .unwrap();
        let notice = |kind, subject: &str| NewNotice {
            account_id: account.clone(),
            kind,
            message_id: None,
            outbox_id: Some(7),
            people: vec![Person {
                name: Some("Lena".into()),
                address: Some("lena@example.test".into()),
            }],
            subject: subject.into(),
        };
        let (first, second) = (notice(Kind::SendHeld, "Rota"), notice(Kind::SnoozeEnded, "Seeds"));
        let ids = store
            .write(move |tx| Ok((add(tx, &first, 1_000)?, add(tx, &second, 2_000)?)))
            .await
            .unwrap()
            .value;
        let waiting = store.read(|c| unshown(c, 10)).await.unwrap();
        assert_eq!(
            waiting.iter().map(|n| (n.kind, n.subject.as_str())).collect::<Vec<_>>(),
            [(Kind::SendHeld, "Rota"), (Kind::SnoozeEnded, "Seeds")]
        );
        assert_eq!(waiting[0].people[0].name.as_deref(), Some("Lena"));

        // Shown up to the first: only the second still waits.
        let up_to = ids.0;
        assert_eq!(store.write(move |tx| mark_shown(tx, up_to)).await.unwrap().value, 1);
        assert_eq!(store.read(|c| unshown(c, 10)).await.unwrap()[0].id, ids.1);

        // A week on, adding one clears the old ones.
        let later = notice(Kind::ReminderDue, "Lunch");
        store
            .write(move |tx| add(tx, &later, 2_000 + KEPT_MS + 1))
            .await
            .unwrap();
        let left: i64 = store
            .read(|c| Ok(c.query_row("SELECT COUNT(*) FROM notices", [], |r| r.get(0))?))
            .await
            .unwrap();
        assert_eq!(left, 1);
    }
}
