use crate::passport::Session;
use crate::types::{Account, Boulder};
use axum::Json;
use firestore::{path_camel_case, FirestoreQueryDirection, FirestoreResult};
use futures::stream::BoxStream;
use futures::TryStreamExt;
use otp::types::{Object, ObjectId, ObjectType, Operation, Patch, RevId, Snapshot};
use otp::{apply, rebase, ROOT_OBJ_ID, ROOT_PATH, ZERO_REV_ID};
use serde_json::{from_value, Value};

use crate::routes::{LookupObjectResponse, PatchObjectResponse};
use crate::{AppError, AppState};

pub const ACCOUNTS_VIEW_COLLECTION: &str = "accounts_view";
pub const BOULDERS_VIEW_COLLECTION: &str = "boulders_view";
pub const OBJECTS_COLLECTION: &str = "objects";
pub const PATCHES_COLLECTION: &str = "patches";
pub const SESSIONS_COLLECTION: &str = "sessions";
pub const SNAPSHOTS_COLLECTION: &str = "snapshots";

// TODO generic store op using templates and table name?
pub(crate) async fn save_session(
    state: &AppState,
    gym: &String,
    session: &Session,
) -> Result<Option<Patch>, AppError> {
    let parent_path = state.db.parent_path("gyms", gym)?;
    let p: Option<Patch> = state
        .db
        .fluent()
        .insert()
        .into(SESSIONS_COLLECTION)
        .generate_document_id() // FIXME do generate an id here?
        .parent(&parent_path)
        .object(session)
        .execute()
        .await?;

    match p.clone() {
        Some(p) => tracing::debug!("storing: {p}"),
        None => tracing::debug!("failed to store: {session}"),
    }

    Ok(p)
}

// TODO generic store op using templates and table name?
pub(crate) async fn store_patch(
    state: &AppState,
    gym: &String,
    patch: &Patch,
) -> Result<Option<Patch>, AppError> {
    let parent_path = state.db.parent_path("gyms", gym)?;
    let p: Option<Patch> = state
        .db
        .fluent()
        .insert()
        .into(PATCHES_COLLECTION)
        .generate_document_id() // FIXME do generate an id here?
        .parent(&parent_path)
        .object(patch)
        .execute()
        .await?;

    match p.clone() {
        Some(p) => tracing::debug!("storing: {p}"),
        None => tracing::debug!("failed to store: {patch}"),
    }

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
        .into(SNAPSHOTS_COLLECTION)
        .generate_document_id() // FIXME do generate an id here?
        .parent(&parent_path)
        .object(snapshot)
        .execute()
        .await?;

    match p.clone() {
        Some(p) => tracing::debug!("storing: {p}"),
        None => tracing::debug!("failed to store: {snapshot}"),
    }

    Ok(p)
}

