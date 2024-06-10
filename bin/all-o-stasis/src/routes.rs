use axum::response::Json;
use axum::{
    extract::Path, extract::State, routing::delete, routing::get, routing::patch, routing::post,
    Router,
};
use otp::types::Object;
use serde::{Deserialize, Serialize};

use crate::{AppError, AppState};

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

// Having a function that produces our app makes it easy to call it from tests
// without having to create an HTTP server.
pub fn app(state: AppState) -> Router {
    // XXX https://docs.rs/axum/0.6.20/axum/routing/struct.Router.html#method.nest would be nice
    // but we can't use it without breaking the js library
    // TODO simplify gym capture?

    let api = api_routes();

    Router::new()
        // git revision sha
        .route("/revision", get(revision))
        // health check
        .route("/healthz", get(healthz))
        // app routes
        .route("/:gym/public-profile/:id", get(public_profile))
        // collections: json list of object ids
        // serve a list of all active bouldersIds in the gym
        .route("/:gym/collection/activeBoulders", get(active_boulders))
        // serve a list of all draft bouldersIds in the gym
        .route("/:gym/collection/draftBoulders", get(draft_boulders))
        // serve a list of boulderIds that are owned/authored by the user
        // TODO takes credentials?
        .route("/:gym/collection/ownBoulders", get(own_boulders))
        // serve a list of all accountIds
        .route("/:gym/collection/accounts", get(accounts))
        // serve a list of all non-user accountIds
        // TODO takes credentials?
        .route("/:gym/collection/adminAccounts", get(admin_accounts))
        // stats
        // everything has to be set?
        //     :> Capture "setterId" ObjId
        //     :> Capture "year" Integer
        //     :> Capture "month" Int -- 1..12
        //     :> Get '[JSON] SetterMonthlyStats
        .route("/:gym/stats/:setter_id/:year/:month", get(stats))
        // TODO what does this do?
        //     :> Get '[JSON] [BoulderStat]
        .route("/:gym/stats/boulders", get(stats_boulders))
        .merge(api)
        .with_state(state)
    // auth
    //   :> ReqBody '[JSON] SignupRequest2
    //   :> Post '[JSON] SignupResponse2
    // .route("/gym/:gym/signup", post(signup))

    // We can still add middleware
    // .layer(TraceLayer::new_for_http())
}

async fn revision(State(_state): State<AppState>) -> &'static str {
    "rev!"
}

async fn healthz(State(_state): State<AppState>) -> &'static str {
    // run a really simple query to check that the database is also alive
    "healthy"
}

async fn public_profile(
    State(state): State<AppState>,
    Path((gym, id)): Path<(String, String)>,
) -> Result<Json<Object>, AppError> {
    todo!()
}

async fn active_boulders(
    State(state): State<AppState>,
    Path(gym): Path<String>,
) -> Result<Json<Object>, AppError> {
    todo!()
}

async fn draft_boulders(
    State(state): State<AppState>,
    Path(gym): Path<String>,
) -> Result<Json<Object>, AppError> {
    todo!()
}

async fn own_boulders(
    State(state): State<AppState>,
    Path(gym): Path<String>,
) -> Result<Json<Object>, AppError> {
    todo!()
}

async fn accounts(
    State(state): State<AppState>,
    Path(gym): Path<String>,
) -> Result<Json<Object>, AppError> {
    todo!()
}

async fn admin_accounts(
    State(state): State<AppState>,
    Path(gym): Path<String>,
) -> Result<Json<Object>, AppError> {
    todo!()
}

async fn stats(
    State(state): State<AppState>,
    Path((gym, id, year, month)): Path<(String, String, i32, i32)>,
) -> Result<Json<Object>, AppError> {
    todo!()
}

async fn stats_boulders(
    State(state): State<AppState>,
    Path(gym): Path<String>,
) -> Result<Json<Object>, AppError> {
    todo!()
}

