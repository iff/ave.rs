use axum::{
    Router,
    extract::{Path, State},
    response::Json,
    routing::get,
};
use axum_extra::extract::CookieJar;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::{
    AppError, AppState,
    passport::author_from_session,
    routes::api::account_role,
    types::{AccountRole, BouldersView},
};

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
struct BoulderStat {
    set_on: String,
    removed_on: Option<String>,
    setters: Vec<String>,
    sector: String,
    grade: String,
}

pub fn routes() -> Router<AppState> {
    Router::new().route("/{gym}/stats/boulders", get(stats_boulders))
}

async fn stats_boulders(
    State(state): State<AppState>,
    Path(gym): Path<String>,
    jar: CookieJar,
) -> Result<Json<Vec<BoulderStat>>, AppError> {
    let session_id = jar.get("session").ok_or(AppError::NoSession())?;
    let created_by = author_from_session(&state, &gym, session_id).await?;
    let role = account_role(&state, &gym, &created_by).await?;
    if AccountRole::User == role {
        return Err(AppError::NotAuthorized());
    }

    let transform_date = |epoch_millis: usize| {
        let date = if let Some(date) =
            DateTime::from_timestamp_millis(epoch_millis as i64)
        {
            date
        } else {
            // fall back to today?
            Utc::now()
        };
        date.format("%Y-%m-%d").to_string()
    };

    // TODO expose this as a query parameter
    let history_start_millis =
        (Utc::now() - chrono::Duration::days(120)).timestamp_millis() as usize;
    let as_vec =
        BouldersView::stats(&state, &gym, history_start_millis).await?;
    let stats: Vec<BoulderStat> = as_vec
        .into_iter()
        .map(|b| BoulderStat {
            set_on: transform_date(b.set_date),
            removed_on: if b.removed == 0 {
                None
            } else {
                Some(transform_date(b.removed))
            },
            setters: b.setter,
            sector: b.sector,
            grade: b.grade,
        })
        .collect();

    Ok(Json(stats))
}
