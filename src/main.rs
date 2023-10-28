use axum::{extract::Path, extract::State, response::Json, routing::get, Router};
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
    db: FirestoreDb,
}

#[tokio::main]
async fn main() {
    // -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    tracing_subscriber::fmt::init();

    // TODO proper error handling
    let shared_state = Arc::new(AppState {
        db: FirestoreDb::new(&config_env_var("PROJECT_ID").expect("google creds"))
            .await
            .expect("connection to db"),
    });

    let app = Router::new()
        // git revision sha
        .route("/revision", get(revision))
        // health check
        .route("/healthz", get(healthz))
        .with_state(shared_state);

    axum::Server::bind(&"0.0.0.0:3000".parse().unwrap())
        .serve(app.into_make_service())
        .await
        .unwrap();
}

async fn revision(State(_state): State<Arc<AppState>>) -> &'static str {
    "rev!"
}
async fn healthz(State(_state): State<Arc<AppState>>) -> &'static str {
    "healthy"
}
