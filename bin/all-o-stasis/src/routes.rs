use axum::response::Json;
use axum::{
    extract::Path, extract::State, routing::delete, routing::get, routing::patch, routing::post,
    Router,
};
use firestore::{path, FirestoreQueryDirection, FirestoreResult};
use futures::stream::BoxStream;
use futures::TryStreamExt;
use otp::types::ObjectType;
use otp::types::{ObjId, Object, ObjectId, Operation, Patch, RevId, ROOT_PATH, ZERO_REV_ID};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::storage::{apply_object_updates, lookup_object_};
use crate::{AppError, AppState};

#[derive(Serialize, Deserialize, Clone, Debug)]
struct BoulderStat {
    set_on: u32,
    removed_on: Option<u32>,
    setters: Vec<String>,
    sector: String,
    grade: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
struct PublicProfile {
    name: String,
    avatar: Option<String>,
}

#[derive(Serialize, Deserialize, Clone)]
struct CreateObjectBody {
    #[serde(rename = "type")]
    ot_type: ObjectType,
    content: Value,
}

#[derive(Serialize, Deserialize, Clone)]
struct CreateObjectResponse {
    id: ObjId,
    ot_type: ObjectType,
    content: Value,
}

#[derive(Serialize, Deserialize, Clone)]
struct PatchObjectBody {
    revision_id: RevId,
    operations: Vec<Operation>,
}

#[derive(Serialize, Deserialize, Clone)]
pub(crate) struct PatchObjectResponse {
    previous_patches: Vec<Patch>,
    num_processed_operations: usize,
    resulting_patches: Vec<Patch>,
}

impl PatchObjectResponse {
    pub fn new(
        previous_patches: Vec<Patch>,
        num_processed_operations: usize,
        resulting_patches: Vec<Patch>,
    ) -> Self {
        Self {
            previous_patches,
            num_processed_operations,
            resulting_patches,
        }
    }
}

/* avers.js uses:
 *
 * POST      /objects
 * PATCH/GET /objects/objId
 * GET       /objects/objectId/patches/revId
 * GET       /collection/collectionName
 */
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
        // auth
        .route("/gym/:gym/signup", post(signup))
        .merge(api)
        .with_state(state)
}

async fn revision(State(_state): State<AppState>) -> Result<&'static str, AppError> {
    Ok("some git sha - no cheating!")
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

async fn public_profile(
    State(state): State<AppState>,
    Path((gym, _id)): Path<(String, String)>,
) -> Result<Json<Object>, AppError> {
    // TODO
    let _parent_path = state.db.parent_path("gyms", gym)?;
    Err(AppError::NotImplemented())
}

async fn active_boulders(
    State(state): State<AppState>,
    Path(gym): Path<String>,
) -> Result<Json<Vec<Object>>, AppError> {
    let parent_path = state.db.parent_path("gyms", gym)?;
    let object_stream: BoxStream<FirestoreResult<Object>> = state
        .db
        .fluent()
        .select()
        .from("objects")
        .parent(&parent_path)
        .filter(|q| {
            q.for_all([
                // TODO should actually filter contents of object
                q.field(path!(Object::object_type)).eq(ObjectType::Boulder),
                q.field(path!(Object::deleted)).is_not_null(),
                // Some(False)
                //     .and_then(|value| q.field(path!(Object::deleted)).eq(value)),
            ])
        })
        .order_by([(
            path!(Object::created_at),
            FirestoreQueryDirection::Descending,
        )])
        .obj()
        .stream_query_with_errors()
        .await?;

    let as_vec: Vec<Object> = object_stream.try_collect().await?;
    Ok(Json(as_vec))
}

async fn draft_boulders(
    State(state): State<AppState>,
    Path(gym): Path<String>,
) -> Result<Json<Object>, AppError> {
    // TODO just do that with a query
    let _parent_path = state.db.parent_path("gyms", gym)?;
    Err(AppError::NotImplemented())
}

async fn own_boulders(
    State(state): State<AppState>,
    Path(gym): Path<String>,
) -> Result<Json<Object>, AppError> {
    // TODO just do that with a query
    // FIXME needs owner ObjId
    let _parent_path = state.db.parent_path("gyms", gym)?;
    Err(AppError::NotImplemented())
}

async fn accounts(
    State(state): State<AppState>,
    Path(gym): Path<String>,
) -> Result<Json<Vec<Object>>, AppError> {
    let parent_path = state.db.parent_path("gyms", gym)?;
    let object_stream: BoxStream<FirestoreResult<Object>> = state
        .db
        .fluent()
        .select()
        .from("objects")
        .parent(&parent_path)
        .filter(|q| q.for_all([q.field(path!(Object::object_type)).eq(ObjectType::Account)]))
        .order_by([(
            path!(Object::created_at),
            FirestoreQueryDirection::Descending,
        )])
        .obj()
        .stream_query_with_errors()
        .await?;

    let as_vec: Vec<Object> = object_stream.try_collect().await?;
    Ok(Json(as_vec))
}

async fn admin_accounts(
    State(state): State<AppState>,
    Path(gym): Path<String>,
) -> Result<Json<Object>, AppError> {
    // TODO
    // TODO just do that with a query
    let _parent_path = state.db.parent_path("gyms", gym)?;
    Err(AppError::NotImplemented())
}

async fn stats(
    State(state): State<AppState>,
    Path((gym, _id, _year, _month)): Path<(String, String, i32, i32)>,
) -> Result<Json<Object>, AppError> {
    // TODO
    let _parent_path = state.db.parent_path("gyms", gym)?;
    Err(AppError::NotImplemented())
}

