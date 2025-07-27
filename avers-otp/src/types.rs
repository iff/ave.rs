use std::fmt;

use crate::operation::Operation;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

pub type Path = String;

// This path refers to the root of an object. It is only used in 'Set'
// operations.
pub(crate) const ROOT_PATH: &str = "";

// The root object id is used for object created internally or when there
// is no applicable creator.
pub(crate) const ROOT_OBJ_ID: &str = "";

pub type RevId = i64;

// The 'RevId' which is used for the initial snapshot.
pub const ZERO_REV_ID: RevId = 0;

pub type ObjectId = String;

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
    pub fn new(object_type: ObjectType) -> Object {
        // TODO generete random id? (see Avers/Storage.hs)
        // or use firestore ids
        // TODO should we only allow to create Objects with non-optional id?
        // eg use a proxy for adding one to the db and then mutate?
        Object {
            id: None,
            object_type,
            created_at: None,
            created_by: ROOT_OBJ_ID.to_owned(),
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

impl Patch {
    pub fn new(object_id: ObjectId, author_id: String, value: &Value) -> Self {
        let op = Operation::new_set(ROOT_PATH.to_owned(), value.to_owned());
        Self {
            object_id,
            revision_id: ZERO_REV_ID,
            author_id,
            created_at: None,
            operation: op,
        }
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
            // FIXME why is this not ZERO_REV_ID?
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
