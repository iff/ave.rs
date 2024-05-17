use firestore::FirestoreTimestamp;
use serde::{Deserialize, Serialize};
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
const ROOT_OBJ_ID: &str = "";

type RevId = i64;

// The 'RevId' which is used for the initial snapshot.
const ZERO_REV_ID: RevId = 0;

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
pub enum Operation<T> {
    /// applied to objects for adding, updating and inserting multiple elements in a single op
    Set { path: Path, value: Option<T> },

    /// manipulate arrays (remove, insert multiple elements in a single op
    Splice {
        path: Path,
        index: u32,
        remove: u32,
        insert: Vec<T>,
    },
}

#[derive(Serialize, Deserialize, Clone)]
pub struct Object {
    id: ObjId,
    object_type: String,
    created_at: firestore::FirestoreTimestamp,
    created_by: ObjId,
    deleted: Option<bool>,
}

impl Pk for Object {
    fn to_pk(&self) -> String {
        self.id.to_string()
    }
}

#[derive(Serialize, Deserialize, Clone)]
pub struct Patch<T> {
    pub object_id: ObjectId,
    pub revision_id: RevId,
    pub author_id: ObjId,
    pub created_at: firestore::FirestoreTimestamp,
    pub operation: Operation<T>,
}

impl<T> Pk for Patch<T> {
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
