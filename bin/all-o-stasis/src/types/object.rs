use std::fmt;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::{AppError, AppState, types::ObjectType};
use otp::ObjectId;

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

// Object storage representation - used for Firestore serialization
#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ObjectDoc {
    #[serde(alias = "_firestore_id")]
    id: Option<ObjectId>,
    #[serde(alias = "_firestore_created")]
    created_at: Option<DateTime<Utc>>,
    object_type: ObjectType,
    created_by: ObjectId,
    deleted: Option<bool>,
}

impl ObjectDoc {
    pub const COLLECTION: &str = "objects";

    fn new(object_type: ObjectType) -> Self {
        Self {
            id: None,
            object_type,
            created_at: None,
            created_by: otp::ROOT_OBJ_ID.to_owned(),
            deleted: None,
        }
    }

    async fn lookup(state: &AppState, gym: &String, object_id: ObjectId) -> Result<Self, AppError> {
        let parent_path = state.db.parent_path("gyms", gym)?;
        state
            .db
            .fluent()
            .select()
            .by_id_in(ObjectDoc::COLLECTION)
            .parent(&parent_path)
            .obj()
            .one(&object_id)
            .await?
            .ok_or(AppError::Query(format!(
                "lookup_object: failed to get object {object_id}"
            )))
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

impl Object {
    pub async fn new(
        state: &AppState,
        gym: &String,
        object_type: &ObjectType,
    ) -> Result<Self, AppError> {
        let obj_doc = ObjectDoc::new(object_type.clone())
            .store(state, gym)
            .await?;
        let obj: Object = obj_doc.try_into()?;
        Ok(obj)
    }

    pub async fn lookup(
        state: &AppState,
        gym: &String,
        object_id: &ObjectId,
    ) -> Result<Self, AppError> {
        let obj_doc = ObjectDoc::lookup(state, gym, object_id.clone()).await?;
        let obj: Object = obj_doc.try_into()?;
        Ok(obj)
    }
}

impl fmt::Display for Object {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Object: {} {}", self.id, self.object_type)
    }
}
