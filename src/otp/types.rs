use firestore::FirestoreTimestamp;
use serde::{Deserialize, Serialize};
use serde_json::{from_str, json, Error, Value};
use std::collections::HashMap;
use std::vec::Vec;

/// converts to database primary key
trait Pk {
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
    BaseObjectId(ObjId),
    /// An object describing a particualar release of the base object.
    ReleaseObjectId(ObjId, RevId),
    /// Object which contains authorization rules.
    AuthorizationObjectId(ObjId),
}

// TODO parsing?
impl Pk for ObjectId {
    fn to_pk(&self) -> String {
        match self {
            ObjectId::BaseObjectId(obj_id) => obj_id.to_string(),
            ObjectId::ReleaseObjectId(obj_id, rev_id) => {
                obj_id.to_string() + "/release/" + &rev_id.to_string()[..]
            }
            ObjectId::AuthorizationObjectId(obj_id) => obj_id.to_string() + "/authorization",
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

#[derive(Serialize, Deserialize, Clone)]
pub enum Operation {
    /// applied to objects for adding, updating and inserting multiple elements in a single op
    Set { path: Path, value: Option<Value> },

    /// manipulate arrays (remove, insert multiple elements in a single op)
    Splice {
        path: Path,
        index: usize,
        remove: usize,
        insert: Vec<Value>,
    },
}

#[derive(Serialize, Deserialize, Clone)]
pub struct Object {
    id: ObjId,
    object_type: String,
    created_at: firestore::FirestoreTimestamp,
    created_by: ObjId,
    deleted: Option<bool>,

    // data - is there a better way to map?
    // we know its a value or an array?
    // #[serde(flatten)]
    pub content: HashMap<String, Value>,
    // pub content: Option<HashMap<String, Value>>,
}

impl Pk for Object {
    fn to_pk(&self) -> String {
        self.id.to_string()
    }
}

#[derive(Serialize, Deserialize, Clone)]
pub struct Patch {
    pub object_id: ObjectId,
    pub revision_id: RevId,
    pub author_id: ObjId,
    pub created_at: firestore::FirestoreTimestamp,
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
    use serde_json::{from_str, json, to_string, Error, Value};

    #[test]
    fn object_additional_fields_as_value() {
        let object = Object {
            id: String::from("fa21ea12c"),
            object_type: String::from("value"),
            created_at: firestore::FirestoreTimestamp(chrono::Utc::now()),
            created_by: String::from("deadbeef"),
            deleted: None,
            content: HashMap::new(),
        };

        let json = to_string(&object).unwrap();

        // I think the only way to handle custom keys on the Object is to actually parse it as a
        // Value..
        // but I'm pretty sure we need to have this in a typed manner, eg as
        //   Either<Vec, key/value tuple>
        // ?
        match from_str::<Value>(&json[..]) {
            Ok(o) => {
                println!("id: {:#?}", o["id"].as_str());
                match o.get("grade") {
                    Some(g) => println!("grade: {:#?}", g),
                    None => println!("grade: none"),
                }
            }
            Err(e) => {
                println!("Error {:#?}", e);
                panic!();
            }
        }
    }

    #[test]
    fn object_additional_fields_using_extra() {
        let created_at = firestore::FirestoreTimestamp(chrono::Utc::now());
        let mut extra = HashMap::new();
        extra.insert(String::from("grade"), Value::String(String::from("blue")));
        let object = Object {
            id: String::from("fa21ea12c"),
            object_type: String::from("value"),
            created_at,
            created_by: String::from("deadbeef"),
            deleted: None,
            content: extra,
        };

        let json = to_string(&object).unwrap();
        println!("{}", json);

        match serde_json::from_str::<Object>(&json[..]) {
            Ok(o) => {
                println!("id: {:#?}", o.id);
                println!("grade: {}", o.content["grade"].as_str().expect("none"))
            }
            Err(e) => {
                println!("Error {:#?}", e);
                panic!();
            }
        }
    }
}
