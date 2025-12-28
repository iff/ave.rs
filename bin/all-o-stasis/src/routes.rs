use axum::{Router, extract::State, routing::get};
use tower_http::cors::CorsLayer;

use crate::{AppError, AppState, passport};

mod api;
mod collection;
mod stats;

pub use api::PatchObjectResponse;

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
