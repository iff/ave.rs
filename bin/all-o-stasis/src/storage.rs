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
    // first check that the object exists. We'll need its metadata later
    let id = base_id(&obj_id);
    let obj = lookup_object_(state, gym, id).await?;

    // The 'Snapshot' against which the submitted operations were created
    let base_snapshot = lookup_snapshot(&state, &gym, &obj_id, &rev_id).await?;

    // If there are any patches which the client doesn't know about we need
    // to let her know
    let previous_patches = patches_after_revision(&obj_id, &rev_id);
    let latest_snapshot = apply_patches(&base_snapshot, &previous_patches);

    let patches = operations
        .iter()
        .map(|&op| {
            save_operation(
                obj_id.clone(),
                author.clone(),
                (base_snapshot.content).clone(),
                &latest_snapshot,
                previous_patches.clone(),
                op,
                !skip_validation,
            )
        })
        .filter_map(|p| match p {
            Ok(Some(val)) => Some(val),
            Ok(None) => None,
            Err(_e) => None, // Some(Err(e)), FIXME handle err?
        })
        .collect::<Vec<Patch>>();

    //   -- Update object views.
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
fn save_operation(
    object_id: ObjectId,
    author_id: ObjId,
    base_content: Value,
    snapshot: &Snapshot,
    previous_patches: Vec<Patch>,
    op: Operation,
    validate: bool,
) -> Result<Option<Patch>, AppError> {
    match rebase(base_content, op, previous_patches) {
        None => return Ok(None),
        Some(new_op) => {
            let rev_id = snapshot.revision_id + 1;
            let patch = Patch {
                object_id,
                revision_id: rev_id,
                author_id,
                created_at: None,
                operation: new_op.clone(),
            };

            // raise as OtError? or just as patch?
            let new_content = apply(snapshot.content.clone(), new_op.clone())?;
            if new_content == snapshot.content {
                return Ok(None);
            }
            if validate {
                // TODO: validateWithType psObjectType newContent
            }

            let new_snapshot = Snapshot {
                object_id: snapshot.object_id.clone(),
                revision_id: rev_id,
                content: new_content,
            };

            // now we know that the patch can be applied cleanly, so we can save it in the database
            let parent_path = state.db.parent_path("gyms", gym)?;
            let p: Option<Patch> = state
                .db
                .fluent()
                .insert()
                .into("patches")
                .generate_document_id() // FIXME true?
                .parent(&parent_path)
                .object(&patch)
                .execute()
                .await?;
            let _ = p.ok_or_else(AppError::Query)?;

            let s: Option<Snapshot> = state
                .db
                .fluent()
                .insert()
                .into("snapshots")
                .generate_document_id() // FIXME true?
                .parent(&parent_path)
                .object(&new_snapshot)
                .execute()
                .await?;
            let _ = s.ok_or_else(AppError::Query)?;

            return Ok(Some(patch));
        }
    }
}
