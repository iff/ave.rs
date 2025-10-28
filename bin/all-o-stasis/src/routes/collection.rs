use axum::response::Json;
use axum::routing::get;
use axum::{
    Router,
    extract::{Path, State},
};
use axum_extra::extract::CookieJar;
use firestore::{FirestoreQueryDirection, FirestoreResult, path_camel_case};
use futures::TryStreamExt;
use futures::stream::BoxStream;
use otp::ObjectId;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::session::author_from_session;
use crate::storage::{ACCOUNTS_VIEW_COLLECTION, BOULDERS_VIEW_COLLECTION, lookup_latest_snapshot};
use crate::types::{Account, AccountRole, Boulder};
use crate::{AppError, AppState};

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
struct PublicProfile {
    name: String,
    avatar: Option<String>,
}

pub fn routes() -> Router<AppState> {
    Router::new()
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
