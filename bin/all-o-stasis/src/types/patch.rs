use std::fmt;

use chrono::{DateTime, Utc};
use firestore::{FirestoreQueryDirection, FirestoreResult, path_camel_case};
use futures::{TryStreamExt, stream::BoxStream};
use otp::{ObjectId, Operation, RevId};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{AppError, AppState, types::Snapshot};

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

    /// lookup a patch with rev_id
    pub async fn lookup(
        state: &AppState,
        gym: &String,
        object_id: &ObjectId,
        rev_id: RevId, // inclusive
    ) -> Result<Self, AppError> {
        let parent_path = state.db.parent_path("gyms", gym)?;
        let patch_stream: BoxStream<FirestoreResult<Patch>> = state
            .db
            .fluent()
            .select()
            .from(Self::COLLECTION)
            .parent(&parent_path)
            .filter(|q| {
                q.for_all([
                    q.field(path_camel_case!(Patch::object_id))
                        .eq(object_id.clone()),
                    q.field(path_camel_case!(Patch::revision_id)).eq(rev_id),
                ])
            })
            .limit(1)
            .obj()
            .stream_query_with_errors()
            .await?;

        let mut patches: Vec<Patch> = patch_stream.try_collect().await?;
        if patches.len() != 1 {
            return Err(AppError::Query(format!(
                "lookup_patch found {} patches, expecting only 1",
                patches.len()
            )));
        }
        let patch = patches.pop().unwrap();
        Ok(patch)
    }

    pub async fn after_revision(
        state: &AppState,
        gym: &String,
        obj_id: &ObjectId,
        rev_id: RevId,
    ) -> Result<Vec<Patch>, AppError> {
        let parent_path = state.db.parent_path("gyms", gym)?;
        let object_stream: BoxStream<FirestoreResult<Patch>> = state
            .db
            .fluent()
            .select()
            .from(Patch::COLLECTION)
            .parent(&parent_path)
            .filter(|q| {
                q.for_all([
                    q.field(path_camel_case!(Patch::object_id)).eq(obj_id),
                    q.field(path_camel_case!(Patch::revision_id))
                        .greater_than(rev_id),
                ])
            })
            .order_by([(
                path_camel_case!(Snapshot::revision_id),
                FirestoreQueryDirection::Ascending,
            )])
            .obj()
            .stream_query_with_errors()
            .await?;

        let patches: Vec<Patch> = object_stream.try_collect().await?;
        tracing::debug!(
            "patches after rev ({rev_id}): {}, obj = {obj_id}",
            patches.len()
        );
        Ok(patches)
    }
}
