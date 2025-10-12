use crate::passport::Session;
use crate::types::{Account, Boulder};
use axum::Json;
use firestore::{FirestoreQueryDirection, FirestoreResult, path_camel_case};
use futures::TryStreamExt;
use futures::stream::BoxStream;
use otp::types::{Object, ObjectDoc, ObjectId, ObjectType, Patch, RevId, Snapshot, ZERO_REV_ID};
use otp::{Operation, rebase};
use serde_json::{Value, from_value};

use crate::routes::{LookupObjectResponse, PatchObjectResponse};
use crate::{AppError, AppState};

pub const ACCOUNTS_VIEW_COLLECTION: &str = "accounts_view";
pub const BOULDERS_VIEW_COLLECTION: &str = "boulders_view";
pub const OBJECTS_COLLECTION: &str = "objects";
pub const PATCHES_COLLECTION: &str = "patches";
pub const SESSIONS_COLLECTION: &str = "sessions";
pub const SNAPSHOTS_COLLECTION: &str = "snapshots";

macro_rules! store {
    ($state:expr, $gym:expr, $entity:expr, $collection:expr) => {{
        let parent_path = $state.db.parent_path("gyms", $gym)?;
        let result = $state
            .db
            .fluent()
            .insert()
            .into($collection)
            .generate_document_id()
            .parent(&parent_path)
            .object($entity)
            .execute()
            .await?;

        match &result {
            Some(r) => tracing::debug!("storing: {r}"),
            None => tracing::warn!("failed to store: {}", $entity),
        }

        result
    }};
}

// TODO only diff here is that we provide an id and update
pub(crate) async fn save_session(
    state: &AppState,
    gym: &String,
    session: &Session,
    session_id: &str,
) -> Result<Session, AppError> {
    let parent_path = state.db.parent_path("gyms", gym)?;
    let p: Option<Session> = state
        .db
        .fluent()
        .update()
        .in_col(SESSIONS_COLLECTION)
        .document_id(session_id)
        .parent(&parent_path)
        .object(session)
        .execute()
        .await?;

    match p {
        Some(p) => {
            tracing::debug!("storing session: {p}");
            Ok(p)
        }
        None => {
            tracing::warn!("failed to update session: {session} (no such object exists");
            Err(AppError::NoSession())
        }
    }
}

pub(crate) async fn create_object(
    state: &AppState,
    gym: &String,
    author_id: ObjectId,
    object_type: ObjectType,
    value: &Value,
) -> Result<Object, AppError> {
    let obj_doc = ObjectDoc::new(object_type);
    let obj_doc: Option<ObjectDoc> = store!(state, gym, &obj_doc, OBJECTS_COLLECTION);
    let obj_doc = obj_doc.ok_or(AppError::Query(
        "create_object: failed to create object".to_string(),
    ))?;

    let obj: Object = obj_doc
        .try_into()
        .map_err(|e| AppError::Query(format!("create_object: {e}")))?;

    let patch = Patch::new(obj.id.clone(), author_id, value);
    let patch: Option<Patch> = store!(state, gym, &patch, PATCHES_COLLECTION);
    let _ = patch.ok_or(AppError::Query(
        "create_object: failed to store patch".to_string(),
    ))?;

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
        .by_id_in(OBJECTS_COLLECTION)
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
        ObjectType::Account => {
            let account = from_value::<Account>(content.clone())
                .map_err(|e| AppError::ParseError(format!("{e} in: {content}")))?;

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
                .map_err(|e| AppError::ParseError(format!("{e} in: {content}")))?;

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
    let obj: ObjectDoc = state
        .db
        .fluent()
        .select()
        .by_id_in(OBJECTS_COLLECTION)
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
            let _: Option<Snapshot> = store!(state, gym, &snapshot, SNAPSHOTS_COLLECTION);
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
            let _: Option<Snapshot> = store!(state, gym, &snapshot, SNAPSHOTS_COLLECTION);
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
        object_id: snapshot.object_id.to_owned(),
        revision_id: patch.revision_id,
        content: patch.operation.apply_to(snapshot.content.clone())?,
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
            &previous_patches,
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
    validate: bool,
) -> Result<Option<Patch>, AppError> {
    let new_op = match rebase(
        base_content,
        op,
        previous_patches.iter().map(|p| &p.operation),
    ) {
        Ok(Some(new_op)) => new_op,
        Ok(None) => {
            tracing::warn!("rebase had a conflicting patch");
            return Ok(None);
        }
        Err(e) => {
            tracing::error!("rebase failed with error: {e}");
            return Ok(None);
        }
    };

    // tracing::debug!("save_operation: {snapshot}, op={new_op}");
    // FIXME clone?
    let new_content = new_op.apply_to(snapshot.content.to_owned())?;
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
        object_id: snapshot.object_id.to_owned(),
        revision_id: rev_id,
        content: new_content,
    };
    let s: Option<Snapshot> = store!(state, gym, &new_snapshot, SNAPSHOTS_COLLECTION);
    s.ok_or(AppError::Query("storing snapshot failed".to_string()))?;

    // FIXME moved to here but we should probably only do that for the final snapshot?
    update_view(state, gym, &new_snapshot.object_id, &new_snapshot.content).await?;

    let patch = Patch {
        object_id,
        revision_id: rev_id,
        author_id,
        created_at: None,
        operation: new_op.to_owned(),
    };
    let p: Option<Patch> = store!(state, gym, &patch, PATCHES_COLLECTION);
    p.ok_or(AppError::Query("storing patch failed".to_string()))?;

    // TODO maybe await here? or return futures?
    Ok(Some(patch))
}
