use std::{net::SocketAddr, sync::Arc};

use axum::{
    http::StatusCode,
    response::{IntoResponse, Json, Response},
};
use firestore::{FirestoreDb, FirestoreDbOptions, errors::FirestoreError};
use serde_json::json;
use tracing::Level;
use tracing_subscriber::{
    Layer, filter::Targets, layer::SubscriberExt, util::SubscriberInitExt,
};

use crate::routes::app;

mod passport;
mod routes;
mod storage;
mod types;
mod word_list;
mod ws;
use otp::OtError;

pub fn config_env_var(name: &str) -> Result<String, String> {
    std::env::var(name).map_err(|e| format!("{name}: {e}"))
}

#[derive(Clone)]
struct AppState {
    pub db: Arc<FirestoreDb>,
    pub api_host: String,
}

// The kinds of errors we can hit in our application.
#[derive(Debug)]
pub enum AppError {
    // Ot operations fail
    Ot(OtError),
    // firestore db errors
    Firestore(FirestoreError),
    // query error
    Query(String), // TODO split and more meaningful name
    // unable to parse json content into type
    ParseError(String),
    // unexpected internal failure (bug or invariant violation)
    Internal(String),
    // No session found
    NoSession(),
    NotAuthorized(),
    Passport(String),
}

impl From<FirestoreError> for AppError {
    fn from(inner: firestore::errors::FirestoreError) -> Self {
        AppError::Firestore(inner)
    }
}

impl From<OtError> for AppError {
    fn from(inner: OtError) -> Self {
        AppError::Ot(inner)
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let (status, error_message) = match self {
            AppError::Firestore(FirestoreError::SystemError(_)) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "firestore system error".to_string(),
            ),
            AppError::Firestore(FirestoreError::DatabaseError(e)) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("database error: {}", e.details),
            ),
            AppError::Firestore(FirestoreError::DataConflictError(_)) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "data conflict error".to_string(),
            ),
            AppError::Firestore(FirestoreError::DataNotFoundError(_)) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "data not found error".to_string(),
            ),
            AppError::Firestore(FirestoreError::InvalidParametersError(_)) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "invalid params error".to_string(),
            ),
            AppError::Firestore(FirestoreError::SerializeError(_)) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "serialization error".to_string(),
            ),
            AppError::Firestore(FirestoreError::DeserializeError(e)) => {
                (StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
            }
            AppError::Firestore(FirestoreError::NetworkError(_)) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "network error".to_string(),
            ),
            AppError::Firestore(FirestoreError::ErrorInTransaction(_)) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "transaction error".to_string(),
            ),
            AppError::Firestore(FirestoreError::CacheError(_)) => {
                (StatusCode::INTERNAL_SERVER_ERROR, "cache error".to_string())
            }
            AppError::Ot(e) => {
                (StatusCode::NOT_FOUND, format!("OT failure: {e}"))
            }
            AppError::Query(e) => {
                (StatusCode::BAD_REQUEST, format!("Query issue: {e}"))
            }
            AppError::ParseError(err) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("parse error: {err}"),
            ),
            AppError::NoSession() => {
                (StatusCode::NOT_FOUND, "session not found".to_string())
            }
            AppError::NotAuthorized() => {
                (StatusCode::BAD_REQUEST, "not authorized".to_string())
            }
            AppError::Internal(e) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("internal error: {e}"),
            ),
            AppError::Passport(e) => {
                (StatusCode::BAD_REQUEST, format!("passport failure: {e}"))
            }
        };

        let body = Json(json!({
            "error": error_message,
        }));

        (status, body).into_response()
    }
}

fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    // sentry must be initialised before the tokio runtime starts
    let traces_sample_rate = match std::env::var("SENTRY_TRACES_SAMPLE_RATE") {
        Ok(rate) => rate
            .parse::<f32>()
            .map_err(|e| format!("SENTRY_TRACES_SAMPLE_RATE: {e}"))?,
        Err(_) => 1.0,
    };
    if !(0.0..=1.0).contains(&traces_sample_rate) {
        return Err(format!(
            "SENTRY_TRACES_SAMPLE_RATE: {traces_sample_rate} not in [0, 1]"
        )
        .into());
    }
    let mut sentry_options =
        sentry::ClientOptions::new().traces_sample_rate(traces_sample_rate);
    if let Some(git_hash) = option_env!("GIT_HASH") {
        sentry_options = sentry_options.release(git_hash);
    }
    let _sentry = sentry::init(sentry_options);

    tracing_subscriber::registry()
        .with(
            tracing_subscriber::fmt::layer().with_filter(
                tracing_subscriber::EnvFilter::try_from_default_env()
                    .unwrap_or_else(|_| {
                        format!(
                            "{}=info,tower_http=info,firestore=info",
                            env!("CARGO_CRATE_NAME")
                        )
                        .into()
                    }),
            ),
        )
        // use a separate filter so tuning RUST_LOG for stdout cannot drop the
        // spans
        .with(sentry::integrations::tracing::SpanLayer::new().with_filter(
            Targets::new().with_target(env!("CARGO_CRATE_NAME"), Level::INFO),
        ))
        .init();

    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?
        .block_on(serve())
}

async fn serve() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let gcp_project_id = config_env_var("PROJECT_ID")?;
    // TODO prod should also be a named database
    // TODO better configuration
    let (options, api_host) = match config_env_var("FIRESTORE_DATABASE_ID") {
        Ok(name) => (
            FirestoreDbOptions::new(gcp_project_id.to_string())
                .with_database_id(name.to_string())
                .with_max_retries(5),
            "https://apiv2-dev.boulderhalle.app",
        ),
        Err(_) => (
            FirestoreDbOptions::new(gcp_project_id.to_string()),
            "https://apiv2.boulderhalle.app",
        ),
    };

    let state = AppState {
        db: Arc::new(FirestoreDb::with_options(options).await?),
        api_host: api_host.to_string(),
    };
    tracing::debug!("connected to firestore");

    let listener = tokio::net::TcpListener::bind("0.0.0.0:8080").await?;
    axum::serve(
        listener,
        app(state).into_make_service_with_connect_info::<SocketAddr>(),
    )
    .await?;
    tracing::debug!("listening on http://localhost:8080");

    Ok(())
}
