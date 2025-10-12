use std::error::Error;
use std::fmt;

mod operation;
mod path;
mod rebase;

pub use crate::operation::Operation;
pub use crate::rebase::rebase;

pub type Path = String;

// This path refers to the root of an object. It is only used in 'Set'
// operations.
pub const ROOT_PATH: &str = "";

// The root object id is used for object created internally or when there
// is no applicable creator.
pub const ROOT_OBJ_ID: &str = "";

pub type RevId = i64;

// The 'RevId' which is used for the initial snapshot.
pub const ZERO_REV_ID: RevId = 0;

pub type ObjectId = String;

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
