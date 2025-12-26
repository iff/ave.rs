use std::fmt;

use firestore::{FirestoreQueryDirection, FirestoreResult, path_camel_case};
use futures::TryStreamExt;
use futures::stream::BoxStream;
use otp::ObjectId;
use serde::{Deserialize, Serialize};
use serde_json::{Value, from_value};

use crate::{AppError, AppState};

pub mod object;
pub mod patch;
pub mod snapshot;

pub use object::Object;
pub use patch::Patch;
pub use snapshot::Snapshot;

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
pub(crate) use store;

#[derive(Serialize, Deserialize, Clone, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum AccountRole {
    User,
    Setter,
    Admin,
}

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

    pub async fn lookup(
        state: &AppState,
        gym: &String,
        object_id: &ObjectId,
    ) -> Result<Boulder, AppError> {
        let parent_path = state.db.parent_path("gyms", gym)?;
        state
            .db
            .fluent()
            .select()
            .by_id_in(BouldersView::COLLECTION)
            .parent(&parent_path)
            .obj()
            .one(&object_id)
            .await?
            .ok_or(AppError::Query(format!(
                "lookup_boulder: failed to get boulder {object_id}"
            )))
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
    const COLLECTION: &str = "boulders_view";

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

    pub async fn active(state: &AppState, gym: &String) -> Result<Vec<Boulder>, AppError> {
        let parent_path = state.db.parent_path("gyms", gym)?;
        let object_stream: BoxStream<FirestoreResult<Boulder>> = state
            .db
            .fluent()
            .select()
            .from(Self::COLLECTION)
            .parent(&parent_path)
            .filter(|q| {
                q.for_all([
                    q.field(path_camel_case!(Boulder::removed)).eq(0),
                    q.field(path_camel_case!(Boulder::is_draft)).eq(0),
                ])
            })
            .order_by([(
                path_camel_case!(Boulder::set_date),
                FirestoreQueryDirection::Descending,
            )])
            .obj()
            .stream_query_with_errors()
            .await?;

        let boulders: Vec<Boulder> = object_stream.try_collect().await?;
        Ok(boulders)
    }

    pub async fn with_id(
        state: &AppState,
        gym: &String,
        object_id: ObjectId,
    ) -> Result<Vec<Boulder>, AppError> {
        let parent_path = state.db.parent_path("gyms", gym)?;
        let object_stream: BoxStream<FirestoreResult<Boulder>> = state
            .db
            .fluent()
            .select()
            .from(Self::COLLECTION)
            .parent(&parent_path)
            .filter(|q| {
                q.for_all([q
                    .field(path_camel_case!(Boulder::id))
                    .eq(object_id.to_owned())])
            })
            .obj()
            .stream_query_with_errors()
            .await?;

        let as_vec: Vec<Boulder> = object_stream.try_collect().await?;
        Ok(as_vec)
    }

    pub async fn drafts(state: &AppState, gym: &String) -> Result<Vec<Boulder>, AppError> {
        let parent_path = state.db.parent_path("gyms", gym)?;
        // XXX we used to have a separate collection for draft boulders but never used it in the (old)
        // code. Here we choose to follow the old implementation and do not add a collection for draft
        // boulders.
        let object_stream: BoxStream<FirestoreResult<Boulder>> = state
            .db
            .fluent()
            .select()
            .from(Self::COLLECTION)
            .parent(&parent_path)
            .filter(|q| {
                q.for_all([
                    q.field(path_camel_case!(Boulder::removed)).eq(0),
                    q.field(path_camel_case!(Boulder::is_draft)).neq(0),
                ])
            })
            .obj()
            .stream_query_with_errors()
            .await?;

        let as_vec: Vec<Boulder> = object_stream.try_collect().await?;
        Ok(as_vec)
    }

    pub async fn stats(state: &AppState, gym: &String) -> Result<Vec<Boulder>, AppError> {
        let parent_path = state.db.parent_path("gyms", gym)?;
        // TODO this is too expensive: we read all records to compute the stats
        let object_stream: BoxStream<FirestoreResult<Boulder>> = state
            .db
            .fluent()
            .select()
            .from(BouldersView::COLLECTION)
            .parent(&parent_path)
            .filter(|q| q.for_all([q.field(path_camel_case!(Boulder::is_draft)).eq(0)]))
            .obj()
            .stream_query_with_errors()
            .await?;

        let as_vec: Vec<Boulder> = object_stream.try_collect().await?;
        Ok(as_vec)
    }
}
