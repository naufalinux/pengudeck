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
}