fn api_routes() -> Router<AppState> {
    // TODO for now I will not support blobs and no signup creds?

    //  General structure of endpoint definitions
    //
    //  The definition of an endpoint would be too much to put on a single line,
    //  so it is split into multiple lines according to a fixed schema. Each line
    //  represents a particular aspect of the request/response. Lines can be omitted
    //  if they don't apply to the endpoint.
    //
    //   <path> including any captured components
    //   <credentials>
    //   <headers>
    //   <cache validation token>
    //   <request body>
    //   <method and response>
    //
    //
    //
    //  | The cache validator token when passed in the request. The server will
    //  use it to determine if the cached response on the client can be reused
    //  or not.
    // type CacheValidationToken = Header "If-None-Match" Text
    //
    //
    // | Includes @Cache-Control@ and @ETag@ headers in the response to mark
    //  it as cacheable by the client.
    // type Cacheable a = Headers '[Header "Cache-Control" Text, Header "ETag" Text] a

    Router::new()
        // create
        .route("/:gym/objects", post(new_object))
        // lookup (cachable)
        .route("/:gym/objects/:id", get(lookup_object))
        // patch
        .route("/:gym/objects/:id", patch(patch_object))
        // delete
        .route("/:gym/objects/:id", delete(delete_object))
        // lookup patch
        .route("/:gym/objects/:id/patchse/:rev_id", get(lookup_patch))
        // changes on object (raw websocket)
        .route(":gym/objects/:id/changes", get(object_changes))
        // create a release
        .route(":gym/objects/:id/releases", post(create_release))
        // lookup release
        .route(":gym/objects/:id/releases/:rev_id", get(lookup_release))
        // lookup latest release
        .route(
            ":gym/objects/:id/releases/_latest",
            get(lookup_latest_release),
        )
        // feed (raw websocket)
        .route(":gym/feed", get(feed))

    // type ChangeSecret
    //     = "secret"
    //     :> Credentials
    //     :> ReqBody '[JSON] ChangeSecretBody
    //     :> Post '[JSON] ()
    //
    // type CreateSession
    //     = "session"
    //     :> ReqBody '[JSON] CreateSessionBody
    //     :> Post '[JSON] (Headers '[Header "Set-Cookie" SetCookie] CreateSessionResponse)
    //
    // type LookupSession
    //     = "session"
    //     :> SessionId
    //     :> Get '[JSON] (Headers '[Header "Set-Cookie" SetCookie] LookupSessionResponse)
    //
    // type DeleteSession
    //     = "session"
    //     :> SessionId
    //     :> Delete '[JSON] (Headers '[Header "Set-Cookie" SetCookie] ())
    //
    // type UploadBlob
    //     = "blobs"
    //     :> Credentials
    //     :> Header "Content-Type" Text
    //     :> ReqBody '[OctetStream] BlobContent
    //     :> Post '[JSON] UploadBlobResponse
    //
    // type LookupBlob
    //     = "blobs" :> Capture "blobId" BlobId
    //     :> Credentials
    //     :> Get '[JSON] LookupBlobResponse
    //
    // type LookupBlobContent
    //     = "blobs" :> Capture "blobId" BlobId :> "content"
    //     :> Credentials
    //     :> Get '[OctetStream] (Headers '[Header "Content-Type" Text] BlobContent)
}

async fn feed(
    State(state): State<AppState>,
    Path(gym): Path<String>,
) -> Result<Json<Object>, AppError> {
    todo!()
}

async fn new_object(
    State(state): State<AppState>,
    Path(gym): Path<String>,
    Json(payload): Json<Value>,
) -> Result<Json<Object>, AppError> {
    todo!()
    // TODO get body from post and create an object
    // let obj = Object::new(String::from("boulder"), String::from("id"));
    //
    // let parent_path = state.db.parent_path("gyms", gym).unwrap();
    // let obj: Option<Object> = state
    //     .db
    //     .fluent()
    //     .insert()
    //     .into("objects")
    //     .generate_document_id()
    //     .parent(&parent_path)
    //     .object(&obj)
    //     .execute()
    //     .await?;
    //
    // obj.map_or(Err(AppError::Query()), |o| Ok(Json(o)))
}

async fn lookup_object(
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

async fn patch_object(
    State(state): State<AppState>,
    Path((gym, id)): Path<(String, String)>,
) -> Result<Json<Object>, AppError> {
    todo!()
}

async fn delete_object(
    State(state): State<AppState>,
    Path((gym, id)): Path<(String, String)>,
) -> Result<Json<Object>, AppError> {
    todo!()
}

async fn object_changes(
    State(state): State<AppState>,
    Path((gym, id)): Path<(String, String)>,
) -> Result<Json<Object>, AppError> {
    todo!()
}

async fn create_release(
    State(state): State<AppState>,
    Path((gym, id)): Path<(String, String)>,
) -> Result<Json<Object>, AppError> {
    todo!()
}

async fn lookup_patch(
    State(state): State<AppState>,
    Path((gym, id, rev_id)): Path<(String, String, String)>,
) -> Result<Json<Object>, AppError> {
    todo!()
}

async fn lookup_release(
    State(state): State<AppState>,
    Path((gym, id, rev_id)): Path<(String, String, String)>,
) -> Result<Json<Object>, AppError> {
    todo!()
}

async fn lookup_latest_release(
    State(state): State<AppState>,
    Path((gym, id, rev_id)): Path<(String, String, String)>,
) -> Result<Json<Object>, AppError> {
    todo!()
}
