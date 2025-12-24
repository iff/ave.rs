use std::fmt;

use chrono::{DateTime, Utc};
use otp::{ObjectId, Operation, OtError, RevId};
use serde::{Deserialize, Serialize};
use serde_json::{Value, from_value, json};

use crate::{AppError, AppState};

macro_rules! store {
    ($state:expr, $gym:expr, $entity:expr, $collection:expr) => {{
        let parent_path = $state.db.parent_path("gyms", $gym)?;
        let result = $state
            .db
            .fluent()
            .insert()
            .into($collection)
            .generate_document_id()
            .parent(&parent_path)
            .object($entity)
            .execute()
            .await?;

        match &result {
            Some(r) => tracing::debug!("storing: {r}"),
            None => tracing::warn!("failed to store: {}", $entity),
        }

        result
    }};
}

// TODO implement Arbitrary for types

#[derive(Serialize, Deserialize, Clone, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum AccountRole {
    User,
    Setter,
    Admin,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Account {
    #[serde(alias = "_firestore_id")]
    pub id: Option<String>,
    // TODO this is not used - remove
    pub login: String,
    pub role: AccountRole,
    pub email: String,
    #[serde(with = "firestore::serialize_as_null")]
    pub name: Option<String>,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Boulder {
    #[serde(alias = "_firestore_id")]
    pub id: Option<String>,
    pub setter: Vec<ObjectId>,
    pub sector: String,
    pub grade: String,
    grade_nr: u32,
    /// set date as epoch timestamp in millis
    pub set_date: usize,
    // #[serde(with = "firestore::serialize_as_null")]
    // pub removed: Option<usize>,
    /// removed date as epoch timestamp in millis, 0 means not removed yet
    pub removed: usize,
    // #[serde(with = "firestore::serialize_as_null")]
    // pub is_draft: Option<usize>,
    pub is_draft: usize,
    name: String,
    // name: Option<String>,
}

impl fmt::Display for Boulder {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Boulder: {}",
            serde_json::to_string_pretty(self).expect("serialisation should not fail")
        )
    }
}

impl Boulder {
    pub fn in_setter(&self, setter: &ObjectId) -> bool {
        self.setter.contains(setter)
    }
}

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
    pub const COLLECTION: &str = "objects";

    pub fn new(object_type: ObjectType) -> Self {
        Self {
            id: None,
            object_type,
            created_at: None,
            created_by: otp::ROOT_OBJ_ID.to_owned(),
            deleted: None,
        }
    }

    pub async fn store(&self, state: &AppState, gym: &String) -> Result<Self, AppError> {
        let s: Option<Self> = store!(state, gym, self, Self::COLLECTION);
        s.ok_or(AppError::Query("storing object failed".to_string()))
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
    #[allow(dead_code)]
    pub deleted: bool,
}

impl TryFrom<ObjectDoc> for Object {
    type Error = AppError;

