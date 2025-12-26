use axum::response::Json;
use axum::routing::get;
use axum::{
    Router,
    extract::{Path, State},
};
use axum_extra::extract::CookieJar;
use otp::ObjectId;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::passport::author_from_session;
use crate::types::{Account, AccountsView, BouldersView, Snapshot};
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
    let snapshot = Snapshot::lookup_latest(&state, &gym, &id).await?;
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
    let boulders = BouldersView::active(&state, &gym).await?;
    Ok(Json(
        boulders
            .into_iter()
            .map(|b| b.id.expect("object in view has no id")) // TODO no panic
            .collect(),
    ))
}

async fn draft_boulders(
    State(state): State<AppState>,
    Path(gym): Path<String>,
) -> Result<Json<Vec<ObjectId>>, AppError> {
    let as_vec = BouldersView::drafts(&state, &gym).await?;
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
    let session_id = jar.get("session").ok_or(AppError::NoSession())?;
    let own = author_from_session(&state, &gym, session_id).await?;
    // TODO not sure if it is okay to return NotAuthorized
    // if own == ROOT_OBJ_ID {
    //     return Ok(Json(Vec::new()));
    // }

    let as_vec = BouldersView::with_id(&state, &gym, own).await?;
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
    let as_vec = AccountsView::all(&state, &gym).await?;
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
    let as_vec = AccountsView::admins(&state, &gym).await?;
    Ok(Json(
        as_vec
            .into_iter()
            .map(|b| b.id.expect("object in view always has an id"))
            .collect(),
    ))
}
