use axum::response::Json;
use axum::{extract::Path, extract::State, routing::get, Router};
use firestore::*;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

#[derive(Serialize, Deserialize, Clone)]
struct BoulderStat {
    set_on: u32,
    removed_on: Option<u32>,
    setters: Vec<String>,
    sector: String,
    grade: String,
}

#[derive(Serialize, Deserialize, Clone)]
struct PublicProfile {
    name: String,
    avatar: Option<String>,
}

pub fn config_env_var(name: &str) -> Result<String, String> {
    std::env::var(name).map_err(|e| format!("{}: {}", name, e))
}

#[derive(Clone)]
struct AppState {
    pub db: FirestoreDb,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    tracing_subscriber::fmt::init();

    // TODO proper error handling
    let shared_state = Arc::new(AppState {
        db: FirestoreDb::new(&config_env_var("PROJECT_ID")?).await?,
    });

    let app = Router::new()
        // git revision sha
        .route("/revision", get(revision))
        // health check
        .route("/healthz", get(healthz))
        // get object
        .route("/objects/:id", get(objects))
        .with_state(shared_state);

    println!("starting server");
    axum::Server::bind(&"0.0.0.0:3000".parse()?)
        .serve(app.into_make_service())
        .await?;

    Ok(())
}

const TEST_COLLECTION_NAME: &'static str = "test";

#[derive(Debug, Clone, Deserialize, Serialize)]
struct MyTestStructure {
    some_id: String,
    some_string: String,
    one_more_string: String,
    some_num: u64,
}

async fn objects(
    State(_state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Json<serde_json::Value> {
    let obj: Option<MyTestStructure> = _state
        .db
        .fluent()
        .select()
        .by_id_in(TEST_COLLECTION_NAME)
        .obj()
        .one(&id)
        .await
        .unwrap();

    Json(serde_json::Value::String(
        serde_json::to_string(&obj).unwrap(),
    ))
}

async fn revision(State(_state): State<Arc<AppState>>) -> &'static str {
    "rev!"
}
async fn healthz(State(_state): State<Arc<AppState>>) -> &'static str {
    "healthy"
}
