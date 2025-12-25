use std::fmt;

use otp::ObjectId;
use serde::{Deserialize, Serialize};
use serde_json::{Value, from_value};

use crate::{AppError, AppState};

pub mod object;
pub mod patch;
pub mod snapshot;

pub use object::{Object, ObjectDoc};
pub use patch::Patch;
pub use snapshot::Snapshot;

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
