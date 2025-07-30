use std::error::Error;
use std::fmt;

pub mod operation;
pub mod patching;
pub mod types;

pub use crate::operation::Operation;
pub use crate::patching::rebase;
pub use crate::types::{Object, ZERO_REV_ID};

#[derive(Debug)]
pub enum OtError {
    Index(String),
    InvalidSetOp(),
    Key(String),
    NoId(),
    Operation(String),
    Path(String),
    Rebase(String),
    Type(String),
    ValueIsNotArray(),
}

impl Error for OtError {}

impl fmt::Display for OtError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Index(e) => write!(f, "IndexError: {e}"),
            Self::InvalidSetOp() => write!(f, "NoId"),
            Self::Key(e) => write!(f, "KeyError: {e}"),
            Self::NoId() => write!(f, "NoId"),
            Self::Operation(e) => write!(f, "Operation: {e}"),
            Self::Path(e) => write!(f, "PathError: {e}"),
            Self::Rebase(e) => write!(f, "Rebase: {e}"),
            Self::Type(e) => write!(f, "TypeError: {e}"),
            Self::ValueIsNotArray() => write!(f, "ValueIsNotArray"),
        }
    }
}
