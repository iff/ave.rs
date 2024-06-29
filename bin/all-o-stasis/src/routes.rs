use axum::response::Json;
use axum::{
    extract::Path, extract::State, routing::delete, routing::get, routing::patch, routing::post,
    Router,
};
use firestore::{path, FirestoreResult};
use futures::stream::BoxStream;
use futures::TryStreamExt;
use otp::types::{
    ObjId, Object, ObjectId, Operation, Patch, RevId, Snapshot, ROOT_PATH, ZERO_REV_ID,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;

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
    ot_type: String,
    content: Value,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
struct CreateObjectResponse {
    id: ObjId,
    ot_type: String,
    content: Value,
}

#[derive(Serialize, Deserialize, Clone)]
struct PatchObjectBody {
    revision_id: RevId,
    operations: Vec<Operation>,
}

#[derive(Serialize, Deserialize, Clone)]
struct PatchObjectResponse {
    previous_patches: Vec<Patch>,
    num_processed_operations: u32,
    resulting_patches: Vec<Patch>,
}

async fn lookup_object_(state: &AppState, gym: &String, id: ObjId) -> Result<Object, AppError> {
    let parent_path = state.db.parent_path("gyms", gym)?;
    let obj: Option<Object> = state
        .db
        .fluent()
        .select()
        .by_id_in("objects")
        .parent(&parent_path)
        .obj()
        .one(&id)
        .await?;

    obj.ok_or(AppError::Query())
}

fn base_id(obj_id: &ObjectId) -> ObjId {
    match obj_id {
        ObjectId::Base(id) => id.clone(),
        ObjectId::Release(id, _) => id.clone(),
        ObjectId::Authorization(id) => id.clone(),
    }
}

fn lookup_object_type(obj_id: Object) -> String {
    todo!()
}

fn lookup_snapshot(obj_id: &ObjectId, rev_id: &RevId) -> Snapshot {
    todo!()
}

fn patches_after_revision(obj_id: &ObjectId, rev_id: &RevId) -> Vec<Patch> {
    todo!()
}

fn apply_patches(base_snapshot: Snapshot, previous_patches: &Vec<Patch>) -> Snapshot {
    todo!()
}

async fn apply_object_updates(
    state: &AppState,
    gym: &String,
    obj_id: ObjectId,
    rev_id: RevId,
    _author: ObjId,
    _operations: &Vec<Operation>,
    _skip_validation: bool,
) -> Result<Json<PatchObjectResponse>, AppError> {
    let parent_path = state.db.parent_path("gyms", gym)?;

    // first check that the object exists. We'll need its metadata later
    let id = base_id(&obj_id);
    let obj = lookup_object_(state, gym, id).await?;
    let ot_type = lookup_object_type(obj);

    // The 'Snapshot' against which the submitted operations were created
    let base_snapshot = lookup_snapshot(&obj_id, &rev_id);

    // If there are any patches which the client doesn't know about we need
    // to let her know.
    let previous_patches = patches_after_revision(&obj_id, &rev_id);
    let latest_snapshot = apply_patches(base_snapshot, &previous_patches);

    // Apply the operations and get the final snapshot.
    //   (Snapshot{..}, PatchState{..}) <- runStateT (patchHandler novalidate) $
    //       PatchState ot objId revId committerId ops 0
    //           baseSnapshot latestSnapshot previousPatches []
    //           data PatchState a = PatchState
    // PatchState {
    //   psObjectType            :: ObjectType a
    // , psObjectId              :: ObjectId
    // , psRevisionId            :: RevId
    // , psCommitterId           :: ObjId
    // , psOperations            :: [ Operation ]
    // , psNumConsumedOperations :: Int
    // , psBaseSnapshot          :: Snapshot
    // , psLatestSnapshot        :: Snapshot
    // , psPreviousPatches       :: [ Patch ]
    // , psPatches               :: [ Patch ]
    // }
    // TODO form PatchState
    let num_processed_operations = 0;
    let resulting_patches = Vec::new();

    //   -- Update object views.
    //   unless novalidate $ do
    //       content <- parseValue snapshotContent
    //       updateObjectViews ot baseObjId (Just content)

    Ok(Json(PatchObjectResponse {
        previous_patches,
        num_processed_operations,
        resulting_patches,
    }))
}

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
    // run a really simple query to check that the database is also alive
    let _ = state
        .db
        .fluent()
        .list()
        .collections()
        .stream_all_with_errors()
        .await?;

    Ok("healthy")
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
) -> Result<Json<Object>, AppError> {
    // TODO
    let _parent_path = state.db.parent_path("gyms", gym)?;
    Err(AppError::NotImplemented())
    // let object_stream: BoxStream<FirestoreResult<MyTestStructure>> = db
    //     .fluent()
    //     .select()
    //     .fields(
    //         paths!(MyTestStructure::{some_id, some_num, some_string, one_more_string, created_at}),
    //     )
    //     .from(TEST_COLLECTION_NAME)
    //     .filter(|q| {
    //         q.for_all([
    //             q.field(path!(MyTestStructure::some_num)).is_not_null(),
    //             q.field(path!(MyTestStructure::some_string)).eq("Test"),
    //             Some("Test2")
    //                 .and_then(|value| q.field(path!(MyTestStructure::one_more_string)).eq(value)),
    //         ])
    //     })
    //     .order_by([(
    //         path!(MyTestStructure::some_num),
    //         FirestoreQueryDirection::Descending,
    //     )])
    //     .obj()
    //     .stream_query_with_errors()
    //     .await?;
    //
    // let as_vec: Vec<Boulders> = object_stream.try_collect().await?;
    //
    // Ok(as_vec)
}

// TODO views?
async fn draft_boulders(
    State(state): State<AppState>,
    Path(gym): Path<String>,
) -> Result<Json<Object>, AppError> {
    // TODO
    let _parent_path = state.db.parent_path("gyms", gym)?;
    Err(AppError::NotImplemented())
}

// TODO views?
async fn own_boulders(
    State(state): State<AppState>,
    Path(gym): Path<String>,
) -> Result<Json<Object>, AppError> {
    // TODO
    let _parent_path = state.db.parent_path("gyms", gym)?;
    Err(AppError::NotImplemented())
}

// TODO views?
async fn accounts(
    State(state): State<AppState>,
    Path(gym): Path<String>,
) -> Result<Json<Object>, AppError> {
    // TODO
    let _parent_path = state.db.parent_path("gyms", gym)?;
    Err(AppError::NotImplemented())
}

// TODO views?
async fn admin_accounts(
    State(state): State<AppState>,
    Path(gym): Path<String>,
) -> Result<Json<Object>, AppError> {
    // TODO
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
        // delete
        .route("/:gym/objects/:id", delete(delete_object))
        // lookup patch
        .route("/:gym/objects/:id/patches/:rev_id", get(lookup_patch))
        // changes on object (raw websocket)
        .route("/:gym/objects/:id/changes", get(object_changes))
        // create a release
        .route("/:gym/objects/:id/releases", post(create_release))
        // lookup release
        .route("/:gym/objects/:id/releases/:rev_id", get(lookup_release))
        // lookup latest release
        .route(
            "/:gym/objects/:id/releases/_latest",
            get(lookup_latest_release),
        )
        // feed (raw websocket)
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

    // TODO updateObjectViews ot objId (Just content)

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
    let parent_path = state.db.parent_path("gyms", gym)?;
    // TODO use lookup object
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
        &payload.operations,
        false,
    ).await?;

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
    // FIXME cleanup, ensure only 1 result?
    Ok(Json(as_vec[0].clone()))
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
