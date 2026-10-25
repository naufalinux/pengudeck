use crate::model::Task;
use std::{error::Error, fmt};

/// Deliberately carries no source payload, local paths, or stderr.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceError {
    InvalidData,
}

impl fmt::Display for SourceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidData => f.write_str("invalid task source data"),
        }
    }
}

impl Error for SourceError {}

pub fn parse_list(input: &str) -> Result<Vec<Task>, SourceError> {
    serde_json::from_str(input).map_err(|_| SourceError::InvalidData)
}
