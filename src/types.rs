use firestore::FirestoreTimestamp;
use serde::{Deserialize, Serialize};

/// converts to database primary key
trait Pk {
    fn toPk(&self) -> String;
}

type ObjId = String;

type RevId = u64;

#[derive(Serialize, Deserialize, Clone)]
pub enum RevId {
    User = "user",
    Setter = "setter",
    Admin = "admin",
}

#[derive(Serialize, Deserialize, Clone)]
pub enum ObjectId {
    BaseObjectId(ObjId),
    ReleaseObjectId(ObjId, RevId),
    AuthorizationObjectId(ObjId),
}

// TODO parsing?
impl Pk for ObjectId {
    fn toPk(&self) -> String {
        match self {
            BaseObjectId(objId) => objId,
            ReleaseObjectId(objId, revId) => objId + "/release/" + revId,
            AuthorizationObjectId(objId) => objId + "/authorization",
        }
    }
}

#[derive(Serialize, Deserialize, Clone)]
pub struct Patch {
    pub objectId: ObjId,
    pub revisionId: RevId,
    pub authorId: ObjId,
    pub createdAt: firestore::FirestoreTimestamp,
    pub operation: Operation,
}

#[derive(Serialize, Deserialize, Clone)]
pub enum Operation<T> {
    /// applied to objects for adding, updating and inserting multiple elements in a single op
    Set { path: Path, value: Option<T> },

    /// manipulate arrays (remove, insert multiple elements in a single op
    Splice {
        path: Path,
        index: u32,
        remove: u32,
        insert: std::Vec<T>,
    },
}

#[derive(Serialize, Deserialize, Clone)]
pub struct Object {
    id: ObjId,
    objectType: String,
    created_at: firestore::FirestoreTimestamp,
    created_by: ObjId,
    deleted: Option<bool>,
}

impl Pk for Object {
    fn toPk(&self) -> String {
        self.id
    }
}
