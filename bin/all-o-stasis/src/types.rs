#[derive(Serialize, Deserialize, Clone)]
struct Account {
    login: String,
    role: String,
    email: Option<String>,
    name: Option<String>,
}

#[derive(Serialize, Deserialize, Clone)]
struct Boulder {
    setter: Vec<ObjId>,
    sector: String,
    grade: String,
    grade_nr: u16,
    set_date: usize,
    removed: usize,
    is_draft: usize,
    name: Option<String>,
}
