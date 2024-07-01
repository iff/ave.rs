use firestore::{path, FirestoreResult};
use futures::stream::BoxStream;
use futures::TryStreamExt;
use otp::rebase;
use otp::types::{
    ObjId, Object, ObjectId, Operation, Patch, RevId, Snapshot, ROOT_PATH, ZERO_REV_ID,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{AppError, AppState};

pub struct PatchState {
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

fn lookup_object_type(obj_id: Object) -> String {
    todo!()
    // types <- objectTypes <$> gets hConfig
    // case find (\(SomeObjectType ObjectType{..}) -> otType == objType) types of
    //     Nothing -> throwError $ UnknownObjectType objType
    //     Just x  -> return x
}

pub async fn lookup_object_(state: &AppState, gym: &String, id: ObjId) -> Result<Object, AppError> {
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
    let ot_type = lookup_object_type(obj);

    // The 'Snapshot' against which the submitted operations were created
    let base_snapshot = lookup_snapshot(&obj_id, &rev_id);

    // If there are any patches which the client doesn't know about we need
    // to let her know.
    let previous_patches = patches_after_revision(&obj_id, &rev_id);
    let latest_snapshot = apply_patches(&base_snapshot, &previous_patches);

    // Apply the operations and get the final snapshot.
    let ps = PatchState {
        object_type: ot_type,
        object_id: obj_id,
        revision_id: rev_id,
        committer_id: author,
        operations,
        num_consumed_operations: 0,
        base_snapshot,
        latest_snapshot,
        previous_patches,
        patches: Vec::new(),
    };

    let (_snapshot, patch_state) = patch_handler(&ps, !skip_validation)?;

    //   -- Update object views.
    //   unless novalidate $ do
    //       content <- parseValue snapshotContent
    //       updateObjectViews ot baseObjId (Just content)

    Ok(Json(PatchObjectResponse {
        previous_patches: previous_patches.clone(),
        num_processed_operations: patch_state.num_consumed_operations,
        resulting_patches: patch_state.patches,
    }))
}

pub fn patch_handler(
    patch_state: &PatchState,
    validate: bool,
) -> Result<(Snapshot, PatchState), AppError> {
    todo!()
    // -   patchHandler :: (FromJSON a) => Bool -> AversPatch a Snapshot
    // patchHandler novalidate = do
    //     PatchState{..} <- get
    //     foldM (saveOperation $ snapshotContent psBaseSnapshot)
    //         psLatestSnapshot psOperations
    //
    //   where
}

pub fn save_opeation(patch_state: &PatchState, validate: bool) -> Result<Snapshot, AppError> {
    todo!()
    // call rebase_operation
    // handle op result
    // match rebase(patch_state.base_snapshot, patch_state.operations, patch_state.previous_patches) {}

    //     saveOperation baseContent snapshot@Snapshot{..} op = do
    //         PatchState{..} <- get
    //
    //         case rebaseOperation baseContent op psPreviousPatches of
    //             Nothing -> return snapshot
    //             Just op' -> do
    //                 now <- liftIO $ getCurrentTime
    //
    //                 let revId = succ snapshotRevisionId
    //                     patch = Patch psObjectId revId psCommitterId now op'
    //
    //                 case applyOperation snapshotContent op' of
    //                     Left e -> error $ "Failure: " ++ (show e)
    //                     Right newContent
    //                         | newContent /= snapshotContent -> do
    //                             unless novalidate $ do
    //                                 lift $ validateWithType psObjectType newContent
    //
    //                             let newSnapshot = snapshot { snapshotContent    = newContent
    //                                                        , snapshotRevisionId = revId
    //                                                        }
    //
    //                             -- Now we know that the patch can be applied cleanly, so
    //                             -- we can save it in the database.
    //                             lift $ savePatch patch
    //
    //                             modify $ \s -> s
    //                                 { psPatches = psPatches ++ [patch]
    //                                 , psNumConsumedOperations = psNumConsumedOperations + 1
    //                                 }
    //
    //                             lift $ saveSnapshot newSnapshot
    //                             return newSnapshot
    //                         | otherwise -> return snapshot
    //
}
