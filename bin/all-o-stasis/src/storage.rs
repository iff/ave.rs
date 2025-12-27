use crate::{
    AppError, AppState,
    routes::PatchObjectResponse,
    types::{AccountsView, BouldersView, Object, ObjectType, Patch, Snapshot},
};
use axum::Json;
use otp::{ObjectId, Operation, RevId, rebase};
use serde_json::Value;

pub(crate) async fn update_view(
    state: &AppState,
    gym: &String,
    object_id: &ObjectId,
    content: &Value,
) -> Result<(), AppError> {
    let obj = Object::lookup(state, gym, object_id).await?;
    update_view_typed(state, gym, object_id, &obj.object_type, content).await
}

pub(crate) async fn update_view_typed(
    state: &AppState,
    gym: &String,
    object_id: &ObjectId,
    object_type: &ObjectType,
    content: &Value,
) -> Result<(), AppError> {
    match object_type {
        ObjectType::Account => AccountsView::store(state, gym, object_id, content).await?,
        ObjectType::Boulder => BouldersView::store(state, gym, object_id, content).await?,
        ObjectType::Passport => {
            // no view table
        }
    };

    Ok(())
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
    let base_snapshot = Snapshot::lookup(state, gym, &obj_id, rev_id).await?;
    tracing::debug!("base_snapshot={base_snapshot}");

    // if there are any patches which the client doesn't know about we need
    // to let her know
    let previous_patches = Patch::after_revision(state, gym, &obj_id, rev_id).await?;
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

    Ok(Json(PatchObjectResponse::new(previous_patches, patches)))
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
