//! Holds (ADR-0012 §3): what an agent asked that waits for the person. Only
//! the app decides; approving runs the request as the agent made it.

use crate::StoreError;
use rusqlite::{Connection, OptionalExtension, Row, Transaction, params};

/// Why a request waits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reason {
    Sending,
    /// Many messages at once.
    Bulk,
    /// Deleting for good.
    Delete,
}

impl Reason {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Sending => "sending",
            Self::Bulk => "bulk",
            Self::Delete => "delete",
        }
    }

    fn parse(text: &str) -> Self {
        match text {
            "bulk" => Self::Bulk,
            "delete" => Self::Delete,
            _ => Self::Sending,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Pending,
    /// Approved, and it ran.
    Approved,
    Rejected,
    /// Not decided in time.
    Expired,
    /// Approved, but it could not run (the draft changed, say).
    Failed,
}

impl Status {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Approved => "approved",
            Self::Rejected => "rejected",
            Self::Expired => "expired",
            Self::Failed => "failed",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewHold {
    pub created_at: i64,
    pub expires_at: i64,
    pub session: String,
    pub client: String,
    pub account_id: String,
    pub method: String,
    /// The request's parameters, JSON.
    pub params: String,
    pub description: String,
    pub reason: Reason,
    /// For a send: the draft, and its version when the agent asked.
    pub draft: Option<(i64, i64)>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hold {
    pub id: i64,
    pub hold: NewHold,
    pub status: Status,
    pub decided_at: Option<i64>,
    /// What running it gave (JSON), or why it could not run.
    pub result: Option<String>,
    pub error: Option<String>,
}

const COLUMNS: &str = "id, created_at, expires_at, session, client, account_id, method, params, description, \
    reason, draft_id, draft_version, status, decided_at, result, error";

pub fn add(tx: &Transaction<'_>, hold: &NewHold) -> Result<i64, StoreError> {
    tx.prepare_cached(
        "INSERT INTO holds (created_at, expires_at, session, client, account_id, method, params, description, \
         reason, draft_id, draft_version) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
    )?
    .execute(params![
        hold.created_at,
        hold.expires_at,
        hold.session,
        hold.client,
        hold.account_id,
        hold.method,
        hold.params,
        hold.description,
        hold.reason.as_str(),
        hold.draft.map(|(id, _)| id),
        hold.draft.map(|(_, version)| version),
    ])?;
    Ok(tx.last_insert_rowid())
}

/// A hold as it stands at `now`: pending past its time is expired.
pub fn get(conn: &Connection, id: i64, now: i64) -> Result<Option<Hold>, StoreError> {
    Ok(conn
        .query_row(&format!("SELECT {COLUMNS} FROM holds WHERE id = ?1"), [id], |row| {
            from_row(row, now)
        })
        .optional()?)
}

/// Newest first; only those still waiting at `now` when `pending`.
pub fn list(conn: &Connection, pending: bool, now: i64, limit: u32) -> Result<Vec<Hold>, StoreError> {
    let mut statement = conn.prepare_cached(&format!(
        "SELECT {COLUMNS} FROM holds WHERE NOT ?1 OR (status = 'pending' AND expires_at >= ?2) \
         ORDER BY id DESC LIMIT ?3"
    ))?;
    let rows = statement.query_map(params![pending, now, limit], |row| from_row(row, now))?;
    Ok(rows.collect::<Result<_, _>>()?)
}

/// How many holds wait for the person at `now`.
pub fn waiting(conn: &Connection, now: i64) -> Result<u32, StoreError> {
    Ok(conn.query_row(
        "SELECT count(*) FROM holds WHERE status = 'pending' AND expires_at >= ?1",
        [now],
        |row| row.get(0),
    )?)
}

/// Decide a hold still waiting at `now`: approved or rejected. `None` if
/// it is not waiting any more (decided, expired, or no such hold).
pub fn decide(tx: &Transaction<'_>, id: i64, approve: bool, now: i64) -> Result<Option<Hold>, StoreError> {
    let status = if approve { Status::Approved } else { Status::Rejected };
    let changed = tx.execute(
        "UPDATE holds SET status = ?2, decided_at = ?3 WHERE id = ?1 AND status = 'pending' AND expires_at >= ?3",
        params![id, status.as_str(), now],
    )?;
    if changed == 0 {
        return Ok(None);
    }
    get(tx, id, now)
}

/// What running an approved hold gave; an error marks it failed.
pub fn finish(tx: &Transaction<'_>, id: i64, outcome: Result<&str, &str>) -> Result<(), StoreError> {
    match outcome {
        Ok(result) => tx.execute("UPDATE holds SET result = ?2 WHERE id = ?1", params![id, result])?,
        Err(error) => tx.execute(
            "UPDATE holds SET status = 'failed', error = ?2 WHERE id = ?1",
            params![id, error],
        )?,
    };
    Ok(())
}

fn from_row(row: &Row<'_>, now: i64) -> rusqlite::Result<Hold> {
    let expires_at: i64 = row.get(2)?;
    let reason: String = row.get(9)?;
    let draft_id: Option<i64> = row.get(10)?;
    let draft_version: Option<i64> = row.get(11)?;
    let status: String = row.get(12)?;
    let status = match status.as_str() {
        "approved" => Status::Approved,
        "rejected" => Status::Rejected,
        "failed" => Status::Failed,
        _ if expires_at < now => Status::Expired,
        _ => Status::Pending,
    };
    Ok(Hold {
        id: row.get(0)?,
        hold: NewHold {
            created_at: row.get(1)?,
            expires_at,
            session: row.get(3)?,
            client: row.get(4)?,
            account_id: row.get(5)?,
            method: row.get(6)?,
            params: row.get(7)?,
            description: row.get(8)?,
            reason: Reason::parse(&reason),
            draft: draft_id.zip(draft_version),
        },
        status,
        decided_at: row.get(13)?,
        result: row.get(14)?,
        error: row.get(15)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Store;
    use crate::accounts::{self, AuthKind, NewAccount, Security, Server};
    use regards_core::paths::DataDirs;

    fn hold(at: i64, description: &str) -> NewHold {
        NewHold {
            created_at: at,
            expires_at: at + 100,
            session: "s1".into(),
            client: "mcp".into(),
            account_id: "a1".into(),
            method: "draft.send".into(),
            params: "{\"id\":4}".into(),
            description: description.into(),
            reason: Reason::Sending,
            draft: Some((4, 17)),
        }
    }

    #[tokio::test]
    async fn waits_until_decided_once_or_until_its_time_is_up() {
        let temp = tempfile::tempdir().unwrap();
        let store = Store::open(&DataDirs::at(temp.path())).await.unwrap();
        let server = Server {
            host: "h".into(),
            port: 993,
            security: Security::Tls,
        };
        let (first, second) = store
            .write(move |tx| {
                accounts::insert(
                    tx,
                    NewAccount {
                        id: "a1".into(),
                        display_name: "Ada".into(),
                        email: "ada@example.test".into(),
                        auth_kind: AuthKind::Password,
                        username: "ada".into(),
                        secret_key: "k".into(),
                        imap: server.clone(),
                        smtp: server,
                        imap_capabilities: vec![],
                    },
                )?;
                Ok((
                    add(tx, &hold(1_000, "Send “Plans”"))?,
                    add(tx, &hold(2_000, "Send “Rota”"))?,
                ))
            })
            .await
            .unwrap()
            .value;
        let waiting = store.read(|conn| list(conn, true, 1_050, 10)).await.unwrap();
        assert_eq!(waiting.len(), 2);
        assert_eq!(
            waiting[0].hold,
            hold(2_000, "Send “Rota”"),
            "newest first, read back whole"
        );

        // Approved once; then it is not waiting any more.
        let approved = store
            .write(move |tx| decide(tx, first, true, 1_050))
            .await
            .unwrap()
            .value;
        assert_eq!(approved.map(|h| h.status), Some(Status::Approved));
        assert!(
            store
                .write(move |tx| decide(tx, first, false, 1_060))
                .await
                .unwrap()
                .value
                .is_none()
        );
        store
            .write(move |tx| finish(tx, first, Err("the draft changed")))
            .await
            .unwrap();
        let failed = store.read(move |conn| get(conn, first, 1_070)).await.unwrap().unwrap();
        assert_eq!(
            (failed.status, failed.error.as_deref()),
            (Status::Failed, Some("the draft changed"))
        );

        // Past its time: expired, and no longer decided.
        assert_eq!(
            store
                .read(move |conn| get(conn, second, 2_101))
                .await
                .unwrap()
                .unwrap()
                .status,
            Status::Expired
        );
        assert!(
            store
                .write(move |tx| decide(tx, second, true, 2_101))
                .await
                .unwrap()
                .value
                .is_none()
        );
        assert!(store.read(|conn| list(conn, true, 2_101, 10)).await.unwrap().is_empty());
        assert_eq!(store.read(|conn| list(conn, false, 2_101, 10)).await.unwrap().len(), 2);
    }
}
