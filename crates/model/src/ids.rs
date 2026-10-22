use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum IdError {
    #[error("seat id must look like pod.member@rig, got {0:?}")]
    Shape(String),
    #[error("invalid id segment {0:?}: use letters, digits, '_' or '-'")]
    Segment(String),
}

/// A pod, member or rig id segment: non-empty ASCII letters, digits, `_` or `-`.
#[must_use]
pub fn valid_segment(s: &str) -> bool {
    !s.is_empty()
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

/// Stable seat address `pod.member@rig`. Runtime handles (pids, native session ids)
/// are never part of it.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct SeatId {
    pub pod: String,
    pub member: String,
    pub rig: String,
}

impl SeatId {
    /// Builds an address, rejecting empty segments and characters outside ASCII letters,
    /// digits, `_`, and `-`.
    pub fn try_new(pod: &str, member: &str, rig: &str) -> Result<Self, IdError> {
        for s in [pod, member, rig] {
            if !valid_segment(s) {
                return Err(IdError::Segment(s.to_string()));
            }
        }
        Ok(Self {
            pod: pod.into(),
            member: member.into(),
            rig: rig.into(),
        })
    }

    /// `pod.member`, the id used inside a rig spec.
    #[must_use]
    pub fn logical(&self) -> String {
        format!("{}.{}", self.pod, self.member)
    }
}

impl fmt::Display for SeatId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}@{}", self.pod, self.member, self.rig)
    }
}

impl FromStr for SeatId {
    type Err = IdError;
    fn from_str(s: &str) -> Result<Self, IdError> {
        let (logical, rig) = s.split_once('@').ok_or_else(|| IdError::Shape(s.into()))?;
        let (pod, member) = logical
            .split_once('.')
            .ok_or_else(|| IdError::Shape(s.into()))?;
        Self::try_new(pod, member, rig)
    }
}

impl TryFrom<String> for SeatId {
    type Error = IdError;
    fn try_from(s: String) -> Result<Self, IdError> {
        s.parse()
    }
}

impl From<SeatId> for String {
    fn from(id: SeatId) -> String {
        id.to_string()
    }
}
