use axum::Json;
use firestore::{path, FirestoreResult};
use otp::types::{ObjId, Object, ObjectId, Operation, Patch, RevId, Snapshot};
use otp::{apply, rebase};
use serde_json::Value;

use crate::routes::PatchObjectResponse;
use crate::{AppError, AppState};

fn base_id(obj_id: &ObjectId) -> ObjId {
    match obj_id {
        ObjectId::Base(id) => id.clone(),
        ObjectId::Release(id, _) => id.clone(),
        ObjectId::Authorization(id) => id.clone(),
    }
}

/// generic object lookup in `gym` with `id`
pub(crate) async fn lookup_object_(
    state: &AppState,
    gym: &String,
    id: ObjId,
) -> Result<Object, AppError> {
    let parent_path = state.db.parent_path("gyms", gym)?;
    let obj: Option<Object> = state
        .db
        .fluent()
        .select()
        .by_id_in("objects")
        .parent(&parent_path)
        .obj()
        .one(&id)
        .await?;

    obj.ok_or(AppError::Query())
}

async fn lookup_snapshot(
    state: &AppState,
    gym: &String,
    obj_id: &ObjectId,
    rev_id: &RevId,
) -> Result<Snapshot, AppError> {
    todo!()
    // let parent_path = state.db.parent_path("gyms", gym)?;

    // snapshot <- latestSnapshotBetween objId 0 revId
    //
    // -- Get all patches which we need to apply on top of the snapshot to
    // -- arrive at the desired revision.
    // patches <- patchesAfterRevision objId (snapshotRevisionId snapshot)
    //
    // -- Apply those patches to the snapshot.
    // foldM applyPatchToSnapshot snapshot $
    //     filter (\Patch{..} -> unRevId patchRevisionId <= revId) patches
}

async fn store_patch(
    state: &AppState,
    gym: &String,
    patch: &Patch,
) -> Result<Option<Patch>, AppError> {
    let parent_path = state.db.parent_path("gyms", gym)?;
    let p: Option<Patch> = state
        .db
        .fluent()
        .insert()
        .into("patches")
        .generate_document_id() // FIXME true?
        .parent(&parent_path)
        .object(patch)
        .execute()
        .await?;
    return Ok(p);
}

async fn store_snapshot(
    state: &AppState,
    gym: &String,
    snapshot: &Snapshot,
) -> Result<Option<Snapshot>, AppError> {
    let parent_path = state.db.parent_path("gyms", gym)?;
    let p: Option<Snapshot> = state
        .db
        .fluent()
        .insert()
        .into("snapshot")
        .generate_document_id() // FIXME true?
        .parent(&parent_path)
        .object(snapshot)
        .execute()
        .await?;
    return Ok(p);
}

fn patches_after_revision(
    state: &AppState,
    gym: &String,
    obj_id: &ObjectId,
    rev_id: &RevId,
) -> Vec<Patch> {
    todo!()

    // let parent_path = state.db.parent_path("gyms", gym)?;

    // in patches table find all patches with obj_id
    // and revision between rev+1 and MAX
    // order ascending
}

fn apply_patch_to_snapshot(snapshot: &Snapshot, patch: &Patch) -> Result<Snapshot, AppError> {
    Ok(Snapshot {
        object_id: snapshot.object_id.clone(),
        revision_id: patch.revision_id,
        content: apply(snapshot.content.clone(), patch.operation.clone())?,
    })
}

fn apply_patches(base_snapshot: &Snapshot, patches: &Vec<Patch>) -> Result<Snapshot, AppError> {
    patches.iter().fold(Ok(base_snapshot), |snapshot, patch| {
        snapshot = apply_patch_to_snapshot(&snapshot, &patch)?
    })
}

pub async fn apply_object_updates(
    state: &AppState,
    gym: &String,
    obj_id: ObjectId,
    rev_id: RevId,
    author: ObjId,
    operations: Vec<Operation>,
    skip_validation: bool,
) -> Result<Json<PatchObjectResponse>, AppError> {
    // first check that the object exists. We'll need its metadata later
    let id = base_id(&obj_id);
    let obj = lookup_object_(state, gym, id).await?;

    // The 'Snapshot' against which the submitted operations were created
    let base_snapshot = lookup_snapshot(&state, &gym, &obj_id, &rev_id).await?;

    // If there are any patches which the client doesn't know about we need
    // to let her know
    let previous_patches = patches_after_revision(&state, &gym, &obj_id, &rev_id);
    let latest_snapshot = apply_patches(&base_snapshot, &previous_patches)?;

    // FIXME async in closure - can we separate this out? we only need async for actually storing
    // the patch and snapshot in the database?
    let patches = operations
        .iter()
        .map(|&op| {
            save_operation(
                &state,
                &gym,
                obj_id.clone(),
                author.clone(),
                (base_snapshot.content).clone(),
                &latest_snapshot,
                previous_patches.clone(),
                op,
                !skip_validation,
            )
        })
        .await?
        .filter_map(|p| match p {
            Ok(Some(val)) => Some(val),
            Ok(None) => None,
            Err(_e) => None, // Some(Err(e)), FIXME handle err?
        })
        .collect::<Vec<Patch>>();

    //   TODO: Update object views.
    //   unless novalidate $ do
    //       content <- parseValue snapshotContent
    //       let ot_type = obj.get_type();
    //       updateObjectViews ot baseObjId (Just content)

    Ok(Json(PatchObjectResponse::new(
        previous_patches,
        patches.len(),
        patches,
    )))
}

/// try rebase and then apply the operation to get a new snapshot (or return the old)
async fn save_operation(
    state: &AppState,
    gym: &String,
    object_id: ObjectId,
    author_id: ObjId,
    base_content: Value,
    snapshot: &Snapshot,
    previous_patches: Vec<Patch>,
    op: Operation,
    validate: bool,
) -> Result<Option<Patch>, AppError> {
    let Some(new_op) = rebase(base_content, op, previous_patches) else {
        return Ok(None);
    };

    // FIXME clone?
    let new_content = apply(snapshot.content.clone(), new_op.clone())?;
    if new_content == snapshot.content {
        return Ok(None);
    }
    if validate {
        // TODO: validateWithType psObjectType newContent
    }

    let rev_id = snapshot.revision_id + 1;
    // now we know that the patch can be applied cleanly, so we can save it in the database
    let new_snapshot = Snapshot {
        object_id: snapshot.object_id.clone(),
        revision_id: rev_id,
        content: new_content,
    };
    store_snapshot(&state, &gym, &new_snapshot)
        .await?
        .ok_or_else(AppError::Query)?;

    let patch = Patch {
        object_id,
        revision_id: rev_id,
        author_id,
        created_at: None,
        operation: new_op.clone(),
    };
    store_patch(&state, &gym, &patch)
        .await?
        .ok_or_else(AppError::Query)?;

    return Ok(Some(patch));
}
