use axum::http::StatusCode;
use axum::response::{IntoResponse, Json};
use axum::routing::{any, delete};
use axum::{
    Router,
    extract::ws::WebSocketUpgrade,
    extract::{Path, State},
    routing::get,
    routing::patch,
    routing::post,
};
use axum_extra::TypedHeader;
use axum_extra::extract::CookieJar;
use axum_extra::extract::cookie::Cookie;
use axum_extra::headers::UserAgent;
use chrono::{DateTime, Datelike, Utc};
use cookie::time::Duration;
use firestore::{FirestoreQueryDirection, FirestoreResult, path_camel_case};
use futures::TryStreamExt;
use futures::stream::BoxStream;
use otp::Operation;
use otp::types::{Object, ObjectId, ObjectType, Patch, RevId};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::net::SocketAddr;
use tower_http::cors::CorsLayer;

use axum::extract::connect_info::ConnectInfo;

use crate::passport::{Session, passport_routes};
use crate::session::{account_role, author_from_session};
use crate::storage::{
    ACCOUNTS_VIEW_COLLECTION, BOULDERS_VIEW_COLLECTION, OBJECTS_COLLECTION, PATCHES_COLLECTION,
    SESSIONS_COLLECTION, apply_object_updates, create_object, lookup_latest_snapshot,
    lookup_object_,
};
use crate::types::{Account, AccountRole, Boulder};
use crate::ws::handle_socket;
use crate::{AppError, AppState};

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
struct BoulderStat {
    set_on: String,
    removed_on: Option<String>,
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

async fn object_type(
    state: &AppState,
    gym: &String,
    object_id: ObjectId,
) -> Result<ObjectType, AppError> {
    let parent_path = state.db.parent_path("gyms", gym)?;
    let object: Option<Object> = state
        .db
        .fluent()
        .select()
        .by_id_in(OBJECTS_COLLECTION)
        .parent(&parent_path)
        .obj()
        .one(&object_id)
        .await?;

    if let Some(object) = object {
        Ok(object.object_type)
    } else {
        Err(AppError::NotAuthorized())
    }
}

async fn lookup_boulder(
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
    // TODO return version
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
    let avatar = format!("https://gravatar.com/avatar/{:x}", hashed_email.finalize());

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
        .order_by([(
            path_camel_case!(Boulder::set_date),
            FirestoreQueryDirection::Descending,
        )])
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
) -> Result<Json<Vec<ObjectId>>, AppError> {
    let parent_path = state.db.parent_path("gyms", gym)?;
    // XXX we used to have a separate collection for draft boulders but never used it in the (old)
    // code. Here we choose to follow the old implementation and do not add a collection for draft
    // boulders.
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
            .map(|b| b.id.expect("object in view always has an id"))
            .collect(),
    ))
}

async fn own_boulders(
    State(state): State<AppState>,
    Path(gym): Path<String>,
    jar: CookieJar,
) -> Result<Json<Vec<ObjectId>>, AppError> {
    let session_id = jar.get("session");
    let own = author_from_session(&state, &gym, session_id).await?;
    // TODO not sure if it is okay to return NotAuthorized
    // if own == ROOT_OBJ_ID {
    //     return Ok(Json(Vec::new()));
    // }

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
            .map(|b| b.id.expect("object in view always has an id"))
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
            .map(|b| b.id.expect("object in view always has an id"))
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
                .neq(AccountRole::User)])
        })
        .obj()
        .stream_query_with_errors()
        .await?;

    let as_vec: Vec<Account> = object_stream.try_collect().await?;
    Ok(Json(
        as_vec
            .into_iter()
            .map(|b| b.id.expect("object in view always has an id"))
            .collect(),
    ))
}

async fn stats(
    State(state): State<AppState>,
    Path((gym, id, year, month)): Path<(String, String, i32, u32)>,
) -> Result<Json<HashMap<String, usize>>, AppError> {
    // TODO this endpoint is not really used anywhere?
    let parent_path = state.db.parent_path("gyms", gym)?;
    // fetch all boulders (this may be inefficient when we'll have many boulders)
    let object_stream: BoxStream<FirestoreResult<Boulder>> = state
        .db
        .fluent()
        .select()
        .from(BOULDERS_VIEW_COLLECTION)
        .parent(&parent_path)
        // TODO I think we exclude drafts here
        .filter(|q| q.for_all([q.field(path_camel_case!(Boulder::is_draft)).eq(0)]))
        .obj()
        .stream_query_with_errors()
        .await?;

    // grade -> count
    let mut stats: HashMap<String, usize> = HashMap::new();
    let as_vec: Vec<Boulder> = object_stream.try_collect().await?;
    // TODO as_vec.into_iter().filter..
    for b in as_vec {
        // TODO millis, macros, or nanos?
        let boulder_date = DateTime::from_timestamp_nanos(b.set_date as i64);
        // if let Some(date) = boulder_date {
        if b.in_setter(&id) && boulder_date.month() == month && boulder_date.year() == year {
            let grade = stats.entry(b.grade).or_insert(0);
            *grade += 1;
        }
        // }
    }

    Ok(Json(stats))
}

fn stat_date(epoch_millis: usize) -> String {
    let date = DateTime::from_timestamp_millis(epoch_millis as i64).expect("invalid timestamp");
    date.format("%Y-%m-%d").to_string()
}

