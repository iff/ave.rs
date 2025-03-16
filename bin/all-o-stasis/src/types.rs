use otp::types::ObjectId;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub enum AccountRole {
    User,
    Setter,
    Admin,
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Account {
    login: String,
    role: AccountRole,
    #[serde(with = "firestore::serialize_as_null")]
    email: Option<String>,
    #[serde(with = "firestore::serialize_as_null")]
    name: Option<String>,
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Boulder {
    #[serde(alias = "_firestore_id")]
    pub id: Option<String>,
    setter: Vec<ObjectId>,
    sector: String,
    grade: String,
    grade_nr: u32,
    pub set_date: usize,
    #[serde(with = "firestore::serialize_as_null")]
    pub removed: Option<usize>,
    #[serde(with = "firestore::serialize_as_null")]
    pub is_draft: Option<usize>,
    name: Option<String>,
}
