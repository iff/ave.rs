use firestore::FirestoreTimestamp;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;

/// converts to database primary key
pub trait Pk {
    fn to_pk(&self) -> String;
}

pub type Path = String;

// This path refers to the root of an object. It is only used in 'Set'
// operations.
pub const ROOT_PATH: &str = "";

type ObjId = String;

// The root object id is used for object created internally or when there
// is no applicable creator.
pub const ROOT_OBJ_ID: &str = "";

type RevId = i64;

// The 'RevId' which is used for the initial snapshot.
pub const ZERO_REV_ID: RevId = 0;

// #[derive(Serialize, Deserialize, Clone)]
// pub enum RevId {
//     User = "user",
//     Setter = "setter",
//     Admin = "admin",
// }

#[derive(Serialize, Deserialize, Clone)]
pub enum ObjectId {
    /// The base object whose snapshots contain the actual content.
    Base(ObjId),
    /// An object describing a particualar release of the base object.
    Release(ObjId, RevId),
    /// Object which contains authorization rules.
    Authorization(ObjId),
}

// TODO parsing?
impl Pk for ObjectId {
    fn to_pk(&self) -> String {
        match self {
            ObjectId::Base(obj_id) => obj_id.to_string(),
            ObjectId::Release(obj_id, rev_id) => {
                obj_id.to_string() + "/release/" + &rev_id.to_string()[..]
            }
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

// $(deriveEncoding (deriveJSONOptions "op"){
//     omitNothingFields = True,
//     sumEncoding       = TaggedObject "type" "content"
// } ''Operation)

#[derive(Serialize, Deserialize, Clone, PartialEq)]
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

#[derive(Serialize, Deserialize, Clone)]
pub struct Object {
    #[serde(alias = "_firestore_id")]
    id: Option<ObjId>, // not nice that this has to be empty for id generation to work
    #[serde(alias = "_firestore_created")]
    created_at: Option<FirestoreTimestamp>,
    object_type: String,
    created_by: ObjId,
    deleted: Option<bool>,

    // flatten or not? keeping things in content helps with clashes
    // maybe an option?
    pub content: HashMap<String, Value>,
}

impl Object {
    pub fn new(object_type: String, created_by: ObjId) -> Object {
        // TODO generete random id? (see Avers/Storage.hs)
        // or use firestore ids
        // TODO should we only allow to create Objects that are already persisted?
        Object {
            id: None,
            object_type,
            created_at: None,
            created_by,
            deleted: None,
            content: HashMap::new(),
        }
    }

    pub fn id(&self) -> String {
        // FIXME
        self.id.as_ref().expect("no id").to_string()
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
    pub created_at: FirestoreTimestamp,
    pub operation: Operation,
}

impl Pk for Patch {
    fn to_pk(&self) -> String {
        self.object_id.to_pk() + "@" + &self.revision_id.to_string()[..]
    }
}

#[derive(Serialize, Deserialize, Clone)]
pub struct Snapshot<T> {
    pub object_id: ObjectId,
    pub revision_id: RevId,
    pub content: T,
}

// impl<T> Snapshot<T> {
//     pub fn new(objectId: ObjectId) -> Self {
//         Self {
//             objectId,
//             revisionId: -1,
//             content: (), // FIXME Aeson.emptyObject
//         }
//     }
// }

impl<T> Pk for Snapshot<T> {
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
        let object = Object::new(String::from("value"), String::from("deadbeef"));
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

    #[test]
    fn object_additional_fields_using_extra() {
        let mut object = Object::new(String::from("value"), String::from("deadbeef"));
        object
            .content
            .insert(String::from("grade"), Value::String(String::from("blue")));

        let json = to_string(&object).unwrap();
        match serde_json::from_str::<Object>(&json[..]) {
            Ok(o) => o.content.get("grade").expect("should have grade field"),
            Err(e) => panic!("{}", e),
        };
    }
}
