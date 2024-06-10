use crate::routes::app;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::response::Json;
use axum::response::Response;
use firestore::errors::FirestoreError;
use firestore::FirestoreDb;
use serde_json::json;
use std::sync::Arc;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

mod routes;

pub fn config_env_var(name: &str) -> Result<String, String> {
    std::env::var(name).map_err(|e| format!("{}: {}", name, e))
}

#[derive(Clone)]
struct AppState {
    pub db: Arc<FirestoreDb>,
}

// The kinds of errors we can hit in our application.
enum AppError {
    // Ot operations fail
    Ot(OtError),
    // firestore db errors
    Firestore(FirestoreError),
    //
    Query(),
}

impl From<FirestoreError> for AppError {
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

    let state = AppState {
        db: Arc::new(FirestoreDb::new(&config_env_var("PROJECT_ID")?).await?),
    };
    tracing::debug!("connected to firestore");

    axum::Server::bind(&"0.0.0.0:3000".parse()?)
        .serve(app(state).into_make_service())
        .await?;
    tracing::debug!("listening on http://localhost:3000");

    Ok(())
}

// #[cfg(test)]
// mod tests {
//     use super::*;
//     use axum::{
//         body::Body,
//         extract::connect_info::MockConnectInfo,
//         http::{self, Request, StatusCode},
//     };
//     use serde_json::{json, Value};
//     use std::net::{SocketAddr, TcpListener};
//     use tower::Service; // for `call`
//     use tower::ServiceExt; // for `oneshot` and `ready`
//
//     #[tokio::test]
//     async fn hello_world() {
//         // FIXME how do we mock the state?
//         let app = app();
//
//         // `Router` implements `tower::Service<Request<Body>>` so we can
//         // call it like any tower service, no need to run an HTTP server.
//         let response = app
//             .oneshot(Request::builder().uri("/").body(Body::empty()).unwrap())
//             .await
//             .unwrap();
//
//         assert_eq!(response.status(), StatusCode::OK);
//
//         let body = hyper::body::to_bytes(response.into_body()).await.unwrap();
//         assert_eq!(&body[..], b"Hello, World!");
//     }
//
//     #[tokio::test]
//     async fn json() {
//         let app = app();
//
//         let response = app
//             .oneshot(
//                 Request::builder()
//                     .method(http::Method::POST)
//                     .uri("/json")
//                     .header(http::header::CONTENT_TYPE, mime::APPLICATION_JSON.as_ref())
//                     .body(Body::from(
//                         serde_json::to_vec(&json!([1, 2, 3, 4])).unwrap(),
//                     ))
//                     .unwrap(),
//             )
//             .await
//             .unwrap();
//
//         assert_eq!(response.status(), StatusCode::OK);
//
//         let body = hyper::body::to_bytes(response.into_body()).await.unwrap();
//         let body: Value = serde_json::from_slice(&body).unwrap();
//         assert_eq!(body, json!({ "data": [1, 2, 3, 4] }));
//     }
//
//     #[tokio::test]
//     async fn not_found() {
//         let app = app();
//
//         let response = app
//             .oneshot(
//                 Request::builder()
//                     .uri("/does-not-exist")
//                     .body(Body::empty())
//                     .unwrap(),
//             )
//             .await
//             .unwrap();
//
//         assert_eq!(response.status(), StatusCode::NOT_FOUND);
//         let body = hyper::body::to_bytes(response.into_body()).await.unwrap();
//         assert!(body.is_empty());
//     }
//
//     // You can also spawn a server and talk to it like any other HTTP server:
//     #[tokio::test]
//     async fn the_real_deal() {
//         let listener = TcpListener::bind("0.0.0.0:0".parse::<SocketAddr>().unwrap()).unwrap();
//         let addr = listener.local_addr().unwrap();
//
//         tokio::spawn(async move {
//             axum::Server::from_tcp(listener)
//                 .unwrap()
//                 .serve(app().into_make_service())
//                 .await
//                 .unwrap();
//         });
//
//         let client = hyper::Client::new();
//
//         let response = client
//             .request(
//                 Request::builder()
//                     .uri(format!("http://{}", addr))
//                     .body(Body::empty())
//                     .unwrap(),
//             )
//             .await
//             .unwrap();
//
//         let body = hyper::body::to_bytes(response.into_body()).await.unwrap();
//         assert_eq!(&body[..], b"Hello, World!");
//     }
//
//     // You can use `ready()` and `call()` to avoid using `clone()`
//     // in multiple request
//     #[tokio::test]
//     async fn multiple_request() {
//         let mut app = app();
//
//         let request = Request::builder().uri("/").body(Body::empty()).unwrap();
//         let response = app.ready().await.unwrap().call(request).await.unwrap();
//         assert_eq!(response.status(), StatusCode::OK);
//
//         let request = Request::builder().uri("/").body(Body::empty()).unwrap();
//         let response = app.ready().await.unwrap().call(request).await.unwrap();
//         assert_eq!(response.status(), StatusCode::OK);
//     }
//
//     // Here we're calling `/requires-connect-into` which requires `ConnectInfo`
//     //
//     // That is normally set with `Router::into_make_service_with_connect_info` but we can't easily
//     // use that during tests. The solution is instead to set the `MockConnectInfo` layer during
//     // tests.
//     #[tokio::test]
//     async fn with_into_make_service_with_connect_info() {
//         let mut app = app().layer(MockConnectInfo(SocketAddr::from(([0, 0, 0, 0], 3000))));
//
//         let request = Request::builder()
//             .uri("/requires-connect-into")
//             .body(Body::empty())
//             .unwrap();
//         let response = app.ready().await.unwrap().call(request).await.unwrap();
//         assert_eq!(response.status(), StatusCode::OK);
//     }
// }
