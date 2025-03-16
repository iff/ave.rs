/// implementing a subset of OT operations to patch serde_json::Value::Objects and serde_json::Value::Array.
// no concrete types expected here, maybe we want to change that?
use crate::types::{Operation, Patch, Path};
use serde_json::Value;
use std::error::Error;
use std::fmt;

type Object = serde_json::Map<String, Value>;

#[derive(Debug)]
pub enum PatchError {
    InconsistentTypes(),
    IndexError(String),
    KeyError(String),
    NoId(),
    PathError(String),
    Unknown(),
    ValueIsNotArray(),
}

impl Error for PatchError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        todo!()
    }
}

impl fmt::Display for PatchError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InconsistentTypes() => write!(f, "InconsistentTypes"),
            Self::IndexError(e) => write!(f, "IndexError: {}", e),
            Self::KeyError(e) => write!(f, "KeyError: {}", e),
            Self::NoId() => write!(f, "NoId"),
            Self::PathError(e) => write!(f, "PathError: {}", e),
            Self::Unknown() => write!(f, "UnknownError"),
            Self::ValueIsNotArray() => write!(f, "ValueIsNotArray"),
        }
    }
}

/// apply `op` to `value`. Panics if the operation is invalid.
pub fn apply(value: Value, operation: Operation) -> Result<Value, PatchError> {
    match operation {
        Operation::Set {
            path,
            value: op_value,
        } => {
            if path.is_empty() {
                return Ok(value);
            }

            // delete key (path) if op_Value is empty else insert key (path)
            // TODO do we want mut?
            let ins_or_del = |key: String, map: &mut Object| match op_value {
                Some(v) => map.insert(key, v),
                None => map.remove(&key),
            };
            change_object(value, path, ins_or_del)
        }
        Operation::Splice {
            path,
            index: op_index,
            remove: op_remove,
            insert: op_insert,
        } => {
            // convert op_insert (Value::Array) to Vec<Value>
            let op_insert = match op_insert.as_array() {
                Some(v) => Ok(v),
                None => Err(PatchError::ValueIsNotArray()),
            }?;

            // TODO do we want mut?
            let f = |mut a: Vec<Value>| {
                // check if the indices are within the allowed range
                if a.len() < op_index + op_remove {
                    return Err(PatchError::IndexError(format!(
                        "len: {}, index: {}, remove: {}",
                        a.len(),
                        op_index,
                        op_remove
                    )));
                };

                // The existing array and the elements we want to insert must have the same type.
                // Furthermore, if the array consists of objects, each object is required to have an "id" field.
                match (a.first(), op_insert.first()) {
                    (Some(Value::String(_)), Some(Value::String(_))) => {
                        // TODO check all elements of both to be strings only
                        ()
                    }
                    // TODO others??
                    (Some(Value::Number(_)), Some(Value::Number(_))) => {
                        // TODO check all elements of both to be strings only
                        ()
                    }
                    (Some(Value::Object(x)), Some(Value::Object(y))) => {
                        // TODO check all elements of both to be strings only
                        if !x.contains_key("id") || y.contains_key("id") {
                            return Err(PatchError::NoId());
                        }
                    }
                    _ => return Err(PatchError::InconsistentTypes()),
                };

                // TODO check that is indeed the same operation
                // wereHamster tells me it should act like js splice
                // V.take opIndex a V.++ V.fromList opInsert V.++ V.drop (opIndex + opRemove) a
                let _ = a.splice(op_index..op_index + op_remove, op_insert.iter().cloned());
                Ok(a)
            };
            change_array(value, path, f)
        }
    }
}

