use std::fmt;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
// use std::collections::HashMap;

/// converts to database primary key. this is needed when updating the
/// view tables (to get a pk to merge/update/delete/..)
pub trait Pk {
    fn to_pk(&self) -> String;
}

pub type Path = String;

// This path refers to the root of an object. It is only used in 'Set'
// operations.
pub const ROOT_PATH: &str = "";

pub type ObjId = String;

// The root object id is used for object created internally or when there
// is no applicable creator.
pub const ROOT_OBJ_ID: &str = "";

pub type RevId = i64;

// The 'RevId' which is used for the initial snapshot.
pub const ZERO_REV_ID: RevId = 0;

// TODO this is not the firestore id
// TODO can't be internally typed (tuple), so externally okay?
#[derive(Serialize, Deserialize, Clone)]
pub enum ObjectId {
    /// The base object whose snapshots contain the actual content.
    Base(ObjId),
    /// Object which contains authorization rules.
    Authorization(ObjId),
}

// TODO parsing?
impl Pk for ObjectId {
    fn to_pk(&self) -> String {
        match self {
            ObjectId::Base(obj_id) => obj_id.to_string(),
            ObjectId::Authorization(obj_id) => obj_id.to_string() + "/authorization",
        }
    }
}

// pub fn objectIdParser(objId: String) -> ObjectId {
//     match objId.chars().next() {
//         Some(char::is_alphanumeric) => {
//             todo!()
//         }
//     }
// }

#[derive(Serialize, Deserialize, Clone, PartialEq)]
#[serde(tag = "type")]
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

#[derive(Serialize, Deserialize, Clone)]
pub struct Object {
    #[serde(alias = "_firestore_id")]
    id: Option<ObjId>, // not nice that this has to be empty for id generation to work
    #[serde(alias = "_firestore_created")]
    pub created_at: Option<DateTime<Utc>>, // Option<FirestoreTimestamp>,
    pub object_type: ObjectType,
    pub created_by: ObjId,
    // delete the object which has a very different meaning from deleting a boulder
    pub deleted: Option<bool>,
}

// here we fix types to those instead of doing a generic str to type "cast"
#[derive(Serialize, Deserialize, Clone, Debug)]
pub enum ObjectType {
    Account,
    Boulder,
}

impl Object {
    pub fn new(object_type: ObjectType, created_by: ObjId) -> Object {
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

impl Pk for Object {
    fn to_pk(&self) -> String {
        self.id.as_ref().expect("").to_string()
    }
}

#[derive(Serialize, Deserialize, Clone)]
pub struct Patch {
    pub object_id: ObjectId,
    pub revision_id: RevId,
    pub author_id: ObjId,
    #[serde(alias = "_firestore_created")]
    pub created_at: Option<DateTime<Utc>>, //Option<FirestoreTimestamp>,
    pub operation: Operation,
}

impl Pk for Patch {
    fn to_pk(&self) -> String {
        self.object_id.to_pk() + "@" + &self.revision_id.to_string()[..]
    }
}

#[derive(Serialize, Deserialize, Clone)]
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

impl Pk for Snapshot {
    fn to_pk(&self) -> String {
        self.object_id.to_pk() + "@" + &self.revision_id.to_string()[..]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{from_str, to_string, Value};

    #[test]
    fn object_additional_fields_as_value() {
        let object = Object::new(ObjectType::Boulder, String::from("deadbeef"));
        let json = to_string(&object).unwrap();

        // I think the only way to handle custom keys on the Object is to actually parse it as a
        // Value..
        // but I'm pretty sure we need to have this in a typed manner, eg as
        //   Either<Vec, key/value tuple>
        // ?
        match from_str::<Value>(&json[..]) {
            Ok(o) => {
                if o.get("grade").is_some() {
                    panic!("grade should be none")
                }
            }
            Err(e) => {
                panic!("{}", e);
            }
        }
    }

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
