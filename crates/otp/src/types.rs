use std::fmt;

use crate::operation::Operation;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

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

// TODO most of these types should go to a storage crate, only Patch is needed in rebase and we
// need to find a way, eg apply trait that Patch implements
// Object storage representation - used for Firestore serialization
#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ObjectDoc {
    #[serde(alias = "_firestore_id")]
    pub id: Option<ObjectId>,
    #[serde(alias = "_firestore_created")]
    pub created_at: Option<DateTime<Utc>>,
    pub object_type: ObjectType,
    pub created_by: ObjectId,
    pub deleted: Option<bool>,
}

impl ObjectDoc {
    pub fn new(object_type: ObjectType) -> Self {
        Self {
            id: None,
            object_type,
            created_at: None,
            created_by: ROOT_OBJ_ID.to_owned(),
            deleted: None,
        }
    }
}

impl fmt::Display for ObjectDoc {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.id {
            None => write!(f, "ObjectDoc: no id {}", self.object_type),
            Some(id) => write!(f, "ObjectDoc: {id} {}", self.object_type),
        }
    }
}

pub struct Object {
    pub id: ObjectId,
    pub created_at: DateTime<Utc>,
    pub object_type: ObjectType,
    pub created_by: ObjectId,
    pub deleted: bool,
}

impl TryFrom<ObjectDoc> for Object {
    type Error = &'static str;

    fn try_from(doc: ObjectDoc) -> Result<Self, Self::Error> {
        Ok(Object {
            id: doc.id.ok_or("Object missing id")?,
            created_at: doc.created_at.ok_or("Object missing created_at")?,
            object_type: doc.object_type,
            created_by: doc.created_by,
            deleted: doc.deleted.unwrap_or(false),
        })
    }
}

impl fmt::Display for Object {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Object: {} {}", self.id, self.object_type)
    }
}

impl Object {
    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn get_type(&self) -> ObjectType {
        self.object_type.clone()
    }
}

// here we fix types to those instead of doing a generic str to type "cast"
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum ObjectType {
    Account,
    Boulder,
    Passport,
}

impl fmt::Display for ObjectType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ObjectType::Account => write!(f, "type=account"),
            ObjectType::Boulder => write!(f, "type=boulder"),
            ObjectType::Passport => write!(f, "type=passport"),
        }
    }
}

#[derive(Serialize, Deserialize)]
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