/// Travers the path and then either insert or delete at the very end
fn change_object<F>(mut value: Value, path: Path, f: F) -> Result<Value, PatchError>
where
    F: FnOnce(String, &mut Object) -> Option<Value>,
{
    let paths: Vec<&str> = path.split('.').collect();
    let key_to_change = *paths.last().ok_or(PatchError::PathError(path.clone()))?;

    let mut content = &mut value;

    let len = paths.len();
    for key in &paths[..(len - 1)] {
        match content.get_mut(key) {
            Some(value) => content = value,
            None => return Err(PatchError::KeyError(key.to_string())),
        }
    }

    match content {
        Value::Object(o) => Ok(Value::from(f(key_to_change.to_string(), o))),
        _ => Err(PatchError::Unknown()),
    }?;

    // FIXME new object?
    Ok(value)
}

fn change_array<F>(mut value: Value, path: Path, f: F) -> Result<Value, PatchError>
where
    F: FnOnce(Vec<Value>) -> Result<Vec<Value>, PatchError>,
{
    // FIXME almost like change_object - combine as trait?
    // resolving path and then depending on the Value::Array | Object do something different
    let paths: Vec<&str> = path.split('.').collect();
    let key_to_change = *paths.last().ok_or(PatchError::PathError(path.clone()))?;

    let mut content = &mut value;

    let len = paths.len();
    for key in &paths[..(len-1)] {
        match content.get_mut(key) {
            Some(value) => content = value,
            None => return Err(PatchError::KeyError(key.to_string())),
        }
    }

    let new_array = match content.get_mut(key_to_change) {
        Some(value) => match value {
            Value::Array(array) => Ok(Value::from(f(array.to_vec())?)),
            _ => Err(PatchError::ValueIsNotArray()),
        },
        None => Err(PatchError::KeyError(key_to_change.to_string())),
    }?;

    match content {
        Value::Object(o) => Ok(o.insert(key_to_change.to_string(), new_array)),
        _ => Err(PatchError::Unknown()),
    }?;

    Ok(value)
}

/// Apply `op` on top of `base` with values `content`.
/// Conflict resolution:
/// ```plain
/// Set (foo)        -> Set (foo)        = ok
/// Set (foo)        -> Set (foo.bar)    = drop
/// Set (foo.bar)    -> Set (foo)        = ok
/// Set (foo)        -> Set (bar)        = ok
///
/// Set (foo)        -> Splice (foo)     = drop
/// Set (foo)        -> Splice (foo.bar) = drop
/// Set (foo.bar)    -> Splice (foo)     = ok
/// Set (foo)        -> Splice (bar)     = ok
///
/// Splice (foo)     -> Set (foo)        = ok
/// Splice (foo)     -> Set (foo.bar)    = ok if foo.bar exists
/// Splice (foo.bar) -> Set (foo)        = ok
/// Splice (foo)     -> Set (bar)        = ok
///
/// Splice (foo)     -> Splice (foo)     = drop -- todo: ok (adjust)
/// Splice (foo)     -> Splice (foo.bar) = ok if foo.bar exists
/// Splice (foo.bar) -> Splice (foo)     = ok
/// Splice (foo)     -> Splice (bar)     = ok
/// ```
fn op_ot(content: &Value, base: &Operation, op: Operation) -> Option<Operation> {
    // drop duplicates
    if *base == op {
        return None;
    }

    // if neither is a prefix of the other (they touch distinct parts of the object) then it's safe to accept the op
    // FIXME
    // if !(base.path.starts_with(op.path) || op.path.starts_with(base.path)) {
    //     return Some(op);
    // }

    // FIXME clone
    match (base, op.clone()) {
        (
            Operation::Set {
                path: base_path,
                value: _,
            },
            Operation::Set {
                path: op_path,
                value: _,
            },
        ) => {
            if *base_path == op_path {
                return Some(op);
            }
            if (*base_path).starts_with(&op_path) {
                return None;
            }
            Some(op)
        }
        (
            Operation::Set {
                path: base_path,
                value: _,
            },
            Operation::Splice {
                path: op_path,
                index: _,
                remove: _,
                insert: _,
            },
        ) => {
            if *base_path == op_path {
                return None;
            }
            if (*base_path).starts_with(&op_path) {
                return None;
            }
            Some(op)
        }
        (
            Operation::Splice {
                path: base_path,
                index: _,
                remove: _,
                insert: _,
            },
            Operation::Set {
                path: op_path,
                value: _,
            },
        ) => {
            if *base_path == op_path {
                return Some(op);
            }
            if (*base_path).starts_with(&op_path) {
                if is_reachable(op_path, content) {
                    return Some(op);
                }
                return None;
            }
            Some(op)
        }
        (
            Operation::Splice {
                path: base_path,
                index: base_index,
                remove: base_remove,
                insert: base_insert,
            },
            Operation::Splice {
                path: op_path,
                index: op_index,
                remove: op_remove,
                insert: op_insert,
            },
        ) => {
            if *base_path == op_path {
                if base_index + base_remove <= op_index {
                    let base_insert_len = base_insert
                        .as_array()
                        .expect("ot with splice needs array")
                        .len();
                    //  FIXME copy/clone?
                    return Some(Operation::Splice {
                        path: op_path,
                        index: op_index + base_insert_len - op_remove,
                        remove: op_remove,
                        insert: op_insert,
                    });
                }

                if op_index + op_remove < *base_index {
                    return Some(op);
                }

                return None;
            }
            if base_path.starts_with(&op_path) {
                if is_reachable(op_path, content) {
                    return Some(op);
                }
                return None;
            }
            None
        }
    }
}

