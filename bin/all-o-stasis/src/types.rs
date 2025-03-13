use otp::types::ObjectId;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone)]
pub enum AccountRole {
    User,
    Setter,
    Admin,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct Account {
    login: String,
    role: AccountRole,
    email: Option<String>,
    name: Option<String>,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct Boulder {
    setter: Vec<ObjectId>,
    sector: String,
    grade: String,
    grade_nr: u32,
    pub set_date: usize,
    pub removed: Option<usize>,
    is_draft: Option<usize>,
    name: Option<String>,
}
