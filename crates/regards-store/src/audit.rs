//! What agents did (ADR-0012 §4): a row per request an agent made, allowed or
//! refused, newest last. The person reviews it in Settings › Agents.

use crate::StoreError;
use regards_core::agents::Access;
use rusqlite::{Connection, Row, Transaction, params};

/// What became of a request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    Allowed,
    Denied,
    /// Waiting for the person to approve it.
    Held,
    Approved,
    Rejected,
}

impl Decision {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Allowed => "allowed",
            Self::Denied => "denied",
            Self::Held => "held",
            Self::Approved => "approved",
            Self::Rejected => "rejected",
        }
    }

    fn parse(text: &str) -> Self {
        match text {
            "denied" => Self::Denied,
            "held" => Self::Held,
            "approved" => Self::Approved,
            "rejected" => Self::Rejected,
            _ => Self::Allowed,
        }
    }
}

/// A request to record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewEntry {
    pub at: i64,
    pub session: String,
    pub client: String,
    pub method: String,
    /// JSON, secrets already taken out.
    pub args: String,
    pub account_id: Option<String>,
    pub access: Option<Access>,
    pub decision: Decision,
    /// `(code, message)` when it failed.
    pub error: Option<(i32, String)>,
    pub duration_ms: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub id: i64,
    pub entry: NewEntry,
}

pub fn record(tx: &Transaction<'_>, entry: &NewEntry) -> Result<i64, StoreError> {
    tx.prepare_cached(
        "INSERT INTO agent_audit (at, session, client, method, args, account_id, access, decision, \
         error_code, error, duration_ms) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
    )?
    .execute(params![
        entry.at,
        entry.session,
        entry.client,
        entry.method,
        entry.args,
        entry.account_id,
        entry.access.map(Access::as_str),
        entry.decision.as_str(),
        entry.error.as_ref().map(|(code, _)| code),
        entry.error.as_ref().map(|(_, message)| message),
        entry.duration_ms,
    ])?;
    Ok(tx.last_insert_rowid())
}

/// Newest first, older than `before` (an id) if given; of one account if given.
pub fn list(
    conn: &Connection,
    account_id: Option<&str>,
    before: Option<i64>,
    limit: u32,
) -> Result<Vec<Entry>, StoreError> {
    let mut statement = conn.prepare_cached(
        "SELECT id, at, session, client, method, args, account_id, access, decision, error_code, error, \
         duration_ms FROM agent_audit WHERE (?1 IS NULL OR account_id = ?1) AND id < ?2 \
         ORDER BY id DESC LIMIT ?3",
    )?;
    let rows = statement.query_map(params![account_id, before.unwrap_or(i64::MAX), limit], from_row)?;
    Ok(rows.collect::<Result<_, _>>()?)
}

/// Forget what is older than `before` (Unix ms); how many were.
pub fn prune(tx: &Transaction<'_>, before: i64) -> Result<usize, StoreError> {
    Ok(tx.execute("DELETE FROM agent_audit WHERE at < ?1", [before])?)
}

fn from_row(row: &Row<'_>) -> rusqlite::Result<Entry> {
    let access: Option<String> = row.get(7)?;
    let decision: String = row.get(8)?;
    let code: Option<i32> = row.get(9)?;
    let message: Option<String> = row.get(10)?;
    Ok(Entry {
        id: row.get(0)?,
        entry: NewEntry {
            at: row.get(1)?,
            session: row.get(2)?,
            client: row.get(3)?,
            method: row.get(4)?,
            args: row.get(5)?,
            account_id: row.get(6)?,
            access: access.as_deref().and_then(Access::parse),
            decision: Decision::parse(&decision),
            error: code.map(|code| (code, message.unwrap_or_default())),
            duration_ms: row.get(11)?,
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Store;
    use regards_core::paths::DataDirs;

    fn entry(at: i64, method: &str, account: Option<&str>, decision: Decision) -> NewEntry {
        NewEntry {
            at,
            session: "s1".into(),
            client: "mcp".into(),
            method: method.into(),
            args: "{}".into(),
            account_id: account.map(str::to_owned),
            access: account.map(|_| Access::Read),
            decision,
            error: (decision == Decision::Denied).then(|| (-32003, "not open to agents".into())),
            duration_ms: 3,
        }
    }

    #[tokio::test]
    async fn kept_newest_first_by_account_and_forgotten_in_time() {
        let temp = tempfile::tempdir().unwrap();
        let store = Store::open(&DataDirs::at(temp.path())).await.unwrap();
        store
            .write(|tx| {
                record(tx, &entry(1_000, "account.list", None, Decision::Allowed))?;
                record(tx, &entry(2_000, "message.get", Some("work"), Decision::Allowed))?;
                record(tx, &entry(3_000, "message.archive", Some("home"), Decision::Denied))?;
                record(tx, &entry(4_000, "search.query", Some("work"), Decision::Allowed))
            })
            .await
            .unwrap();
        let all = store.read(|conn| list(conn, None, None, 10)).await.unwrap();
        let methods: Vec<&str> = all.iter().map(|e| e.entry.method.as_str()).collect();
        assert_eq!(
            methods,
            ["search.query", "message.archive", "message.get", "account.list"]
        );
        assert_eq!(
            all[1].entry,
            entry(3_000, "message.archive", Some("home"), Decision::Denied),
            "read back whole"
        );

        let work = store.read(|conn| list(conn, Some("work"), None, 1)).await.unwrap();
        assert_eq!(work[0].entry.method, "search.query");
        let before = work[0].id;
        let older = store
            .read(move |conn| list(conn, Some("work"), Some(before), 10))
            .await
            .unwrap();
        assert_eq!(older.len(), 1);
        assert_eq!(older[0].entry.method, "message.get");

        assert_eq!(store.write(|tx| prune(tx, 2_500)).await.unwrap().value, 2);
        assert_eq!(store.read(|conn| list(conn, None, None, 10)).await.unwrap().len(), 2);
    }
}
