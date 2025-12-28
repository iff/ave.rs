use std::collections::hash_map::DefaultHasher;
use std::fmt;
use std::hash::{Hash, Hasher};
use std::net::SocketAddr;

use chrono::{DateTime, Utc};
use firestore::{
    FirestoreDb, FirestoreListener, FirestoreListenerTarget, FirestoreMemListenStateStorage,
    ParentPathBuilder,
};
use firestore::{FirestoreQueryDirection, FirestoreResult, path_camel_case};
use futures::{TryStreamExt, stream::BoxStream};
use otp::{ObjectId, Operation, RevId};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::types::store;
use crate::{AppError, AppState, types::Snapshot};

fn hash_addr(addr: &SocketAddr) -> u64 {
    let mut hasher = DefaultHasher::new();
    // TODO hash addr.ip()?

    match addr {
        SocketAddr::V4(v4) => {
            v4.ip().octets().hash(&mut hasher);
            v4.port().hash(&mut hasher);
        }
        SocketAddr::V6(v6) => {
            v6.ip().octets().hash(&mut hasher);
            v6.port().hash(&mut hasher);
        }
    }

    hasher.finish()
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
    const COLLECTION: &str = "patches";

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

    /// get all patches for an object with revision id > rev_id
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

    pub async fn listener(
        state: &AppState,
        parent_path: &ParentPathBuilder,
        who: SocketAddr,
    ) -> Option<FirestoreListener<FirestoreDb, FirestoreMemListenStateStorage>> {
        let client_id = hash_addr(&who) as u32;
        let listener_id: FirestoreListenerTarget = FirestoreListenerTarget::new(client_id);
        tracing::debug!("connection {who} gets firestore listener id: {client_id:?}");

        // now start streaming patches using firestore listeners: https://github.com/abdolence/firestore-rs/blob/master/examples/listen-changes.rs
        let mut listener = match state
            .db
            .create_listener(FirestoreMemListenStateStorage::new())
            .await
        {
            Ok(l) => l,
            Err(..) => return None,
        };

        let _ = state
            .db
            .fluent()
            .select()
            .from(Patch::COLLECTION)
            .parent(parent_path)
            .listen()
            .add_target(listener_id, &mut listener);

        Some(listener)
    }
}
