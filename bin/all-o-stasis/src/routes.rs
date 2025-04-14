use axum::response::{IntoResponse, Json};
use axum::routing::any;
use axum::{
    extract::ws::WebSocketUpgrade,
    extract::{Path, State},
    routing::delete,
    routing::get,
    routing::patch,
    routing::post,
    Router,
};
use axum_extra::extract::cookie::Cookie;
use axum_extra::extract::CookieJar;
use axum_extra::headers::UserAgent;
use axum_extra::TypedHeader;
use chrono::{DateTime, Utc};
use cookie::time::Duration;
use firestore::{path_camel_case, FirestoreResult};
use futures::stream::BoxStream;
use futures::TryStreamExt;
use otp::types::ObjectType;
use otp::types::{Object, ObjectId, Operation, Patch, RevId, ROOT_PATH, ZERO_REV_ID};
use otp::ROOT_OBJ_ID;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::net::SocketAddr;
use tower_http::cors::CorsLayer;

use axum::extract::connect_info::ConnectInfo;

use crate::passport::{passport_routes, Session};
use crate::storage::{
    apply_object_updates, lookup_latest_snapshot, lookup_object_, store_patch,
    ACCOUNTS_VIEW_COLLECTION, BOULDERS_VIEW_COLLECTION, OBJECTS_COLLECTION, PATCHES_COLLECTION,
    SESSIONS_COLLECTION,
};
use crate::types::{Account, AccountRole, Boulder};
use crate::ws::handle_socket;
use crate::{AppError, AppState};

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
struct BoulderStat {
    set_on: u32,
    removed_on: Option<u32>,
    setters: Vec<String>,
    sector: String,
    grade: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
struct PublicProfile {
    name: String,
    avatar: Option<String>,
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
struct CreateObjectBody {
    #[serde(rename = "type")]
    ot_type: ObjectType,
    content: Value,
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LookupObjectResponse {
    pub id: ObjectId,
    #[serde(rename = "type")]
    pub ot_type: ObjectType,
    pub created_at: DateTime<Utc>,
    pub created_by: ObjectId,
    pub revision_id: RevId,
    pub content: Value,
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
struct LookupSessionResponse {
    id: String,
    obj_id: ObjectId,
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
struct CreateObjectResponse {
    id: ObjectId,
    ot_type: ObjectType,
    content: Value,
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
struct PatchObjectBody {
    revision_id: RevId,
    operations: Vec<Operation>,
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
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

async fn author_from_session(
    state: &AppState,
    gym: &String,
    session_id: Option<&Cookie<'static>>,
) -> Result<String, AppError> {
    let session_id = if let Some(session_id) = session_id {
        session_id.value().to_owned()
    } else {
        return Ok(ROOT_OBJ_ID.to_owned());
    };

    let parent_path = state.db.parent_path("gyms", gym)?;
    let session: Option<Session> = state
        .db
        .fluent()
        .select()
        .by_id_in(SESSIONS_COLLECTION)
        .parent(&parent_path)
        .obj()
        .one(&session_id)
        .await?;

    if let Some(session) = session {
        Ok(session.obj_id)
    } else {
        Ok(ROOT_OBJ_ID.to_string())
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

    // TODO allow any? see examples here: https://docs.rs/tower-http/latest/tower_http/cors/struct.CorsLayer.html#method.allow_origin
    let cors = CorsLayer::very_permissive();
    // let cors = CorsLayer::new().allow_origin(Any)..allow_credentials(true);

    let api = api_routes();
    let passport = passport_routes();

    Router::new()
        // git revision sha
        .route("/revision", get(revision))
        // health check
        .route("/healthz", get(healthz))
        // app routes
        .route("/{gym}/public-profile/{id}", get(public_profile))
        // collections: json list of object ids
        // serve a list of all active bouldersIds in the gym
        .route("/{gym}/collection/activeBoulders", get(active_boulders))
        // serve a list of all draft bouldersIds in the gym
        .route("/{gym}/collection/draftBoulders", get(draft_boulders))
        // serve a list of boulderIds that are owned/authored by the user
        // TODO takes credentials?
        .route("/{gym}/collection/ownBoulders", get(own_boulders))
        // serve a list of all accountIds
        .route("/{gym}/collection/accounts", get(accounts))
        // serve a list of all non-user accountIds
        // TODO takes credentials?
        .route("/{gym}/collection/adminAccounts", get(admin_accounts))
        // stats
        // everything has to be set?
        //     :> Capture "setterId" ObjId
        //     :> Capture "year" Integer
        //     :> Capture "month" Int -- 1..12
        //     :> Get '[JSON] SetterMonthlyStats
        .route("/{gym}/stats/{setter_id}/{year}/{month}", get(stats))
        // TODO what does this do?
        //     :> Get '[JSON] [BoulderStat]
        .route("/{gym}/stats/boulders", get(stats_boulders))
        //
        .merge(api)
        .merge(passport)
        //
        .with_state(state)
        .layer(cors)
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
    Path((gym, id)): Path<(String, String)>,
) -> Result<Json<PublicProfile>, AppError> {
    let snapshot = lookup_latest_snapshot(&state, &gym, &id).await?;
    let account: Account = serde_json::from_value(snapshot.content).or(Err(
        AppError::ParseError("failed to parse object".to_string()),
    ))?;

    let name = if let Some(name) = account.name {
        name
    } else {
        "".to_string()
    };

    let mut hashed_email = Sha256::new();
    hashed_email.update(account.email.trim());
    let avatar = format!("https://gravatar.com/avatar/{:X}", hashed_email.finalize());

    Ok(Json(PublicProfile {
        name,
        avatar: Some(avatar),
    }))
}

async fn active_boulders(
    State(state): State<AppState>,
    Path(gym): Path<String>,
) -> Result<Json<Vec<String>>, AppError> {
    let parent_path = state.db.parent_path("gyms", gym)?;
    let object_stream: BoxStream<FirestoreResult<Boulder>> = state
        .db
        .fluent()
        .select()
        .from(BOULDERS_VIEW_COLLECTION)
        .parent(&parent_path)
        .filter(|q| {
            q.for_all([
                q.field(path_camel_case!(Boulder::removed)).eq(0),
                q.field(path_camel_case!(Boulder::is_draft)).eq(0),
            ])
        })
        .obj()
        .stream_query_with_errors()
        .await?;

    let as_vec: Vec<Boulder> = object_stream.try_collect().await?;
    Ok(Json(
        as_vec
            .into_iter()
            .map(|b| b.id.expect("object in view has no id")) // TODO no panic
            .collect(),
    ))
}

async fn draft_boulders(
    State(state): State<AppState>,
    Path(gym): Path<String>,
) -> Result<Json<ObjectId>, AppError> {
    let parent_path = state.db.parent_path("gyms", gym)?;
    let object_stream: BoxStream<FirestoreResult<Boulder>> = state
        .db
        .fluent()
        .select()
        .from(BOULDERS_VIEW_COLLECTION)
        .parent(&parent_path)
        .filter(|q| q.for_all([q.field(path_camel_case!(Boulder::is_draft)).neq(0)]))
        .obj()
        .stream_query_with_errors()
        .await?;

    let as_vec: Vec<Boulder> = object_stream.try_collect().await?;
    Ok(Json(
        as_vec
            .into_iter()
            .map(|b| b.id.expect("object in view has no id")) // TODO no panic
            .collect(),
    ))
}

async fn own_boulders(
    State(state): State<AppState>,
    Path(gym): Path<String>,
    jar: CookieJar,
) -> Result<Json<ObjectId>, AppError> {
    let session_id = jar.get("session");
    let own = author_from_session(&state, &gym, session_id).await?;
    // TODO return [] if not found

    let parent_path = state.db.parent_path("gyms", gym)?;
    let object_stream: BoxStream<FirestoreResult<Boulder>> = state
        .db
        .fluent()
        .select()
        .from(BOULDERS_VIEW_COLLECTION)
        .parent(&parent_path)
        .filter(|q| q.for_all([q.field(path_camel_case!(Boulder::id)).eq(own.to_owned())]))
        .obj()
        .stream_query_with_errors()
        .await?;

    let as_vec: Vec<Boulder> = object_stream.try_collect().await?;
    Ok(Json(
        as_vec
            .into_iter()
            .map(|b| b.id.expect("object in view has no id")) // TODO no panic
            .collect(),
    ))
}

async fn accounts(
    State(state): State<AppState>,
    Path(gym): Path<String>,
) -> Result<Json<Vec<ObjectId>>, AppError> {
    let parent_path = state.db.parent_path("gyms", gym)?;
    let object_stream: BoxStream<FirestoreResult<Account>> = state
        .db
        .fluent()
        .select()
        .from(ACCOUNTS_VIEW_COLLECTION)
        .parent(&parent_path)
        .obj()
        .stream_query_with_errors()
        .await?;

    let as_vec: Vec<Account> = object_stream.try_collect().await?;
    Ok(Json(
        as_vec
            .into_iter()
            .map(|b| b.id.expect("object in view has no id")) // TODO no panic
            .collect(),
    ))
}

async fn admin_accounts(
    State(state): State<AppState>,
    Path(gym): Path<String>,
) -> Result<Json<Vec<ObjectId>>, AppError> {
    let parent_path = state.db.parent_path("gyms", gym)?;
    let object_stream: BoxStream<FirestoreResult<Account>> = state
        .db
        .fluent()
        .select()
        .from(ACCOUNTS_VIEW_COLLECTION)
        .parent(&parent_path)
        .filter(|q| {
            q.for_all([q
                .field(path_camel_case!(Account::role))
                .eq(AccountRole::Admin)])
        })
        .obj()
        .stream_query_with_errors()
        .await?;

    let as_vec: Vec<Account> = object_stream.try_collect().await?;
    Ok(Json(
        as_vec
            .into_iter()
            .map(|b| b.id.expect("object in view has no id")) // TODO no panic
            .collect(),
    ))
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

fn api_routes() -> Router<AppState> {
    // | The cache validator token when passed in the request. The server will
    // use it to determine if the cached response on the client can be reused
    // or not.
    // type CacheValidationToken = Header "If-None-Match" Text
    //
    //
    // | Includes @Cache-Control@ and @ETag@ headers in the response to mark
    // it as cacheable by the client.
    // type Cacheable a = Headers '[Header "Cache-Control" Text, Header "ETag" Text] a

    Router::new()
        // lookup session
        .route("/{gym}/session", get(lookup_session))
        //
        // create
        .route("/{gym}/objects", post(new_object))
        // lookup (cachable)
        .route("/{gym}/objects/{id}", get(lookup_object))
        // patch
        .route("/{gym}/objects/{id}", patch(patch_object))
        // lookup patch
        .route("/{gym}/objects/{id}/patches/{rev_id}", get(lookup_patch))
        // changes (patches) on object (raw websocket)
        .route("/{gym}/objects/{id}/changes", get(object_changes))
        // feed (raw websocket) -- to subscribe to object updates (patches)
        .route("/{gym}/feed", any(feed))
}

async fn lookup_session(
    State(state): State<AppState>,
    Path(gym): Path<String>,
    // Json(payload): axum::extract::Json<LookupSessionBody>,
    jar: CookieJar,
) -> Result<impl IntoResponse, AppError> {
    let parent_path = state.db.parent_path("gyms", gym)?;
    // TODO NoSession correct here?
    let session_id = jar
        .get("session")
        .ok_or(AppError::NoSession())?
        .value()
        .to_owned();
    let session: Session = state
        .db
        .fluent()
        .select()
        .by_id_in(SESSIONS_COLLECTION)
        .parent(&parent_path)
        .obj()
        .one(&session_id)
        .await?
        .ok_or(AppError::NoSession())?;

    let cookie = Cookie::build(("session", session_id.clone()))
        // .domain("api?")
        .path("/")
        .max_age(Duration::weeks(52))
        .secure(true) // TODO not sure about this
        .http_only(true);

    Ok((
        jar.add(cookie),
        Json(LookupSessionResponse {
            id: session.id.expect("session has id"),
            obj_id: session.obj_id,
        }),
    ))
}
async fn new_object(
    State(state): State<AppState>,
    Path(gym): Path<String>,
    jar: CookieJar,
    Json(payload): axum::extract::Json<CreateObjectBody>,
) -> Result<Json<CreateObjectResponse>, AppError> {
    let session_id = jar.get("session");
    let created_by = author_from_session(&state, &gym, session_id).await?;

    let ot_type = payload.ot_type;
    let content = payload.content;
    let obj = Object::new(ot_type.clone(), created_by.clone());

    let parent_path = state.db.parent_path("gyms", gym.clone())?;
    let obj: Option<Object> = state
        .db
        .fluent()
        .insert()
        .into(OBJECTS_COLLECTION)
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
        object_id: obj.id(),
        revision_id: ZERO_REV_ID,
        author_id: created_by,
        created_at: None,
        operation: op,
    };
    let patch = store_patch(&state, &gym, &patch).await?;
    let _ = patch.ok_or_else(AppError::Query)?;

    // TODO needs to also view update..
    //   why dont we here?
    // update_boulder_view(state, gym, content);
    //
    // TODO why not create a snapshot?

    Ok(Json(CreateObjectResponse {
        id: obj.id(),
        ot_type,
        content,
    }))
}

async fn lookup_object(
    State(state): State<AppState>,
    Path((gym, id)): Path<(String, String)>,
) -> Result<Json<LookupObjectResponse>, AppError> {
    lookup_object_(&state, &gym, id).await
}

async fn patch_object(
    State(state): State<AppState>,
    Path((gym, id)): Path<(String, String)>,
    jar: CookieJar,
    Json(payload): axum::extract::Json<PatchObjectBody>,
) -> Result<Json<PatchObjectResponse>, AppError> {
    let session_id = jar.get("session");
    let created_by = author_from_session(&state, &gym, session_id).await?;

    tracing::debug!(
        "patch object ({id}@{}): {} operations",
        payload.revision_id,
        payload.operations.len()
    );
    let result = apply_object_updates(
        &state,
        &gym,
        id,
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
        .from(PATCHES_COLLECTION)
        .parent(&parent_path)
        .filter(|q| {
            q.for_all([
                q.field(path_camel_case!(Patch::object_id)).eq(id.clone()),
                q.field(path_camel_case!(Patch::revision_id)).eq(rev_id),
            ])
        })
        .limit(1)
        .obj()
        .stream_query_with_errors()
        .await?;

    let patches: Vec<Patch> = patch_stream.try_collect().await?;
    match patches.first() {
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
    State(state): State<AppState>,
    Path(gym): Path<String>,
    ws: WebSocketUpgrade,
    user_agent: Option<TypedHeader<UserAgent>>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
) -> impl IntoResponse {
    let user_agent = if let Some(TypedHeader(user_agent)) = user_agent {
        user_agent.to_string()
    } else {
        String::from("Unknown browser")
    };
    tracing::debug!("`{user_agent}` at {addr} connected.");
    // finalize the upgrade process by returning upgrade callback.
    // TODO expect
    let parent_path = state.db.parent_path("gyms", gym).expect("need a gym");
    ws.on_upgrade(move |socket| handle_socket(socket, addr, state, parent_path))
}
