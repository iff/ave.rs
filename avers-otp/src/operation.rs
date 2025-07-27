use crate::types::Path;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::error::Error;
use std::fmt;

#[derive(Debug)]
pub enum OperationError {
    InvalidSetOp(),
    ValueIsNotArray(),
}

impl Error for OperationError {}

impl fmt::Display for OperationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidSetOp() => write!(f, "NoId"),
            Self::ValueIsNotArray() => write!(f, "ValueIsNotArray"),
        }
    }
}

// TODO hide behind struct to disallow use outside?
// TODO serde serializer also needs to check
#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
#[serde(tag = "type")]
#[serde(rename_all = "camelCase")]
pub enum Operation {
    /// applied to Value::Object for adding, updating and inserting multiple elements in a single op
    Set { path: Path, value: Option<Value> },

    /// manipulate Value::Array (remove, insert multiple elements in a single op) mimicing js/rust splice
    /// implementation
    Splice {
        path: Path,
        index: usize,
        remove: usize,
        /// this is actually a Value::Array, everything else will result in an error
        insert: Value,
    },
}

impl fmt::Display for Operation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Operation::Set { path, value } => write!(f, "Set: {path}, value={value:?}"),
            Operation::Splice {
                path,
                index,
                remove,
                insert,
            } => write!(
                f,
                "Splice: {path} @ {index}, remove={remove}, insert={insert}"
            ),
        }
    }
}

impl Operation {
    pub fn new_set(path: impl Into<Path>, value: Value) -> Self {
        Self::Set {
            path: path.into(),
            value: Some(value),
        }
    }

    pub fn try_new_set(
        path: impl Into<Path>,
        value: Option<Value>,
    ) -> Result<Self, OperationError> {
        let path = path.into();
        if path.is_empty() && value.is_none() {
            Err(OperationError::InvalidSetOp())
        } else {
            Ok(Self::Set { path, value })
        }
    }

    pub fn try_new_splice(
        path: impl Into<Path>,
        index: usize,
        remove: usize,
        insert: Value,
    ) -> Result<Self, OperationError> {
        let path = path.into();
        if insert.is_array() {
            Err(OperationError::ValueIsNotArray())
        } else {
            Ok(Self::Splice {
                path,
                index,
                remove,
                insert,
            })
        }
    }

    pub fn path(&self) -> Path {
        match self {
            Operation::Set { path, value: _ } => path.to_owned(),
            Operation::Splice {
                path,
                index: _,
                remove: _,
                insert: _,
            } => path.to_owned(),
        }
    }

    pub fn path_contains(&self, p: impl Into<Path>) -> bool {
        let p = p.into();
        match self {
            Operation::Set { path, value: _ } => path.contains(&p),
            Operation::Splice {
                path,
                index: _,
                remove: _,
                insert: _,
            } => path.contains(&p),
        }
    }
}
