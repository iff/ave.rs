use axum::Router;
use axum::extract::{Path, State};
use axum::response::Json;
use axum::routing::get;
use chrono::{DateTime, Datelike};
use firestore::{FirestoreResult, path_camel_case};
use futures::TryStreamExt;
use futures::stream::BoxStream;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::storage::BOULDERS_VIEW_COLLECTION;
use crate::types::Boulder;
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
    Router::new()
        // everything has to be set?
        //     :> Capture "setterId" ObjId
        //     :> Capture "year" Integer
        //     :> Capture "month" Int -- 1..12
        //     :> Get '[JSON] SetterMonthlyStats
        .route("/{gym}/stats/{setter_id}/{year}/{month}", get(stats))
        // TODO what does this do?
        //     :> Get '[JSON] [BoulderStat]
        .route("/{gym}/stats/boulders", get(stats_boulders))
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