/// Check if path is reachable starting from value
fn is_reachable(path: Path, value: &Value) -> bool {
    let paths: Vec<&str> = path.split('.').collect();

    let mut content = value;

    for p in paths {
        content = match content {
            Value::Object(o) => match o.get(p) {
                Some(v) => v,
                None => return false,
            },
            Value::Array(a) => match a.iter().find(|element| match element {
                Value::Object(o) => Some(&Value::String(p.to_string())) == o.get("id"),
                _ => false,
            }) {
                Some(v) => v,
                None => return false,
            },
            _ => return false,
        }
    }

    true
}

/// Given an `op` which was created against a particular `content`, rebase it on top of patches which were created against the very same content in parallel.
///
/// This function assumes that the patches apply cleanly to the content. Otherwire the function will panic.
pub fn rebase(content: Value, op: Operation, patches: Vec<Patch>) -> Option<Operation> {
    let mut new_content = content;
    let mut op = Some(op);

    for patch in patches {
        // FIXME clone
        match apply(new_content, patch.operation.clone()) {
            Ok(value) => {
                new_content = value;
                // TODO do we skip NONE ops?
                op = op_ot(&new_content, &patch.operation, op?);
            }
            Err(e) => panic!("unexpected failure: {}", e),
        }
    }

    op
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{Operation, ROOT_PATH};
    use serde_json::json;

    // macro_rules! test_battery {
    //   ($($t:ty as $name:ident),*) => {
    //     $(
    //       mod $name {
    //         #[test]
    //         fn frobnified() { test_inner::<$t>(1, true) }
    //         #[test]
    //         fn unfrobnified() { test_inner::<$t>(1, false) }
    //       }
    //     )*
    //   }
    // }
    //
    // test_battery! {
    //   u8 as u8_tests,
    //   // ...
    //   i128 as i128_tests
    // }

    #[test]
    fn apply_set_op_root_path() {
        let op = Operation::Set {
            path: ROOT_PATH.to_string(),
            value: None,
        };

        let val = json!({ "meaning of life": 42});
        let res = apply(val.clone(), op);
        assert_eq!(res.ok(), Some(val));
    }

    #[test]
    fn apply_set_op_path_overwrite() {
        let op = Operation::Set {
            path: "x".to_string(),
            value: Some(json!({"y": 7})),
        };

        let val = json!({ "x": {"a": 42}, "z": "z"});
        let res = apply(val, op);
        let exp = json!({ "x": {"y": 7}, "z": "z"});
        assert_eq!(res.ok(), Some(exp));
    }

    #[test]
    fn apply_set_op_path_insert() {
        let op = Operation::Set {
            path: "x.y".to_string(),
            value: Some(json!({"z": 7})),
        };

        let val = json!({ "x": {"a": 42, "y": {}}, "z": "z"});
        let res = apply(val, op);
        let exp = json!({ "x": {"a": 42, "y": {"z": 7}}, "z": "z"});
        assert_eq!(res.ok(), Some(exp));
    }

    #[test]
    fn apply_set_op_path_delete() {
        let op = Operation::Set {
            path: "x.a".to_string(),
            value: None,
        };

        let val = json!({ "x": {"a": 42}, "z": "z"});
        let res = apply(val, op);
        let exp = json!({ "x": {}, "z": "z"});
        assert_eq!(res.ok(), Some(exp));
    }

    #[test]
    fn apply_splice_op_number() {
        let op = Operation::Splice {
            path: "x".to_string(),
            index: 1,
            remove: 0,
            insert: json!([42, 43]),
        };

        let val = json!({ "x": [1,2,3,4], "z": "z"});
        let res = apply(val, op);
        let exp = json!({ "x": [1, 42, 43, 2, 3, 4], "z": "z"});
        match res {
            Ok(v) => assert_eq!(v, exp),
            Err(e) => panic!("{}", e),
        }
    }

    #[test]
    fn apply_splice_op_inconsistent_types() {
        let op = Operation::Splice {
            path: "x".to_string(),
            index: 1,
            remove: 0,
            insert: json!(["42", "43"]),
        };

        let val = json!({ "x": [1,2,3,4], "z": "z"});
        let res = apply(val, op);
        match res {
            Ok(_) => panic!(),
            Err(e) => match e {
                PatchError::InconsistentTypes() => (),
                _ => panic!(),
            },
        }
    }

    // TODO test bool and Object with id
    #[test]
    fn apply_splice_op_str() {
        let op = Operation::Splice {
            path: "x".to_string(),
            index: 1,
            remove: 0,
            insert: json!(["42", "43"]),
        };

        let val = json!({ "x": ["a", "b", "c", "d"], "z": "z"});
        let res = apply(val, op);
        let exp = json!({ "x": ["a", "42", "43", "b", "c", "d"], "z": "z"});
        match res {
            Ok(v) => assert_eq!(v, exp),
            Err(e) => panic!("{}", e),
        }
    }

    #[test]
    fn apply_splice_op_remove() {
        let op = Operation::Splice {
            path: "x".to_string(),
            index: 1,
            remove: 2,
            insert: json!([42, 43]),
        };

        let val = json!({ "x": [1,2,3,4], "z": "z"});
        let res = apply(val, op);
        let exp = json!({ "x": [1, 42, 43, 4], "z": "z"});
        match res {
            Ok(v) => assert_eq!(v, exp),
            Err(e) => panic!("{}", e),
        }
    }

    // TODO what is the expected behavior here?
    // #[test]
    // fn is_reachable_empty_path() {
    //     let value = json!(null);
    //     assert!(is_reachable(Path::from(""), &value));
    // }

    #[test]
    fn is_reachable_for_primitive_values() {
        let value = json!(null);
        assert!(!is_reachable(Path::from("x"), &value));

        let value = json!("");
        assert!(!is_reachable(Path::from("x"), &value));

        let value = json!(1);
        assert!(!is_reachable(Path::from("x"), &value));

        let value = json!(true);
        assert!(!is_reachable(Path::from("x"), &value));
    }

    #[test]
    fn is_reachable_for_object() {
        let value = json!({"id": "foo", "bar": "baz"});
        assert!(is_reachable(Path::from("bar"), &value));
    }

    #[test]
    fn is_reachable_for_array() {
        let value = json!([]);
        assert!(!is_reachable(Path::from("foo.bar"), &value));

        let value = json!([{}]);
        assert!(!is_reachable(Path::from("foo.bar"), &value));

        let value = json!([{"id": "foo", "bar": "baz"}]);
        assert!(is_reachable(Path::from("foo.bar"), &value));
    }
}
