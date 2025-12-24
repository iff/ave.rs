use crate::{
    AppError, AppState,
    routes::{LookupObjectResponse, PatchObjectResponse},
    types::{AccountsView, BouldersView, Object, ObjectDoc, ObjectType, Patch, Snapshot},
};
use axum::Json;
use firestore::{FirestoreQueryDirection, FirestoreResult, path_camel_case};
use futures::TryStreamExt;
use futures::stream::BoxStream;
use otp::{ObjectId, Operation, RevId, ZERO_REV_ID, rebase};
use serde_json::Value;

pub(crate) async fn create_object(
    state: &AppState,
    gym: &String,
    author_id: ObjectId,
    object_type: ObjectType,
    value: &Value,
) -> Result<Object, AppError> {
    let obj_doc = ObjectDoc::new(object_type).store(state, gym).await?;
    let obj: Object = obj_doc
        .try_into()
        .map_err(|e| AppError::Query(format!("create_object: {e}")))?;

    let _ = Patch::new(obj.id.clone(), author_id, value)
        .store(state, gym)
        .await?;
    update_view(state, gym, &obj.id, value).await?;

    Ok(obj)
}

pub(crate) async fn update_view(
    state: &AppState,
    gym: &String,
    object_id: &ObjectId,
    content: &Value,
) -> Result<(), AppError> {
    let parent_path = state.db.parent_path("gyms", gym)?;

    // lookup object to find out what type it is
    let obj: ObjectDoc = state
        .db
        .fluent()
        .select()
        .by_id_in(ObjectDoc::COLLECTION)
        .parent(&parent_path)
        .obj()
        .one(&object_id)
        .await?
        .ok_or(AppError::Query(format!(
            "update_view: failed to update view for {object_id}"
        )))?;

    let obj: Object = obj
        .try_into()
        .map_err(|e| AppError::Query(format!("update_view: {e}")))?;

    match obj.object_type {
        ObjectType::Account => AccountsView::store(state, gym, object_id, content).await?,
        ObjectType::Boulder => BouldersView::store(state, gym, object_id, content).await?,
        ObjectType::Passport => {
            // no view table
        }
    };

    Ok(())
}

/// generic object lookup in `gym` with `id`
pub(crate) async fn lookup_object_(
    state: &AppState,
    gym: &String,
    id: ObjectId,
) -> Result<Json<LookupObjectResponse>, AppError> {
    let parent_path = state.db.parent_path("gyms", gym)?;
    let obj: ObjectDoc = state
        .db
        .fluent()
        .select()
        .by_id_in(ObjectDoc::COLLECTION)
        .parent(&parent_path)
        .obj()
        .one(&id)
        .await?
        .ok_or(AppError::Query(format!(
            "lookup_object: failed to get object {id}"
        )))?;

    let obj: Object = obj
        .try_into()
        .map_err(|e| AppError::Query(format!("lookup_object: {e}")))?;

    tracing::debug!("looking up last snapshot for obj={id}");
    let snapshot = lookup_latest_snapshot(state, gym, &id.clone()).await?;

    Ok(Json(LookupObjectResponse {
        id,
        ot_type: obj.object_type,
        created_at: obj.created_at,
        created_by: obj.created_by,
        revision_id: snapshot.revision_id,
        content: snapshot.content,
    }))
}

pub(crate) async fn lookup_latest_snapshot(
    state: &AppState,
    gym: &String,
    obj_id: &ObjectId,
) -> Result<Snapshot, AppError> {
    // same as lookup_snapshot but not with upper bound
    let parent_path = state.db.parent_path("gyms", gym)?;
    let object_stream: BoxStream<FirestoreResult<Snapshot>> = state
        .db
        .fluent()
        .select()
        .from(Snapshot::COLLECTION)
        .parent(&parent_path)
        .filter(|q| {
            q.for_all([
                q.field(path_camel_case!(Snapshot::object_id)).eq(obj_id),
                q.field(path_camel_case!(Snapshot::revision_id))
                    .greater_than_or_equal(ZERO_REV_ID),
            ])
        })
        .limit(1)
        .order_by([(
            path_camel_case!(Snapshot::revision_id),
            FirestoreQueryDirection::Descending,
        )])
        .obj()
        .stream_query_with_errors()
        .await?;

    let snapshots: Vec<Snapshot> = object_stream.try_collect().await?;
    let latest_snapshot: Snapshot = match snapshots.first() {
        Some(snapshot) => {
            tracing::debug!("found {snapshot}");
            snapshot.clone()
        }
        None => {
            tracing::debug!("no snapshot found");
            // XXX we could already create the first snapshot on object creation?
            Snapshot::new(obj_id.clone()).store(state, gym).await?
        }
    };

    // get all patches which we need to apply on top of the snapshot to
    // arrive at the desired revision
    let patches = patches_after_revision(state, gym, obj_id, latest_snapshot.revision_id).await?;

    // apply those patches to the snapshot
    latest_snapshot.apply_patches(&patches)
}