async fn stats_boulders(
    State(state): State<AppState>,
    Path(gym): Path<String>,
) -> Result<Json<Vec<BoulderStat>>, AppError> {
    let parent_path = state.db.parent_path("gyms", gym)?;
    let object_stream: BoxStream<FirestoreResult<Boulder>> = state
        .db
        .fluent()
        .select()
        .from(BOULDERS_VIEW_COLLECTION)
        .parent(&parent_path)
        // TODO I think we exclude drafts here
        // in the old app we had a separate view for draft boulders?
        .filter(|q| q.for_all([q.field(path_camel_case!(Boulder::is_draft)).eq(0)]))
        .obj()
        .stream_query_with_errors()
        .await?;

    let as_vec: Vec<Boulder> = object_stream.try_collect().await?;
    let stats: Vec<BoulderStat> = as_vec
        .into_iter()
        .map(|b| BoulderStat {
            set_on: stat_date(b.set_date),
            removed_on: if b.removed == 0 {
                None
            } else {
                Some(stat_date(b.removed))
            },
            setters: b.setter,
            sector: b.sector,
            grade: b.grade,
        })
        .collect();

    Ok(Json(stats))
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
        // signout
        .route("/{gym}/session", delete(delete_session))
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
        // feed (raw websocket) -- to subscribe to object updates (patches)
        .route("/{gym}/feed", any(feed))
}

async fn delete_session(
    State(state): State<AppState>,
    Path(gym): Path<String>,
    jar: CookieJar,
) -> Result<impl IntoResponse, AppError> {
    let parent_path = state.db.parent_path("gyms", gym)?;
    let session_id = jar
        .get("session")
        .ok_or(AppError::NoSession())?
        .value()
        .to_owned();

    state
        .db
        .fluent()
        .delete()
        .from(SESSIONS_COLLECTION)
        .parent(&parent_path)
        .document_id(&session_id)
        .execute()
        .await?;

    let cookie = Cookie::build(("session", session_id.clone()))
        .path("/")
        .max_age(Duration::seconds(0))
        .secure(true) // TODO not sure about this
        .http_only(true);

    Ok(jar.add(cookie))
}

async fn lookup_session(
    State(state): State<AppState>,
    Path(gym): Path<String>,
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

    // unauthorized users should be able to create accounts
    // but this happens in the passport routes
    // so we dont allow unauthorized here
    // if created_by == ROOT_OBJ_ID {
    //     return Err(AppError::NotAuthorized());
    // }

    // only admins and setters can add objects (account setup in passport)
    let role = account_role(&state, &gym, &created_by).await?;
    if AccountRole::User == role {
        return Err(AppError::NotAuthorized());
    }

    let ot_type = payload.ot_type;
    let content = payload.content;
    // changing this to also add the object to the view
    let obj = create_object(&state, &gym, created_by, ot_type.clone(), content.clone()).await?;

    Ok(Json(CreateObjectResponse {
        id: obj.id(),
        ot_type,
        content,
    }))
}

async fn lookup_object(
    State(state): State<AppState>,
    Path((gym, id)): Path<(String, String)>,
    jar: CookieJar,
) -> Result<Json<LookupObjectResponse>, AppError> {
    let response = lookup_object_(&state, &gym, id).await?;

    // anyone can lookup boulders
    if response.ot_type == ObjectType::Boulder {
        return Ok(response);
    }

    let session_id = jar.get("session");
    let created_by = author_from_session(&state, &gym, session_id).await?;

    // otherwise just object the owner owns
    if created_by == response.created_by {
        return Ok(response);
    }

    let role = account_role(&state, &gym, &created_by).await?;
    // or if the user is an admin/setter
    if role == AccountRole::Admin || role == AccountRole::Setter {
        Ok(response)
    } else {
        Err(AppError::NotAuthorized())
    }
}

async fn patch_object(
    State(state): State<AppState>,
    Path((gym, id)): Path<(String, String)>,
    jar: CookieJar,
    Json(payload): axum::extract::Json<PatchObjectBody>,
) -> Result<Json<PatchObjectResponse>, AppError> {
    let session_id = jar.get("session");
    let created_by = author_from_session(&state, &gym, session_id).await?;

    // users cant patch atm
    // TODO should be able to patch their account? (probably not implemented in the client?)
    let role = account_role(&state, &gym, &created_by).await?;
    if role == AccountRole::User {
        return Err(AppError::NotAuthorized());
    }

    let ot_type = object_type(&state, &gym, id.clone()).await?;
    match ot_type {
        ObjectType::Account => {
            if role == AccountRole::Setter {
                // only admins can change the role of an Account
                let patch_changes_role = payload
                    .operations
                    .clone()
                    .into_iter()
                    .find(|op| op.path().contains("role"));
                if patch_changes_role.is_some() {
                    return Err(AppError::NotAuthorized());
                }

                // otherwise we can change our own account?
                if id.clone() != created_by.clone() {
                    return Err(AppError::NotAuthorized());
                }
            }
        }
        ObjectType::Boulder => {
            let boulder = lookup_boulder(&state, &gym, &id).await?;
            if boulder.is_draft > 0 {
                // drafts can be edited by any admin/setter
            } else {
                // admin and setter of boulder or created by
                #[allow(clippy::collapsible_if)]
                if role == AccountRole::Setter {
                    if !(id.clone() == created_by || boulder.in_setter(&created_by.clone())) {
                        tracing::debug!("PATCH: setter cant patch this boulder");
                        return Err(AppError::NotAuthorized());
                    }
                }
            }
        }
        ObjectType::Passport => (),
    }

    tracing::debug!(
        "patch object ({}@{}): {} operations",
        id.clone(),
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
        String::from("unknown browser")
    };
    tracing::debug!("`{user_agent}` at {addr} connected.");

    match state.db.parent_path("gyms", &gym) {
        Ok(path) => ws
            .on_upgrade(move |socket| handle_socket(socket, addr, state, path))
            .into_response(),
        Err(e) => {
            tracing::error!("firestore parent_path {gym}: {e:?}");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("firestore parent_path {gym}: {e:?}"),
            )
                .into_response()
        }
    }
}
