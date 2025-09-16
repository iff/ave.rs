use crate::routes::app;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::response::Json;
use axum::response::Response;
use firestore::FirestoreDb;
use firestore::errors::FirestoreError;
use serde_json::json;
use std::net::SocketAddr;
use std::sync::Arc;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

mod passport;
mod routes;
mod session;
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
}

// The kinds of errors we can hit in our application.
#[derive(Debug)]
enum AppError {
    // Ot operations fail
    Ot(OtError),
    // firestore db errors
    Firestore(FirestoreError),
    // query error
    Query(),
    // unable to parse json content into type
    ParseError(String),
    // No session found
    NoSession(),
    NotAuthorized(),
    Request(),
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
            AppError::Ot(e) => (StatusCode::NOT_FOUND, format!("OT failure: {e}")),
            AppError::Query() => (StatusCode::BAD_REQUEST, "can't handle req".to_string()),
            AppError::ParseError(err) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("parse error: {err}"),
            ),
            AppError::NoSession() => (StatusCode::NOT_FOUND, "session not found".to_string()),
            AppError::NotAuthorized() => (StatusCode::BAD_REQUEST, "not authorized".to_string()),
            AppError::Request() => (StatusCode::BAD_REQUEST, "failed to send email".to_string()),
        };

        let body = Json(json!({
            "error": error_message,
        }));

        (status, body).into_response()
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| {
                format!(
                    // "{}=debug,tower_http=info,firestore=debug",
                    "{}=info,tower_http=info,firestore=info",
                    env!("CARGO_CRATE_NAME")
                )
                .into()
            }),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    let state = AppState {
        db: Arc::new(FirestoreDb::new(&config_env_var("PROJECT_ID")?).await?),
    };
    tracing::debug!("connected to firestore");

    let listener = tokio::net::TcpListener::bind("0.0.0.0:8080").await.unwrap();
    axum::serve(
        listener,
        app(state).into_make_service_with_connect_info::<SocketAddr>(),
    )
    .await
    .unwrap();
    tracing::debug!("listening on http://localhost:8080");

    Ok(())
}
