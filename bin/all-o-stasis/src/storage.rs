use axum::Json;
use firestore::struct_path::path;
use firestore::{FirestoreQueryDirection, FirestoreResult};
use futures::stream::BoxStream;
use futures::TryStreamExt;
use otp::types::{ObjId, Object, ObjectId, Operation, Patch, RevId, Snapshot};
use otp::{apply, rebase};
use serde_json::{json, Value};

use crate::routes::PatchObjectResponse;
use crate::{AppError, AppState};

fn base_id(obj_id: &ObjectId) -> ObjId {
    match obj_id {
        ObjectId::Base(id) => id.clone(),
        ObjectId::Release(id, _) => id.clone(),
        ObjectId::Authorization(id) => id.clone(),
    }
}

// TODO generic store op using templates and table name?
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
        .generate_document_id() // FIXME do generate an id here?
        .parent(&parent_path)
        .object(patch)
        .execute()
        .await?;

    Ok(p)
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
        .generate_document_id() // FIXME do generate an id here?
        .parent(&parent_path)
        .object(snapshot)
        .execute()
        .await?;

    Ok(p)
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
    rev_id: RevId,
) -> Result<Snapshot, AppError> {
    let parent_path = state.db.parent_path("gyms", gym)?;
    let object_stream: BoxStream<FirestoreResult<Snapshot>> = state
        .db
        .fluent()
        .select()
        .from("snapshots")
        .parent(&parent_path)
        .filter(|q| {
            q.for_all([
                q.field(path!(Snapshot::object_id)).eq(obj_id),
                q.field(path!(Snapshot::revision_id))
                    .greater_than_or_equal(0),
                q.field(path!(Snapshot::revision_id)).less_than(rev_id),
            ])
        })
        .limit(1)
        .order_by([(
            path!(Snapshot::revision_id),
            FirestoreQueryDirection::Descending,
        )])
        .obj()
        .stream_query_with_errors()
        .await?;

    let snapshots: Vec<Snapshot> = object_stream.try_collect().await?;
    let latest_snapshot: Snapshot = match snapshots.first() {
        Some(snapshot) => snapshot.clone(),
        None => {
            // XXX we could already create the first snapshot on object creation?
            let snapshot = Snapshot {
                object_id: obj_id.clone(),
                revision_id: -1,
                content: json!({}),
            };
            store_snapshot(state, gym, &snapshot).await?;
            snapshot
        }
    };

    // get all patches which we need to apply on top of the snapshot to
    // arrive at the desired revision
    let patches = patches_after_revision(state, gym, obj_id, latest_snapshot.revision_id)
        .await?
        .into_iter()
        .filter(|p| p.revision_id <= rev_id)
        .collect();

    // apply those patches to the snapshot
    apply_patches(&latest_snapshot, &patches)
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
        .from("patches")
        .parent(&parent_path)
        .filter(|q| {
            q.for_all([
                q.field(path!(Patch::object_id)).eq(obj_id),
                q.field(path!(Patch::revision_id)).greater_than(rev_id),
            ])
        })
        .order_by([(
            path!(Snapshot::revision_id),
            FirestoreQueryDirection::Ascending,
        )])
        .obj()
        .stream_query_with_errors()
        .await?;

    let patches: Vec<Patch> = object_stream.try_collect().await?;
    Ok(patches)
}

fn apply_patch_to_snapshot(snapshot: &Snapshot, patch: &Patch) -> Result<Snapshot, AppError> {
    Ok(Snapshot {
        object_id: snapshot.object_id.clone(),
        revision_id: patch.revision_id,
        content: apply(snapshot.content.clone(), patch.operation.clone())?,
    })
}

fn apply_patches(snapshot: &Snapshot, patches: &Vec<Patch>) -> Result<Snapshot, AppError> {
    let mut s = snapshot.clone();
    for patch in patches {
        s = apply_patch_to_snapshot(&s, patch)?;
    }
    // Ok(patches.iter().fold(snapshot.clone(), |snapshot, patch| {
    //     apply_patch_to_snapshot(&snapshot, &patch)?
    // }))

    Ok(s)
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

    // The 'Snapshot' against which the submitted operations were created
    let base_snapshot = lookup_snapshot(state, gym, &obj_id, rev_id).await?;

    // If there are any patches which the client doesn't know about we need
    // to let her know
    let previous_patches = patches_after_revision(state, gym, &obj_id, rev_id).await?;
    let latest_snapshot = apply_patches(&base_snapshot, &previous_patches)?;

    let mut patches = Vec::<Patch>::new();
    for op in operations {
        let patch = save_operation(
            state,
            gym,
            obj_id.clone(),
            author.clone(),
            (base_snapshot.content).clone(),
            &latest_snapshot,
            previous_patches.clone(),
            op,
            !skip_validation,
        )
        .await; // TODO await all? does not matter that much probably?

        match patch {
            Ok(Some(val)) => patches.push(val),
            Ok(None) => (), // TODO push nones?
            Err(_e) => (),  // Some(Err(e)), FIXME handle err?
        }
    }

    // FIXME async in closure - can we separate this out? we only need async for actually storing
    // the patch and snapshot in the database?
    // let patches = operations.iter().map(|&op| {
    //     save_operation(
    //         &state,
    //         &gym,
    //         obj_id.clone(),
    //         author.clone(),
    //         (base_snapshot.content).clone(),
    //         &latest_snapshot,
    //         previous_patches.clone(),
    //         op,
    //         !skip_validation,
    //     )
    // });
    //
    // let concret_patches = patches.await?;
    // let ps = concret_patches
    //     .filter_map(|p| match p {
    //         Ok(Some(val)) => Some(val),
    //         Ok(None) => None,
    //         Err(_e) => None, // Some(Err(e)), FIXME handle err?
    //     })
    //     .collect::<Vec<Patch>>();

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
    store_snapshot(state, gym, &new_snapshot)
        .await?
        .ok_or_else(AppError::Query)?;

    let patch = Patch {
        object_id,
        revision_id: rev_id,
        author_id,
        created_at: None,
        operation: new_op.clone(),
    };
    store_patch(state, gym, &patch)
        .await?
        .ok_or_else(AppError::Query)?;

    // TODO maybe await here? or return futures?

    Ok(Some(patch))
}
