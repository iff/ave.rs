use firestore::FirestoreTimestamp;
use serde::{Deserialize, Serialize};

trait toPk {
    fn as_str(&self) -> String;
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
impl toPk for ObjectId {
    fn as_str(&self) -> String {
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
