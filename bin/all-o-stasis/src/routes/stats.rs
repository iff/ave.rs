use axum::Router;
use axum::extract::{Path, State};
use axum::response::Json;
use axum::routing::get;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::types::BouldersView;
use crate::{AppError, AppState};

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
) -> Result<Json<Vec<BoulderStat>>, AppError> {
    let transform_date = |epoch_millis: usize| {
        let date = if let Some(date) = DateTime::from_timestamp_millis(epoch_millis as i64) {
            date
        } else {
            // fall back to today?
            Utc::now()
        };
        date.format("%Y-%m-%d").to_string()
    };

    let as_vec = BouldersView::stats(&state, &gym).await?;
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
