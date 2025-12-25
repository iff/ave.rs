use std::fmt;

use chrono::{DateTime, Utc};
use firestore::{FirestoreQueryDirection, FirestoreResult, path_camel_case};
use futures::{TryStreamExt, stream::BoxStream};
use otp::{ObjectId, Operation, OtError, RevId, ZERO_REV_ID};
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
