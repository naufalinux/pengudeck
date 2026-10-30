//! Read-only access to Hermes kanban event history, used to derive "last updated".
//!
//! Database layout follows the Hermes CLI: the `default` board lives at
//! `<root>/kanban.db`, every other board at `<root>/kanban/boards/<slug>/kanban.db`.
//! The database is opened read-only and is never created by this module.

use crate::{
    model::{LastUpdatedSource, Task},
    source::SourceError,
};
use rusqlite::{Connection, OpenFlags};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};

/// Hermes state root: `$HERMES_HOME` if set, otherwise `~/.hermes`.
pub fn hermes_root() -> PathBuf {
    if let Some(home) = std::env::var_os("HERMES_HOME") {
        return PathBuf::from(home);
    }
    let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default();
    home.join(".hermes")
}

/// Returns the kanban database path for `board`, or `None` for an unsafe slug.
pub fn board_db_path(hermes_root: &Path, board: &str) -> Option<PathBuf> {
    let safe = !board.is_empty()
        && board
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'_' || c == b'-');
    if !safe {
        return None;
    }
    Some(if board == "default" {
        hermes_root.join("kanban.db")
    } else {
        hermes_root.join("kanban/boards").join(board).join("kanban.db")
    })
}

/// Latest `created_at` per task id from `task_events`. Read-only; never creates the file.
pub fn latest_event_times(db: &Path) -> Result<HashMap<String, i64>, SourceError> {
    let conn = Connection::open_with_flags(
        db,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(|_| SourceError::Unavailable)?;
    let mut stmt = conn
        .prepare("SELECT task_id, MAX(created_at) FROM task_events GROUP BY task_id")
        .map_err(|_| SourceError::InvalidData)?;
    let rows = stmt
        .query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?)))
        .map_err(|_| SourceError::InvalidData)?;
    rows.collect::<Result<HashMap<_, _>, _>>()
        .map_err(|_| SourceError::InvalidData)
}

/// Fills `last_updated` and its source on every task.
///
/// The value is the newest of the latest event and the created/started/completed
/// timestamps. If the database cannot be read, the timestamps alone are used, so the
/// dashboard keeps working; the source field then reports `timestamps`.
pub fn enrich(tasks: &mut [Task], board: &str, hermes_root: &Path) {
    let events = board_db_path(hermes_root, board)
        .and_then(|db| latest_event_times(&db).ok());
    for task in tasks {
        let timestamp = [task.created_at, task.started_at, task.completed_at]
            .into_iter()
            .flatten()
            .max();
        let event = events.as_ref().and_then(|map| map.get(&task.id).copied());
        let (value, source) = match (event, timestamp) {
            (Some(e), Some(t)) if e >= t => (Some(e), LastUpdatedSource::Events),
            (Some(e), None) => (Some(e), LastUpdatedSource::Events),
            (_, Some(t)) => (Some(t), LastUpdatedSource::Timestamps),
            (None, None) => (None, LastUpdatedSource::Timestamps),
        };
        task.last_updated = value;
        task.last_updated_source = value.map(|_| source);
    }
}
