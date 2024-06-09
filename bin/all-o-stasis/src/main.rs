use axum::response::Json;
use axum::{extract::Path, extract::State, routing::get, Router};
use firestore::*;
use otp::types::{Object, Pk};
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

    let shared_state = Arc::new(AppState {
        db: FirestoreDb::new(&config_env_var("PROJECT_ID")?).await?,
    });

    let app = Router::new()
        // git revision sha
        .route("/revision", get(revision))
        // health check
        .route("/healthz", get(healthz))
        // debug create
        .route("/:gym/objects/new", get(new))
        // get object
        .route("/:gym/objects/:id", get(objects))
        .with_state(shared_state);

    println!("starting server");
    axum::Server::bind(&"0.0.0.0:3000".parse()?)
        .serve(app.into_make_service())
        .await?;

    Ok(())
}

async fn new(State(_state): State<Arc<AppState>>, Path(gym): Path<String>) {
    let collection_name = String::from("objects");
    let obj = Object::new(String::from("boulder"), String::from("id"));

    let parent_path = _state.db.parent_path("gyms", gym).unwrap();
    let obj: Option<Object> = _state
        .db
        .fluent()
        .insert()
        .into(&collection_name[..])
        .document_id(obj.to_pk())
        .parent(&parent_path)
        .object(&obj)
        .execute()
        .await
        .unwrap();
}

async fn objects(
    State(_state): State<Arc<AppState>>,
    Path((gym, id)): Path<(String, String)>,
) -> Json<serde_json::Value> {
    let parent_path = _state.db.parent_path("gyms", gym).unwrap();
    let collection_name = String::from("objects");
    let obj: Option<Object> = _state
        .db
        .fluent()
        .select()
        .by_id_in(&collection_name[..])
        .parent(&parent_path)
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
