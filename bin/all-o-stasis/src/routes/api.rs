use std::net::SocketAddr;

use crate::passport::Session;
use crate::session::{account_role, author_from_session};
use crate::storage::{apply_object_updates, create_object, lookup_object_};
use crate::types::{AccountRole, Boulder, BouldersView, Object, ObjectDoc, ObjectType, Patch};
use crate::ws::handle_socket;
use crate::{AppError, AppState};
use axum::{
    Router,
    extract::ws::WebSocketUpgrade,
    extract::{Path, State, connect_info::ConnectInfo},
    response::{IntoResponse, Json},
    routing::{any, delete, get, patch, post},
};
use axum_extra::TypedHeader;
use axum_extra::extract::{CookieJar, cookie::Cookie};
use axum_extra::headers::UserAgent;
use chrono::{DateTime, Utc};
use cookie::time::Duration;
use firestore::{FirestoreResult, path_camel_case};
use futures::TryStreamExt;
use futures::stream::BoxStream;
use otp::{ObjectId, Operation, RevId};
use reqwest::StatusCode;
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CreateObjectBody {
    #[serde(rename = "type")]
    ot_type: ObjectType,
    content: Value,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CreateObjectResponse {
    id: ObjectId,
    ot_type: ObjectType,
    content: Value,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LookupObjectResponse {
    pub id: ObjectId,
    #[serde(rename = "type")]
    pub ot_type: ObjectType,
    pub created_at: DateTime<Utc>,
    pub created_by: ObjectId,
    pub revision_id: RevId,
    pub content: Value,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PatchObjectBody {
    revision_id: RevId,
    operations: Vec<Operation>,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PatchObjectResponse {
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

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LookupSessionResponse {
    id: String,
    obj_id: ObjectId,
}

async fn object_type(
    state: &AppState,
    gym: &String,
    object_id: ObjectId,
) -> Result<ObjectType, AppError> {
    let parent_path = state.db.parent_path("gyms", gym)?;
    let object_doc: Option<ObjectDoc> = state
        .db
        .fluent()
        .select()
        .by_id_in(ObjectDoc::COLLECTION)
        .parent(&parent_path)
        .obj()
        .one(&object_id)
        .await?;

    if let Some(doc) = object_doc {
        let object: Object = doc.try_into()?;
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
        .by_id_in(BouldersView::COLLECTION)
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

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/{gym}/session", get(lookup_session))
        // signout
        .route("/{gym}/session", delete(delete_session))
        .route("/{gym}/objects", post(new_object))
        .route("/{gym}/objects/{id}", get(lookup_object))
        .route("/{gym}/objects/{id}", patch(patch_object))
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
        .from(Session::COLLECTION)
        .parent(&parent_path)
        .document_id(&session_id)
        .execute()
        .await?;

    let cookie = Cookie::build(("session", session_id.clone()))
        .path("/")
        .max_age(Duration::seconds(0))
        .secure(true)
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
        .by_id_in(Session::COLLECTION)
        .parent(&parent_path)
        .obj()
        .one(&session_id)
        .await?
        .ok_or(AppError::NoSession())?;

    let cookie = Cookie::build(("session", session_id.clone()))
        .path("/")
        .max_age(Duration::weeks(52))
        .secure(true)
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
    let obj = create_object(&state, &gym, created_by, ot_type.clone(), &content).await?;

    Ok(Json(CreateObjectResponse {
        id: obj.id.clone(),
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
        .from(Patch::COLLECTION)
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

    let mut patches: Vec<Patch> = patch_stream.try_collect().await?;
    if patches.len() != 1 {
        return Err(AppError::Query(format!(
            "lookup_patch found {} patches, expecting only 1",
            patches.len()
        )));
    }
    let patch = patches.pop().unwrap();
    Ok(Json(patch))
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
