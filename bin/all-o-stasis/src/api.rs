use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone)]
pub struct CreateObjectBody<T> {
    #[serde(rename = "type")]
    pub objectType: String,
    pub content: T,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct CreateObjectResponse<T> {
    pub id: String,
    #[serde(rename = "type")]
    pub objectType: String,
    pub content: T,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct PatchObjectBody<T> {
    /// The 'RevId' against which the client created the operations. This may
    /// be a bit behind if some other client submitted patches in parallel.
    revisionId: String,

    /// The operations which the client wants to store in the database.
    operations: T,
}

pub struct PatchObjectResponse {
    /// Patches which were already in the database. The submitted ops were
    /// rebased on top of these.
    previousPatches: Patch,

    /// The number of operations which were processed. This may be smaller
    /// than the number of submitted ops if the processing failed somewhere
    /// in the middle. The client can then decide what to do with those which
    /// were not accepted.
    numProcessedOperations: u32,

    /// Out of the submitted operations, these are the patches which were
    /// actually applied and stored in the database. This list may be shorter
    /// if some operations were dropped (because redundant or conflicting).
    resultingPatches: Patch,
}
