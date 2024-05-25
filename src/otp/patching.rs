/**
 * Implementing a subset of OT operations to patch serde_json::Value::Objects and serde_json::Value::Array.
 */
use crate::otp::types::{Operation, Path};
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
            Self::Unknown() => write!(f, "UnknownError"),
            Self::ValueIsNotArray() => write!(f, "ValueIsNotArray"),
        }
    }
}

/// Apply the given op on the value. Can throw an exception if the operation is invalid.
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
            // TODO do we want mut?
            let f = |mut a: Vec<Value>| {
                // check if the indices are within the allowed range.
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
                // match (a.first(), op_insert.first()) {
                //     (Some(Value::String(_)), Some(Value::String(_))) => {
                //         // TODO check all elements of both to be strings only
                //         ()
                //     }
                //     (Some(Value::Number(_)), Some(Value::Number(_))) => {
                //         // TODO check all elements of both to be strings only
                //         ()
                //     }
                //     (Some(Value::Object(x)), Some(Value::Object(y))) => {
                //         // TODO check all elements of both to be strings only
                //         if !x.contains_key("id") || y.contains_key("id") {
                //             return Err(PatchError::NoId());
                //         }
                //     }
                //     _ => return Err(PatchError::InconsistentTypes()),
                // };

                // TODO check that is indeed the same operation
                // wereHamster tells me it should act like js splice
                // V.take opIndex a V.++ V.fromList opInsert V.++ V.drop (opIndex + opRemove) a
                let _ = a.splice(
                    op_index..op_index + op_remove,
                    op_insert.as_array().expect("is vec").iter().cloned(),
                );
                Ok(a)
            };
            change_array(value, path, f)
        }
    }
}

// pathElements :: Path -> [Text]
// fn path_elements(path: Path) -> Vec<&'static str> {
//     path.split(".").collect()
// }

/// travers the path and then either insert or delete at the very end
fn change_object<F>(mut value: Value, path: Path, f: F) -> Result<Value, PatchError>
where
    F: FnOnce(String, &mut Object) -> Option<Value>,
{
    // let paths = path_elements(path);
    // TODO use splits iterator somehow (stop before last element?) maybe not possible
    // in a sense we want a traverse that returns the last object of the path and returns that plus
    // a key
    let mut paths: Vec<&str> = path.split('.').collect();
    // let len = paths.len();
    // let key_to_change = paths[len - 1]; //paths.pop().expect("paths is non-empty");
    let key_to_change = paths.pop().expect("paths is non-empty");

    let mut content = &mut value;

    for key in &paths {
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
    let mut paths: Vec<&str> = path.split('.').collect();
    let key_to_change = paths.pop().expect("paths is non-empty");

    let mut content = &mut value;

    for key in &paths {
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

// matchObjectId :: Text -> Value -> Bool
// matchObjectId itemId (Object o) = Just (String itemId) == M.lookup "id" o
// matchObjectId _      _          = False

// | Resolve the path in the object.
// resolvePathIn :: Path -> Value -> Maybe Value
// resolvePathIn path = go (pathElements path)
//   where
//     go []     value      = Just value
//     go [""]   value      = Just value
//
//     go (x:xs) (Object o) =
//         case parse (const $ o .: x) o of
//             Error   _ -> Nothing
//             Success a -> go xs a
//
//     go (x:xs) (Array a)  =
//         maybe Nothing (go xs) $ V.find (matchObjectId x) a
//
//     go _      _          = Nothing

// Set (foo)        -> Set (foo)        = ok
// Set (foo)        -> Set (foo.bar)    = drop
// Set (foo.bar)    -> Set (foo)        = ok
// Set (foo)        -> Set (bar)        = ok
//
// Set (foo)        -> Splice (foo)     = drop
// Set (foo)        -> Splice (foo.bar) = drop
// Set (foo.bar)    -> Splice (foo)     = ok
// Set (foo)        -> Splice (bar)     = ok
//
// Splice (foo)     -> Set (foo)        = ok
// Splice (foo)     -> Set (foo.bar)    = ok if foo.bar exists
// Splice (foo.bar) -> Set (foo)        = ok
// Splice (foo)     -> Set (bar)        = ok
//
// Splice (foo)     -> Splice (foo)     = drop -- todo: ok (adjust)
// Splice (foo)     -> Splice (foo.bar) = ok if foo.bar exists
// Splice (foo.bar) -> Splice (foo)     = ok
// Splice (foo)     -> Splice (bar)     = ok

// opOT :: Value -> Operation -> Operation -> Maybe Operation
// opOT content base op
//
//     // Duplicate ops are dropped.
//     | base == op = Nothing
//
//     // If neither is a prefix of the other (they touch distinct parts of the
//     // object) then it's safe to accept the op.
//     | not ((opPath base `isPrefixOf` opPath op) || (opPath op `isPrefixOf` opPath base)) =
//         Just op
//
//     | otherwise = case base of
//         Set{..}    -> setOT opPath
//         Splice{..} -> spliceOT opPath
//
//   where
//     setOT path = case op of
//         Set{..} -- Set -> Set
//             | path == opPath             -> Just op
//             | path `isPrefixOf` opPath   -> Nothing
//             | otherwise                  -> Just op
//
//         Splice{..} -- Set -> Splice
//             | path == opPath             -> Nothing
//             | path `isPrefixOf` opPath   -> Nothing
//             | otherwise                  -> Just op
//
//     spliceOT path = case op of
//         Set{..} -- Splice -> Set
//             | path == opPath             -> Just op
//             | path `isPrefixOf` opPath   -> onlyIfPresent opPath
//             | otherwise                  -> Just op
//
//         Splice{..} -- Splice -> Splice
//             | path == opPath             -> spliceOnSplice base op
//             | path `isPrefixOf` opPath   -> onlyIfPresent opPath
//             | otherwise                  -> Nothing
//
//     onlyIfPresent path = case resolvePathIn path content of
//         Nothing -> Nothing
//         Just _  -> Just op
//
//     (Path a) `isPrefixOf` (Path b) = a `T.isPrefixOf` b
//
//     // Both ops are 'Splice' on the same path.
//     spliceOnSplice op1 op2
//         | opIndex op1 + opRemove op1 <= opIndex op2
//             = Just $ op2 { opIndex = opIndex op2 + (length $ opInsert op1) - opRemove op2 }
//
//         | opIndex op2 + opRemove op2 < opIndex op1
//             = Just op2
//
//         | otherwise = Nothing

// | Given an 'Operation' which was created against a particular 'Value'
// (content), rebase it on top of patches which were created against the very
// same content in parallel.
//
// This function assumes that the patches apply cleanly to the content.
// Failure to do so results in a fatal error.

// rebaseOperation :: Value -> Operation -> [Patch] -> Maybe Operation
// rebaseOperation _       op []     = Just op
// rebaseOperation content op (x:xs) = case applyOperation content (patchOperation x) of
//     Left e -> error $ "Unexpected failure: " ++ (show e)
//     Right newContent -> case opOT newContent (patchOperation x) op of
//         Nothing  -> Nothing
//         Just op' -> rebaseOperation newContent op' xs

#[cfg(test)]
mod tests {
    use super::*;
    use crate::otp::types::{Operation, ROOT_PATH};
    use serde_json::json;

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
    fn apply_splice_op_path() {
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
}
