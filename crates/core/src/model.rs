use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    Triage,
    Todo,
    Scheduled,
    Ready,
    Running,
    Blocked,
    Review,
    Done,
    Archived,
    #[serde(other)]
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Task {
    pub id: String,
    pub title: String,
    pub status: Status,
    pub assignee: Option<String>,
    /// Read from the CLI JSON; used only to derive `last_updated`, never serialized.
    #[serde(default, skip_serializing)]
    pub created_at: Option<i64>,
    #[serde(default, skip_serializing)]
    pub started_at: Option<i64>,
    #[serde(default, skip_serializing)]
    pub completed_at: Option<i64>,
    /// Unix seconds of the latest known activity; set by `events::enrich`, omitted until then.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_updated: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_updated_source: Option<LastUpdatedSource>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LastUpdatedSource {
    /// Latest row of `task_events` for this task.
    Events,
    /// Fallback: max of created/started/completed timestamps (no event row found).
    Timestamps,
}

pub const COLUMNS: [Status; 6] = [
    Status::Todo,
    Status::Ready,
    Status::Running,
    Status::Blocked,
    Status::Review,
    Status::Done,
];

#[derive(Debug, Serialize)]
pub struct Column<'a> {
    pub status: Status,
    pub tasks: Vec<&'a Task>,
}

/// Retains source order within each visible column, including empty columns.
pub fn group_by_column(tasks: &[Task]) -> Vec<Column<'_>> {
    COLUMNS
        .into_iter()
        .map(|status| Column {
            status,
            tasks: tasks.iter().filter(|task| task.status == status).collect(),
        })
        .collect()
}