/// get or create a snapshot between low and high (inclusive)
async fn lookup_snapshot_between(
    state: &AppState,
    gym: &String,
    obj_id: &ObjectId,
    low: RevId,
    high: RevId,
) -> Result<Snapshot, AppError> {
    let parent_path = state.db.parent_path("gyms", gym)?;
    let object_stream: BoxStream<FirestoreResult<Snapshot>> = state
        .db
        .fluent()
        .select()
        .from(Snapshot::COLLECTION)
        .parent(&parent_path)
        .filter(|q| {
            q.for_all([
                q.field(path_camel_case!(Snapshot::object_id)).eq(obj_id),
                q.field(path_camel_case!(Snapshot::revision_id))
                    .greater_than_or_equal(low),
                q.field(path_camel_case!(Snapshot::revision_id))
                    .less_than_or_equal(high),
            ])
        })
        .limit(1)
        .order_by([(
            path_camel_case!(Snapshot::revision_id),
            FirestoreQueryDirection::Descending,
        )])
        .obj()
        .stream_query_with_errors()
        .await?;

    let snapshots: Vec<Snapshot> = object_stream.try_collect().await?;
    tracing::debug!(
        "snapshots ({low} <= s <= {high}): {} snapshots, obj={obj_id}",
        snapshots.len(),
    );
    match snapshots.first() {
        Some(snapshot) => Ok(snapshot.clone()),
        None => {
            // TODO we could already create the first snapshot on object creation?
            // TODO why is initial snapshot rev = -1?
            Ok(Snapshot::new(obj_id.clone()).store(state, gym).await?)
        }
    }
}

async fn lookup_snapshot(
    state: &AppState,
    gym: &String,
    obj_id: &ObjectId,
    rev_id: RevId, // inclusive
) -> Result<Snapshot, AppError> {
    let latest_snapshot = lookup_snapshot_between(state, gym, obj_id, ZERO_REV_ID, rev_id).await?;

    // get all patches which we need to apply on top of the snapshot to
    // arrive at the desired revision
    let patches: Vec<Patch> =
        patches_after_revision(state, gym, obj_id, latest_snapshot.revision_id)
            .await?
            .into_iter()
            .filter(|p| p.revision_id <= rev_id)
            .collect();

    // apply those patches to the snapshot
    latest_snapshot.apply_patches(&patches)
}

async fn patches_after_revision(
    state: &AppState,
    gym: &String,
    obj_id: &ObjectId,
    rev_id: RevId,
) -> Result<Vec<Patch>, AppError> {
    let parent_path = state.db.parent_path("gyms", gym)?;
    let object_stream: BoxStream<FirestoreResult<Patch>> = state
        .db
        .fluent()
        .select()
        .from(Patch::COLLECTION)
        .parent(&parent_path)
        .filter(|q| {
            q.for_all([
                q.field(path_camel_case!(Patch::object_id)).eq(obj_id),
                q.field(path_camel_case!(Patch::revision_id))
                    .greater_than(rev_id),
            ])
        })
        .order_by([(
            path_camel_case!(Snapshot::revision_id),
            FirestoreQueryDirection::Ascending,
        )])
        .obj()
        .stream_query_with_errors()
        .await?;

    let patches: Vec<Patch> = object_stream.try_collect().await?;
    tracing::debug!(
        "patches after rev ({rev_id}): {}, obj = {obj_id}",
        patches.len()
    );
    Ok(patches)
}

pub async fn apply_object_updates(
    state: &AppState,
    gym: &String,
    obj_id: ObjectId,
    rev_id: RevId,
    author: ObjectId,
    operations: Vec<Operation>,
) -> Result<Json<PatchObjectResponse>, AppError> {
    // the 'Snapshot' against which the submitted operations were created
    // this only contains patches until base_snapshot.revision_id
    tracing::debug!("looking up base_snapshot@rev{rev_id}");
    let base_snapshot = lookup_snapshot(state, gym, &obj_id, rev_id).await?;
    tracing::debug!("base_snapshot={base_snapshot}");

    // if there are any patches which the client doesn't know about we need
    // to let her know
    let previous_patches = patches_after_revision(state, gym, &obj_id, rev_id).await?;
    let latest_snapshot = base_snapshot.apply_patches(&previous_patches)?;

    let mut patches = Vec::<Patch>::new();
    let mut final_snapshot = latest_snapshot.clone();
    for op in operations {
        let saved = save_operation(
            state,
            gym,
            obj_id.clone(),
            author.clone(),
            (base_snapshot.content).clone(),
            &latest_snapshot,
            &previous_patches,
            op,
        )
        .await;

        match saved {
            Err(e) => return Err(e),
            Ok(Some(saved)) => {
                patches.push(saved.patch);
                final_snapshot = saved.snapshot
            }
            Ok(None) => (), // skip
        }
    }

    update_view(
        state,
        gym,
        &final_snapshot.object_id,
        &final_snapshot.content,
    )
    .await?;

    Ok(Json(PatchObjectResponse::new(
        previous_patches,
        patches.len(),
        patches,
    )))
}

struct SaveOp {
    patch: Patch,
    snapshot: Snapshot,
}

/// Rebase and then apply the operation to the snapshot to get a new snapshot
/// Returns `None` if the rebasing fails or applying the (rebased) operation yields the same
/// snapshot.
#[allow(clippy::too_many_arguments)]
async fn save_operation(
    state: &AppState,
    gym: &String,
    object_id: ObjectId,
    author_id: ObjectId,
    base_content: Value,
    snapshot: &Snapshot,
    previous_patches: &[Patch],
    op: Operation,
) -> Result<Option<SaveOp>, AppError> {
    let rebased_op = match rebase(
        base_content,
        op,
        previous_patches.iter().map(|p| &p.operation),
    ) {
        Ok(Some(rebased_op)) => rebased_op,
        Ok(None) => {
            // TODO better error, log op, base_content
            tracing::warn!("rebase failed due to a conflict");
            return Ok(None);
        }
        Err(e) => {
            tracing::error!("rebase failed with error: {e}");
            return Ok(None);
        }
    };

    match snapshot.new_revision(object_id, author_id, rebased_op)? {
        None => Ok(None),
        Some((new_snapshot, patch)) => {
            let s = new_snapshot.store(state, gym).await?;
            let p = patch.store(state, gym).await?;
            Ok(Some(SaveOp {
                patch: p,
                snapshot: s,
            }))
        }
    }
}
