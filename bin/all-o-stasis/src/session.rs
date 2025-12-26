use crate::passport::Session;
use crate::types::{AccountRole, AccountsView};
use crate::{AppError, AppState};
use axum_extra::extract::cookie::Cookie;
use otp::ObjectId;

// pub(crate) async fn author_from_session(
//     state: &AppState,
//     gym: &String,
//     session_id: Option<&Cookie<'static>>,
// ) -> Result<Option<String>, AppError> {
//     let session_id = if let Some(session_id) = session_id {
//         session_id.value().to_owned()
//     } else {
//         return Ok(None);
//         // FIXME why do we allow this?
//         // return Ok(String::from(""));
//     };
//
//     let parent_path = state.db.parent_path("gyms", gym)?;
//     let session: Option<Session> = state
//         .db
//         .fluent()
//         .select()
//         .by_id_in(SESSIONS_COLLECTION)
//         .parent(&parent_path)
//         .obj()
//         .one(&session_id)
//         .await?;
//
//     if let Some(session) = session {
//         Ok(Some(session.obj_id))
//     } else {
//         Err(AppError::NotAuthorized())
//     }
// }

pub(crate) async fn author_from_session(
    state: &AppState,
    gym: &String,
    session_id: Option<&Cookie<'static>>,
) -> Result<String, AppError> {
    let session_id = if let Some(session_id) = session_id {
        session_id.value().to_owned()
    } else {
        // FIXME why do we allow this?
        // should be option?
        return Ok(String::from(""));
    };

    let parent_path = state.db.parent_path("gyms", gym)?;
    let session: Option<Session> = state
        .db
        .fluent()
        .select()
        .by_id_in(Session::COLLECTION)
        .parent(&parent_path)
        .obj()
        .one(&session_id)
        .await?;

    if let Some(session) = session {
        Ok(session.obj_id)
    } else {
        Err(AppError::NotAuthorized())
    }
}

pub(crate) async fn account_role(
    state: &AppState,
    gym: &String,
    object_id: &ObjectId,
) -> Result<AccountRole, AppError> {
    let account = AccountsView::with_id(state, gym, object_id.clone()).await?;
    Ok(account.role)
}