pub(crate) async fn create_object(
    state: &AppState,
    gym: &String,
    author_id: ObjectId,
    object_type: ObjectType,
    value: Value,
) -> Result<Object, AppError> {
    let parent_path = state.db.parent_path("gyms", gym)?;
    let obj = Object::new(object_type, ROOT_OBJ_ID.to_owned());
    let obj: Option<Object> = state
        .db
        .fluent()
        .insert()
        .into(OBJECTS_COLLECTION)
        .generate_document_id()
        .parent(&parent_path)
        .object(&obj)
        .execute()
        .await?;

    let obj = obj.ok_or_else(AppError::Query)?;

    let op = Operation::Set {
        path: ROOT_PATH.to_string(),
        value: Some(value.clone()),
    };
    let patch = Patch {
        object_id: obj.id(),
        revision_id: ZERO_REV_ID,
        author_id,
        created_at: None,
        operation: op,
    };
    let patch = store_patch(&state, &gym, &patch).await?;
    let _ = patch.ok_or_else(AppError::Query)?;

    update_view(state, gym, &obj.id(), &value).await?;

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
    let obj: Object = state
        .db
        .fluent()
        .select()
        .by_id_in(OBJECTS_COLLECTION)
        .parent(&parent_path)
        .obj()
        .one(&object_id)
        .await?
        .ok_or(AppError::Query())?;

    match obj.object_type {
        ObjectType::Account => {
            let account = from_value::<Account>(content.clone())
                .map_err(|e| AppError::ParseError(format!("{} in: {}", e, content)))?;

            let _: Option<Account> = state
                .db
                .fluent()
                .update()
                .in_col(ACCOUNTS_VIEW_COLLECTION)
                .document_id(object_id.clone())
                .parent(&parent_path)
                .object(&account)
                .execute()
                .await?;
        }
        ObjectType::Boulder => {
            let boulder = from_value::<Boulder>(content.clone())
                .map_err(|e| AppError::ParseError(format!("{} in: {}", e, content)))?;

            let _: Option<Boulder> = state
                .db
                .fluent()
                .update()
                .in_col(BOULDERS_VIEW_COLLECTION)
                .document_id(object_id.clone())
                .parent(&parent_path)
                .object(&boulder)
                .execute()
                .await?;
        }
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
    let obj: Object = state
        .db
        .fluent()
        .select()
        .by_id_in(OBJECTS_COLLECTION)
        .parent(&parent_path)
        .obj()
        .one(&id)
        .await?
        .ok_or(AppError::Query())?;

    tracing::debug!("looking up last snapshot for obj={id}");
    let snapshot = lookup_latest_snapshot(state, gym, &id.clone()).await?;
    let created_at = obj.created_at.ok_or(AppError::Query())?;

    Ok(Json(LookupObjectResponse {
        id,
        ot_type: "boulder".to_string(), //obj.object_type.to_string(),
        created_at,
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
        .from(SNAPSHOTS_COLLECTION)
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
            let snapshot = Snapshot::new(obj_id.clone());
            store_snapshot(state, gym, &snapshot).await?;
            snapshot
        }
    };

    // get all patches which we need to apply on top of the snapshot to
    // arrive at the desired revision
    let patches = patches_after_revision(state, gym, obj_id, latest_snapshot.revision_id).await?;

    // apply those patches to the snapshot
    apply_patches(&latest_snapshot, &patches)
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
        .from(SNAPSHOTS_COLLECTION)
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
            let snapshot = Snapshot::new(obj_id.clone());
            store_snapshot(state, gym, &snapshot).await?;
            Ok(snapshot)
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
        .from(PATCHES_COLLECTION)
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

fn apply_patch_to_snapshot(snapshot: &Snapshot, patch: &Patch) -> Result<Snapshot, AppError> {
    let s = Snapshot {
        object_id: snapshot.object_id.clone(),
        revision_id: patch.revision_id,
        content: apply(snapshot.content.clone(), patch.operation.clone())?,
    };
    tracing::debug!("applying patch={patch} to {snapshot} results in snapshot={s}");
    Ok(s)
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
    rev_id: RevId, // TODO this is what? first is 0?
    author: ObjectId,
    operations: Vec<Operation>,
    skip_validation: bool,
) -> Result<Json<PatchObjectResponse>, AppError> {
    // first check that the object exists. We'll need its metadata later
    // let id = base_id(&obj_id);

    // the 'Snapshot' against which the submitted operations were created
    // this only contains patches until base_snapshot.revision_id
    tracing::debug!("looking up base_snapshot@rev{rev_id}");
    let base_snapshot = lookup_snapshot(state, gym, &obj_id, rev_id).await?;
    tracing::debug!("base_snapshot={base_snapshot}");

    // if there are any patches which the client doesn't know about we need
    // to let her know
    // TODO cant we have patches that are not applied above but are now missing?
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
            Err(e) => return Err(e),
        }
    }

    // TODO update boulder/account view here? to make queries possible?
    // so in Avers we had generic Views that provided this interface
    //
    //      viewObjectTransformer :: obj -> Avers (Maybe a)
    //      (here this would just be serde trying to parse the Json)
    //
    // and concrete types implemented this transform to store concrete queriable
    // data in the database:
    //
    // FIXME why using validate here? validation and view update is the same?
    // unless novalidate $ do
    // FIXME this is the wrong snapshot - we dont return the one with the op applied
    // update_boulder_view(state, gym, &latest_snapshot).await?;

    Ok(Json(PatchObjectResponse::new(
        previous_patches,
        patches.len(),
        patches,
    )))

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
}

/// try rebase and then apply the operation to get a new snapshot (or return the old)
async fn save_operation(
    state: &AppState,
    gym: &String,
    object_id: ObjectId,
    author_id: ObjectId,
    base_content: Value,
    snapshot: &Snapshot,
    previous_patches: Vec<Patch>,
    op: Operation,
    validate: bool,
) -> Result<Option<Patch>, AppError> {
    let Some(new_op) = rebase(base_content, op, previous_patches) else {
        tracing::debug!("error: rebase failed!");
        return Ok(None);
    };

    tracing::debug!("save_operation: {snapshot}, op={new_op}");
    // FIXME clone?
    let new_content = apply(snapshot.content.clone(), new_op.clone())?;
    if new_content == snapshot.content {
        tracing::debug!("skipping save operation: content did not change");
        return Ok(None);
    }
    if validate {
        // TODO: validateWithType psObjectType newContent
    }

    let rev_id = snapshot.revision_id + 1;
    // now we know that the patch can be applied cleanly, so we can store both
    let new_snapshot = Snapshot {
        object_id: snapshot.object_id.clone(),
        revision_id: rev_id,
        content: new_content,
    };
    store_snapshot(state, gym, &new_snapshot)
        .await?
        .ok_or_else(AppError::Query)?;

    // TODO moved to here
    update_view(state, gym, &new_snapshot.object_id, &new_snapshot.content).await?;

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
