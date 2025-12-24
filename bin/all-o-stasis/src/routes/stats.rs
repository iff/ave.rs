use axum::Router;
use axum::extract::{Path, State};
use axum::response::Json;
use axum::routing::get;
use chrono::DateTime;
use firestore::{FirestoreResult, path_camel_case};
use futures::TryStreamExt;
use futures::stream::BoxStream;
use serde::{Deserialize, Serialize};

use crate::types::{Boulder, BouldersView};
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

fn stat_date(epoch_millis: usize) -> String {
    let date = DateTime::from_timestamp_millis(epoch_millis as i64).expect("invalid timestamp");
    date.format("%Y-%m-%d").to_string()
}

pub fn routes() -> Router<AppState> {
    Router::new().route("/{gym}/stats/boulders", get(stats_boulders))
}

async fn stats_boulders(
    State(state): State<AppState>,
    Path(gym): Path<String>,
) -> Result<Json<Vec<BoulderStat>>, AppError> {
    let parent_path = state.db.parent_path("gyms", gym)?;
    // TODO this is too expensive: we read all records to compute the stats
    let object_stream: BoxStream<FirestoreResult<Boulder>> = state
        .db
        .fluent()
        .select()
        .from(BouldersView::COLLECTION)
        .parent(&parent_path)
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
