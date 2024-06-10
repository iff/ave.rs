use axum::response::Json;
use axum::{extract::Path, extract::State, routing::get, Router};
use otp::types::Object;

use crate::{AppError, AppState};

// Having a function that produces our app makes it easy to call it from tests
// without having to create an HTTP server.
#[allow(dead_code)]
pub fn app(state: AppState) -> Router {
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
    State(state): State<AppState>,
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

    obj.map_or(Err(AppError::Query()), |o| Ok(Json(o)))
}
async fn objects(
    State(state): State<AppState>,
    Path((gym, id)): Path<(String, String)>,
) -> Result<Json<Object>, AppError> {
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

    obj.map_or(Err(AppError::Query()), |o| Ok(Json(o)))
}

async fn revision(State(_state): State<AppState>) -> &'static str {
    "rev!"
}
async fn healthz(State(_state): State<AppState>) -> &'static str {
    "healthy"
}
