use std::fmt;

use firestore::{FirestoreQueryDirection, FirestoreResult, path_camel_case};
use futures::{TryStreamExt, stream::BoxStream};
use otp::{ObjectId, Operation, OtError, RevId, ZERO_REV_ID};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::{AppError, AppState, types::patch::Patch};

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

#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    pub object_id: ObjectId,
    pub revision_id: RevId,
    pub content: Value,
}

impl Snapshot {
    const COLLECTION: &str = "snapshots";

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
    }

    /// lookup a snapshot with rev_id or lower and apply patches with revision <= rev_id if necessary
    pub async fn lookup(
        state: &AppState,
        gym: &String,
        obj_id: &ObjectId,
        rev_id: RevId, // inclusive
    ) -> Result<Snapshot, AppError> {
        let latest_snapshot =
            Self::lookup_between(state, gym, obj_id, (ZERO_REV_ID, Some(rev_id))).await?;

        // get all patches which we need to apply on top of the snapshot to
        // arrive at the desired revision
        let patches: Vec<Patch> =
            Patch::after_revision(state, gym, obj_id, latest_snapshot.revision_id)
                .await?
                .into_iter()
                .filter(|p| p.revision_id <= rev_id)
                .collect();

        // apply those patches to the snapshot
        latest_snapshot.apply_patches(&patches)
    }

    /// get latest available snapshot with object_id or create a new snapshot. apply unapplied
    /// patches to get to the latest possible revision.
    pub async fn lookup_latest(
        state: &AppState,
        gym: &String,
        object_id: &ObjectId,
    ) -> Result<Self, AppError> {
        let latest_snapshot =
            Snapshot::lookup_between(state, gym, object_id, (ZERO_REV_ID, None)).await?;

        // get all patches which we need to apply on top of the snapshot to
        // arrive at the desired revision
        let patches =
            Patch::after_revision(state, gym, object_id, latest_snapshot.revision_id).await?;

        // apply those patches to the snapshot
        latest_snapshot.apply_patches(&patches)
    }

    /// get or create a latest snapshot between low and high (inclusive)
    async fn lookup_between(
        state: &AppState,
        gym: &String,
        object_id: &ObjectId,
        range: (RevId, Option<RevId>),
    ) -> Result<Snapshot, AppError> {
        let parent_path = state.db.parent_path("gyms", gym)?;
        let object_stream: BoxStream<FirestoreResult<Snapshot>> = state
            .db
            .fluent()
            .select()
            .from(Self::COLLECTION)
            .parent(&parent_path)
            .filter(|q| {
                q.for_all(
                    [
                        Some(q.field(path_camel_case!(Snapshot::object_id)).eq(object_id)),
                        Some(
                            q.field(path_camel_case!(Snapshot::revision_id))
                                .greater_than_or_equal(range.0),
                        ),
                        range.1.map(|h| {
                            q.field(path_camel_case!(Snapshot::revision_id))
                                .less_than_or_equal(h)
                        }),
                    ]
                    .into_iter()
                    .flatten(),
                )
            })
            .limit(1)
            .order_by([(
                path_camel_case!(Snapshot::revision_id),
                FirestoreQueryDirection::Descending,
            )])
            .obj()
            .stream_query_with_errors()
            .await?;

        let snapshots: Vec<Snapshot> = object_stream.try_collect().await?;
        tracing::debug!(
            "snapshots ({} <= s <= {:?}): {} snapshots, obj={object_id}",
            range.0,
            range.1,
            snapshots.len(),
        );
        match snapshots.first() {
            Some(snapshot) => Ok(snapshot.clone()),
            None => {
                // TODO we could already create the first snapshot on object creation?
                Ok(Snapshot::new(object_id.clone()).store(state, gym).await?)
            }
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
