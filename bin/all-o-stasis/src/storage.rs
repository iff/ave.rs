use axum::Json;
use firestore::{path, FirestoreResult};
use futures::stream::BoxStream;
use futures::TryStreamExt;
use otp::types::{
    ObjId, Object, ObjectId, Operation, Patch, RevId, Snapshot, ROOT_PATH, ZERO_REV_ID,
};
use otp::{apply, rebase};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::routes::PatchObjectResponse;
use crate::{AppError, AppState};

struct PatchState {
    object_type: String,
    object_id: ObjectId,
    revision_id: RevId,
    committer_id: ObjId,
    operations: Vec<Operation>,
    num_consumed_operations: u32,
    base_snapshot: Snapshot,
    latest_snapshot: Snapshot,
    previous_patches: Vec<Patch>,
    patches: Vec<Patch>,
}

fn base_id(obj_id: &ObjectId) -> ObjId {
    match obj_id {
        ObjectId::Base(id) => id.clone(),
        ObjectId::Release(id, _) => id.clone(),
        ObjectId::Authorization(id) => id.clone(),
    }
}

async fn lookup_object_type(_obj_id: Object) -> String {
    // TODO for now we just assume it is a boulder
    // later we only need to also support Accounts, atm it does not make sense to be more flexible
    // but still we need to check the registered types (given an object id)?
    // or use an enum in objects that enumerates all possibilities (also easy to query)
    String::from("boulder")
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

fn patches_after_revision(obj_id: &ObjectId, rev_id: &RevId) -> Vec<Patch> {
    todo!()
}

fn apply_patches(base_snapshot: &Snapshot, previous_patches: &Vec<Patch>) -> Snapshot {
    todo!()
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
    let parent_path = state.db.parent_path("gyms", gym)?;

    // first check that the object exists. We'll need its metadata later
    let id = base_id(&obj_id);
    let obj = lookup_object_(state, gym, id).await?;
    let ot_type = lookup_object_type(obj).await;

    // The 'Snapshot' against which the submitted operations were created
    let base_snapshot = lookup_snapshot(&state, &gym, &obj_id, &rev_id).await?;

    // If there are any patches which the client doesn't know about we need
    // to let her know.
    let previous_patches = patches_after_revision(&obj_id, &rev_id);
    let latest_snapshot = apply_patches(&base_snapshot, &previous_patches);

    // Apply the operations and get the final snapshot.
    let mut ps = PatchState {
        object_type: ot_type,
        object_id: obj_id,
        revision_id: rev_id,
        committer_id: author,
        operations,
        num_consumed_operations: 0,
        base_snapshot,
        latest_snapshot,
        previous_patches: previous_patches.clone(),
        patches: Vec::new(),
    };

    // FIXME return values??
    let (_snapshot, patch_state) = patch_handler(&mut ps, !skip_validation).await?;

    //   -- Update object views.
    //   unless novalidate $ do
    //       content <- parseValue snapshotContent
    //       updateObjectViews ot baseObjId (Just content)

    Ok(Json(PatchObjectResponse::new(
        previous_patches,
        patch_state.num_consumed_operations,
        patch_state.patches,
    )))
}

async fn patch_handler(
    patch_state: &mut PatchState,
    validate: bool,
) -> Result<(Snapshot, PatchState), AppError> {
    todo!()
    // for each op in patch_state.operation:
    //  save_operation(patch_state, validate, patch_state.base_snapshot, patch_state.latest_snapshot, op)
}

async fn save_operation(
    patch_state: &mut PatchState,
    validate: bool,
    base_content: Value,
    snapshot: Snapshot,
    op: Operation,
) -> Result<Snapshot, AppError> {
    match rebase(base_content, op, patch_state.previous_patches) {
        None => return Ok(snapshot),
        Some(new_op) => {
            let rev_id = snapshot.revision_id + 1;
            let patch = Patch {
                object_id: patch_state.object_id,
                revision_id: rev_id,
                author_id: patch_state.committer_id,
                created_at: None,
                operation: new_op,
            };

            let new_content = apply(snapshot.content, new_op)?;
            if new_content == snapshot.content {
                return Ok(snapshot);
            }
            if validate {
                // TODO: validateWithType psObjectType newContent
            }

            let new_snapshot = Snapshot {
                object_id: snapshot.object_id,
                revision_id: rev_id,
                content: new_content,
            };

            // Now we know that the patch can be applied cleanly, so
            // we can save it in the database.
            // TODO savePatch

            // TODO
            // modify $ \s -> s
            //     { psPatches = psPatches ++ [patch]
            //     , psNumConsumedOperations = psNumConsumedOperations + 1
            //     }

            // TODO saveSnapshot

            return Ok(new_snapshot);
        }
    }
}
