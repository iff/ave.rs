use axum::{Router, extract::State, routing::get};
use otp::ObjectId;
use tower_http::cors::CorsLayer;

use crate::{
    AppError, AppState, passport,
    storage::{BOULDERS_VIEW_COLLECTION, OBJECTS_COLLECTION},
    types::{Boulder, Object, ObjectDoc, ObjectType},
};

mod api;
mod collection;
mod stats;

pub use api::{LookupObjectResponse, PatchObjectResponse};

pub async fn object_type(
    state: &AppState,
    gym: &String,
    object_id: ObjectId,
) -> Result<ObjectType, AppError> {
    let parent_path = state.db.parent_path("gyms", gym)?;
    let object_doc: Option<ObjectDoc> = state
        .db
        .fluent()
        .select()
        .by_id_in(OBJECTS_COLLECTION)
        .parent(&parent_path)
        .obj()
        .one(&object_id)
        .await?;

    if let Some(doc) = object_doc {
        let object: Object = doc
            .try_into()
            .map_err(|e| AppError::Query(format!("lookup_object_type: {e}")))?;
        Ok(object.object_type)
    } else {
        Err(AppError::NotAuthorized())
    }
}

pub async fn lookup_boulder(
    state: &AppState,
    gym: &String,
    object_id: &ObjectId,
) -> Result<Boulder, AppError> {
    let parent_path = state.db.parent_path("gyms", gym)?;
    let boulder: Option<Boulder> = state
        .db
        .fluent()
        .select()
        .by_id_in(BOULDERS_VIEW_COLLECTION)
        .parent(&parent_path)
        .obj()
        .one(&object_id)
        .await?;

    if let Some(boulder) = boulder {
        Ok(boulder)
    } else {
        Err(AppError::NotAuthorized())
    }
}

mod built_info {
    include!(concat!(env!("OUT_DIR"), "/built.rs"));
}

async fn revision(State(_state): State<AppState>) -> Result<&'static str, AppError> {
    Ok(built_info::GIT_COMMIT_HASH.unwrap_or("no git revision found"))
}

async fn healthz(State(state): State<AppState>) -> Result<&'static str, AppError> {
    let _db_is_alive = state
        .db
        .fluent()
        .list()
        .collections()
        .stream_all_with_errors()
        .await?;

    Ok("alive and kickin")
}

pub fn app(state: AppState) -> Router {
    // TODO simplify gym capture?
    // TODO allow any? see examples here: https://docs.rs/tower-http/latest/tower_http/cors/struct.CorsLayer.html#method.allow_origin
    let cors = CorsLayer::very_permissive();
    // let cors = CorsLayer::new().allow_origin(Any)..allow_credentials(true);

    Router::new()
        .route("/revision", get(revision))
        .route("/healthz", get(healthz))
        // app routes
        .merge(collection::routes())
        .merge(stats::routes())
        .merge(api::routes())
        .merge(passport::routes())
        //
        .with_state(state)
        .layer(cors)
}