    fn try_from(doc: ObjectDoc) -> Result<Self, Self::Error> {
        Ok(Object {
            id: doc
                .id
                .ok_or(AppError::Query("object doc is missing an id".to_string()))?,
            created_at: doc.created_at.ok_or(AppError::Query(
                "object doc is missing created_at".to_string(),
            ))?,
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
    pub const COLLECTION: &str = "patches";

    pub fn new(object_id: ObjectId, author_id: String, value: &Value) -> Self {
        let op = Operation::new_set(otp::ROOT_PATH.to_owned(), value.to_owned());
        Self {
            object_id,
            revision_id: otp::ZERO_REV_ID,
            author_id,
            created_at: None,
            operation: op,
        }
    }

    pub fn new_revision(
        revision_id: RevId,
        object_id: ObjectId,
        author_id: String,
        operation: Operation,
    ) -> Self {
        Self {
            object_id,
            revision_id,
            author_id,
            created_at: None,
            operation,
        }
    }

    pub async fn store(&self, state: &AppState, gym: &String) -> Result<Self, AppError> {
        let s: Option<Self> = store!(state, gym, self, Self::COLLECTION);
        s.ok_or(AppError::Query("storing patch failed".to_string()))
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
    pub const COLLECTION: &str = "snapshots";

    pub fn new(object_id: ObjectId) -> Self {
        Self {
            object_id,
            // FIXME why is this not ZERO_REV_ID?
            revision_id: -1,
            content: json!({}),
        }
    }

    pub fn new_revision(
        &self,
        object_id: ObjectId,
        author_id: ObjectId,
        operation: Operation,
    ) -> Result<Option<(Self, Patch)>, OtError> {
        assert_eq!(object_id, self.object_id);

        let content = operation.apply_to(self.content.to_owned())?;
        if content == self.content {
            tracing::debug!("skipping save operation: content did not change");
            return Ok(None);
        }

        let revision_id = self.revision_id + 1;
        let patch = Patch::new_revision(revision_id, object_id.clone(), author_id, operation);
        Ok(Some((
            Self {
                object_id,
                revision_id,
                content,
            },
            patch,
        )))
    }

    fn apply_patch(&self, patch: &Patch) -> Result<Self, AppError> {
        // tracing::debug!("applying patch={patch} to {snapshot} results in snapshot={s}");
        Ok(Self {
            object_id: self.object_id.to_owned(),
            revision_id: patch.revision_id,
            content: patch.operation.apply_to(self.content.clone())?,
        })
    }

    pub fn apply_patches(&self, patches: &Vec<Patch>) -> Result<Self, AppError> {
        let mut s = self.clone();
        for patch in patches {
            s = s.apply_patch(patch)?;
        }

        Ok(s)
    }

    pub async fn store(&self, state: &AppState, gym: &String) -> Result<Self, AppError> {
        let s: Option<Snapshot> = store!(state, gym, self, Self::COLLECTION);
        s.ok_or(AppError::Query("storing snapshot failed".to_string()))

        // let parent_path = state.db.parent_path("gyms", gym)?;
        // let result: Option<Self> = state
        //     .db
        //     .fluent()
        //     .insert()
        //     .into(Self::COLLECTION)
        //     .generate_document_id()
        //     .parent(&parent_path)
        //     .object(self)
        //     .execute()
        //     .await?;
        //
        // // TODO logging?
        // result.ok_or(AppError::Query("storing snapshot failed".to_string()))
        // match &result {
        //     Some(r) => {
        //         tracing::debug!("storing: {r}");
        //         Ok(r)
        //     },
        //     None => {
        //         tracing::warn!("failed to store: {}", self);
        //         Err(AppError
        //     },
        // }
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

pub struct AccountsView {}

impl AccountsView {
    pub const COLLECTION: &str = "accounts_view";

    pub async fn store(
        state: &AppState,
        gym: &String,
        object_id: &ObjectId,
        content: &Value,
    ) -> Result<(), AppError> {
        let parent_path = state.db.parent_path("gyms", gym)?;
        let account = from_value::<Account>(content.clone())
            .map_err(|e| AppError::ParseError(format!("{e} in: {content}")))?;

        let _: Option<Boulder> = state
            .db
            .fluent()
            .update()
            .in_col(Self::COLLECTION)
            .document_id(object_id.clone())
            .parent(parent_path)
            .object(&account)
            .execute()
            .await?;

        Ok(())
    }
}

pub struct BouldersView {}

impl BouldersView {
    pub const COLLECTION: &str = "boulders_view";

    pub async fn store(
        state: &AppState,
        gym: &String,
        object_id: &ObjectId,
        content: &Value,
    ) -> Result<(), AppError> {
        let parent_path = state.db.parent_path("gyms", gym)?;
        let boulder = from_value::<Boulder>(content.clone())
            .map_err(|e| AppError::ParseError(format!("{e} in: {content}")))?;

        let _: Option<Boulder> = state
            .db
            .fluent()
            .update()
            .in_col(Self::COLLECTION)
            .document_id(object_id.clone())
            .parent(parent_path)
            .object(&boulder)
            .execute()
            .await?;

        Ok(())
    }
}
