use crate::model::Task;
use std::{error::Error, fmt, path::PathBuf, process::Command};

/// Deliberately carries no source payload, local paths, or stderr.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceError {
    InvalidData,
    Unavailable,
}

impl fmt::Display for SourceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidData => f.write_str("invalid task source data"),
            Self::Unavailable => f.write_str("task source unavailable"),
        }
    }
}

impl Error for SourceError {}

pub fn parse_list(input: &str) -> Result<Vec<Task>, SourceError> {
    serde_json::from_str(input).map_err(|_| SourceError::InvalidData)
}

pub trait TaskSource {
    fn list(&self, board: &str) -> Result<Vec<Task>, SourceError>;
}

pub struct CliSource {
    pub hermes_bin: PathBuf,
}

impl TaskSource for CliSource {
    /// Blocking process invocation; async callers must isolate this operation.
    fn list(&self, board: &str) -> Result<Vec<Task>, SourceError> {
        let output = Command::new(&self.hermes_bin)
            .args(["kanban", "--board", board, "list", "--json"])
            .output()
            .map_err(|_| SourceError::Unavailable)?;
        if !output.status.success() {
            return Err(SourceError::Unavailable);
        }
        let stdout = std::str::from_utf8(&output.stdout).map_err(|_| SourceError::InvalidData)?;
        parse_list(stdout)
    }
}
