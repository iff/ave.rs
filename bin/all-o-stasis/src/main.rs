use axum::response::IntoResponse;
use axum::response::Json;
use axum::response::Response;
use axum::{extract::Path, extract::State, http::StatusCode, routing::get, Router};
use firestore::errors::FirestoreError;
use firestore::*;
use otp::types::{Object, Pk};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::sync::Arc;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

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

// The kinds of errors we can hit in our application.
enum AppError {
    // Ot operations fail
    Ot(OtError),
    // firestore db errors
    Firestore(firestore::errors::FirestoreError),
    //
    Query(),
}

impl From<firestore::errors::FirestoreError> for AppError {
    fn from(inner: firestore::errors::FirestoreError) -> Self {
        AppError::Firestore(inner)
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let (status, error_message) = match self {
            AppError::Firestore(FirestoreError::DatabaseError(_)) => (StatusCode::NOT_FOUND, "xxx"),
            AppError::Ot(_) => (StatusCode::NOT_FOUND, "xxx"),
            _ => (StatusCode::INTERNAL_SERVER_ERROR, "Something went wrong"),
        };

        let body = Json(json!({
            "error": error_message,
        }));

        (status, body).into_response()
    }
}

/// Errors that can happen when using OT
#[derive(Debug)]
enum OtError {
    #[allow(dead_code)]
    NotFound,
    #[allow(dead_code)]
    InvalidObjectId,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "all-o-stasis=debug,tower_http=debug".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    let state = Arc::new(AppState {
        db: FirestoreDb::new(&config_env_var("PROJECT_ID")?).await?,
    });
    tracing::debug!("connected to firestore");

    axum::Server::bind(&"0.0.0.0:3000".parse()?)
        .serve(app(state).into_make_service())
        .await?;
    tracing::debug!("listening on http://localhost:3000");

    Ok(())
}

// Having a function that produces our app makes it easy to call it from tests
// without having to create an HTTP server.
#[allow(dead_code)]
fn app(state: Arc<AppState>) -> Router {
    Router::new()
        // git revision sha
        .route("/revision", get(revision))
        // health check
        .route("/healthz", get(healthz))
        // debug create
        .route("/:gym/objects/new", get(new))
        // get object
        .route("/:gym/objects/:id", get(objects))
        .with_state(state)

    // We can still add middleware
    // .layer(TraceLayer::new_for_http())
}

async fn new(
    State(state): State<Arc<AppState>>,
    Path(gym): Path<String>,
) -> Result<Json<Object>, AppError> {
    // create a dummy object
    let obj = Object::new(String::from("boulder"), String::from("id"));

    let parent_path = state.db.parent_path("gyms", gym).unwrap();
    let obj: Option<Object> = state
        .db
        .fluent()
        .insert()
        .into("objects")
        .generate_document_id()
        .parent(&parent_path)
        .object(&obj)
        .execute()
        .await?;

    if let Some(o) = obj {
        Ok(Json(o))
    } else {
        Err(AppError::Query())
    }
}
async fn objects(
    State(state): State<Arc<AppState>>,
    Path((gym, id)): Path<(String, String)>,
) -> Result<Json<Object>, AppError> {
    // ) -> Result<Json<Object>, StatusCode> {
    let parent_path = state.db.parent_path("gyms", gym).unwrap();
    let obj: Option<Object> = state
        .db
        .fluent()
        .select()
        .by_id_in("objects")
        .parent(&parent_path)
        .obj()
        .one(&id)
        .await?;

    if let Some(o) = obj {
        Ok(Json(o))
    } else {
        Err(AppError::Query())
    }
}

async fn revision(State(_state): State<Arc<AppState>>) -> &'static str {
    "rev!"
}
async fn healthz(State(_state): State<Arc<AppState>>) -> &'static str {
    "healthy"
}
