use std::fmt;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::error::Error;

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
    EmptyPath(),
    ValueIsNotArray(),
}

impl Error for OtError {}

impl fmt::Display for OtError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyPath() => write!(f, "NoId"),
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
            Operation::Set { path, value } => write!(f, "Set: {}, value={:?}", path, value),
            Operation::Splice {
                path,
                index,
                remove,
                insert,
            } => write!(
                f,
                "Splice: {} @ {}, remove={}, insert={}",
                path, index, remove, insert
            ),
        }
    }
}

impl Operation {
    pub fn try_new_set(path: impl Into<Path>, value: Option<Value>) -> Result<Self, OtError> {
        let path = path.into();
        if path.is_empty() {
            Err(OtError::EmptyPath())
        } else {
            Ok(Self::Set { path, value })
        }
    }

    pub fn try_new_splice(
        path: impl Into<Path>,
        index: usize,
        remove: usize,
        insert: Value,
    ) -> Result<Self, OtError> {
        let path = path.into();
        if path.is_empty() {
            Err(OtError::EmptyPath())
        } else if insert.is_array() {
            Err(OtError::ValueIsNotArray())
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

#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Object {
    #[serde(alias = "_firestore_id")]
    id: Option<ObjectId>, // not nice that this has to be empty for id generation to work
    #[serde(alias = "_firestore_created")]
    pub created_at: Option<DateTime<Utc>>, // Option<FirestoreTimestamp>,
    pub object_type: ObjectType, // TODO we should pass those as a template?
    pub created_by: ObjectId,
    // delete the object which has a very different meaning from deleting a boulder
    pub deleted: Option<bool>,
}

// here we fix types to those instead of doing a generic str to type "cast"
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum ObjectType {
    Account,
    Boulder,
    Passport,
}

impl Object {
    pub fn new(object_type: ObjectType, created_by: ObjectId) -> Object {
        // TODO generete random id? (see Avers/Storage.hs)
        // or use firestore ids
        // TODO should we only allow to create Objects with non-optional id?
        // eg use a proxy for adding one to the db and then mutate?
        Object {
            id: None,
            object_type,
            created_at: None,
            created_by,
            deleted: None,
        }
    }

    pub fn id(&self) -> String {
        // FIXME
        self.id.as_ref().expect("no id").to_string()
    }

    pub fn get_type(&self) -> ObjectType {
        self.object_type.clone()
    }
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Patch {
    pub object_id: ObjectId,
    pub revision_id: RevId,
    pub author_id: ObjectId,
    #[serde(alias = "_firestore_created")]
    pub created_at: Option<DateTime<Utc>>, //Option<FirestoreTimestamp>,
    pub operation: Operation,
}

impl fmt::Display for Patch {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Patch: {}@{} ops={}",
            self.object_id, self.revision_id, self.operation
        )
    }
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    pub object_id: ObjectId,
    pub revision_id: RevId,
    pub content: Value,
}

impl Snapshot {
    pub fn new(object_id: ObjectId) -> Self {
        Self {
            object_id,
            revision_id: -1,
            content: json!({}),
        }
    }
}

impl fmt::Display for Snapshot {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Snapshot: {}@{} content={}",
            self.object_id, self.revision_id, self.content
        )
    }
}

#[cfg(test)]
mod tests {
    // use super::*;
    // use serde_json::{from_str, to_string, Value};

    // #[test]
    // fn object_additional_fields_as_value() {
    //     let object = Object::new(ObjectType::Boulder, String::from("deadbeef"));
    //     let json = to_string(&object).unwrap();
    //
    //     // I think the only way to handle custom keys on the Object is to actually parse it as a
    //     // Value..
    //     // but I'm pretty sure we need to have this in a typed manner, eg as
    //     //   Either<Vec, key/value tuple>
    //     // ?
    //     match from_str::<Value>(&json[..]) {
    //         Ok(o) => {
    //             if o.get("grade").is_some() {
    //                 panic!("grade should be none")
    //             }
    //         }
    //         Err(e) => {
    //             panic!("{}", e);
    //         }
    //     }
    // }

    // #[test]
    // fn object_additional_fields_using_extra() {
    //     let mut object = Object::new(ObjectType::Boulder, String::from("deadbeef"));
    //     object
    //         .content
    //         .insert(String::from("grade"), Value::String(String::from("blue")));
    //
    //     let json = to_string(&object).unwrap();
    //     match serde_json::from_str::<Object>(&json[..]) {
    //         Ok(o) => o.content.get("grade").expect("should have grade field"),
    //         Err(e) => panic!("{}", e),
    //     };
    // }
}
