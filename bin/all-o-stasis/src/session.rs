use crate::passport::Session;
use crate::types::{AccountRole, AccountsView};
use crate::{AppError, AppState};
use axum_extra::extract::cookie::Cookie;
use otp::ObjectId;

pub(crate) async fn author_from_session(
    state: &AppState,
    gym: &String,
    session_id: &Cookie<'static>,
) -> Result<String, AppError> {
    let session_id = session_id.value().to_owned();
    let session = Session::lookup(state, gym, session_id).await?;
    Ok(session.obj_id)
}

pub(crate) async fn account_role(
    state: &AppState,
    gym: &String,
    object_id: &ObjectId,
) -> Result<AccountRole, AppError> {
    let account = AccountsView::with_id(state, gym, object_id.clone()).await?;
    Ok(account.role)
}
