// mod types {
//     pub struct Object;
//     pub struct Operation;
//     pub struct Patch;
// }
//
// mod patching {
//     pub fn rebase(content: Value, op: Operation, patches: Vec<Patch>) -> Option<Operation>;
// }

pub(crate) mod patching;
pub(crate) mod types;

pub use crate::patching::rebase;
pub use crate::types::{Object, Operation, ROOT_OBJ_ID, ROOT_PATH, ZERO_REV_ID};