async fn stats_boulders(
    State(state): State<AppState>,
    Path(gym): Path<String>,
) -> Result<Json<Object>, AppError> {
    // TODO
    let _parent_path = state.db.parent_path("gyms", gym)?;
    Err(AppError::NotImplemented())
}

async fn signup(
    State(_state): State<AppState>,
    Json(_payload): Json<Value>,
) -> Result<Json<Object>, AppError> {
    // TODO needs gym
    // TODO
    Err(AppError::NotImplemented())
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
        // lookup patch
        .route("/:gym/objects/:id/patches/:rev_id", get(lookup_patch))
        // unused below
        // delete - not used
        .route("/:gym/objects/:id", delete(delete_object))
        // changes on object (raw websocket) -- never used?
        .route("/:gym/objects/:id/changes", get(object_changes))
        // create a release -- not used
        .route("/:gym/objects/:id/releases", post(create_release))
        // lookup release -- not used
        .route("/:gym/objects/:id/releases/:rev_id", get(lookup_release))
        // lookup latest release -- not used
        .route(
            "/:gym/objects/:id/releases/_latest",
            get(lookup_latest_release),
        )
        // feed (raw websocket) -- never used?
        .route("/:gym/feed", get(feed))

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

async fn new_object(
    State(state): State<AppState>,
    Path(gym): Path<String>,
    Json(payload): axum::extract::Json<CreateObjectBody>,
) -> Result<Json<CreateObjectResponse>, AppError> {
    // TODO where do we get that? ah that comes from the credentials
    let created_by = String::from("some id");

    let ot_type = payload.ot_type;
    let content = payload.content;
    let obj = Object::new(ot_type.clone(), created_by.clone());

    let parent_path = state.db.parent_path("gyms", gym)?;
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

    let obj = obj.ok_or_else(AppError::Query)?;
    let op = Operation::Set {
        path: ROOT_PATH.to_string(),
        value: Some(content.clone()),
    };
    let patch = Patch {
        object_id: ObjectId::Base(obj.id()),
        revision_id: ZERO_REV_ID,
        author_id: created_by,
        created_at: None,
        operation: op,
    };
    let patch: Option<Patch> = state
        .db
        .fluent()
        .insert()
        .into("patches")
        .generate_document_id()
        .parent(&parent_path)
        .object(&patch)
        .execute()
        .await?;
    let _ = patch.ok_or_else(AppError::Query)?;

    Ok(Json(CreateObjectResponse {
        id: obj.id(),
        ot_type,
        content,
    }))
}

async fn lookup_object(
    State(state): State<AppState>,
    Path((gym, id)): Path<(String, String)>,
) -> Result<Json<Object>, AppError> {
    Ok(Json(lookup_object_(&state, &gym, id).await?))
}

async fn patch_object(
    State(state): State<AppState>,
    Path((gym, id)): Path<(String, String)>,
    Json(payload): axum::extract::Json<PatchObjectBody>,
) -> Result<Json<PatchObjectResponse>, AppError> {
    // TODO where do we get that? ah that comes from the credentials
    let created_by = String::from("some id");

    let result = apply_object_updates(
        &state,
        &gym,
        ObjectId::Base(id),
        payload.revision_id,
        created_by,
        payload.operations,
        false,
    )
    .await?;

    Ok(result)
}

async fn lookup_patch(
    State(state): State<AppState>,
    Path((gym, id, rev_id)): Path<(String, String, i64)>,
) -> Result<Json<Patch>, AppError> {
    let parent_path = state.db.parent_path("gyms", gym)?;
    let patch_stream: BoxStream<FirestoreResult<Patch>> = state
        .db
        .fluent()
        .select()
        .from("patches")
        .parent(&parent_path)
        .filter(|q| {
            q.for_all([
                q.field(path!(Patch::object_id))
                    .eq(ObjectId::Base(id.clone())),
                q.field(path!(Patch::revision_id)).eq(rev_id),
            ])
        })
        .obj()
        .stream_query_with_errors()
        .await?;

    let as_vec: Vec<Patch> = patch_stream.try_collect().await?;
    // FIXME ensure only 1 result?
    match as_vec.first() {
        Some(patch) => Ok(Json(patch.clone())),
        None => Err(AppError::Query()),
    }
}

// XXX maybe don't needed

async fn object_changes(
    State(_state): State<AppState>,
    Path((_gym, _id)): Path<(String, String)>,
) -> Result<Json<Object>, AppError> {
    Err(AppError::NotImplemented())
}

async fn feed(
    State(_state): State<AppState>,
    Path(_gym): Path<String>,
) -> Result<Json<Object>, AppError> {
    // changes are streamed realtime to the client
    Err(AppError::NotImplemented())
}

// XXX below not implemented in Avers

async fn delete_object(
    State(_state): State<AppState>,
    Path((_gym, _id)): Path<(String, String)>,
) -> Result<Json<Object>, AppError> {
    Err(AppError::NotImplemented())
}

async fn create_release(
    State(_state): State<AppState>,
    Path((_gym, _id)): Path<(String, String)>,
) -> Result<Json<Object>, AppError> {
    Err(AppError::NotImplemented())
}

async fn lookup_release(
    State(_state): State<AppState>,
    Path((_gym, _id, _rev_id)): Path<(String, String, String)>,
) -> Result<Json<Object>, AppError> {
    Err(AppError::NotImplemented())
}

async fn lookup_latest_release(
    State(_state): State<AppState>,
    Path((_gym, _id, _rev_id)): Path<(String, String, String)>,
) -> Result<Json<Object>, AppError> {
    Err(AppError::NotImplemented())
}
