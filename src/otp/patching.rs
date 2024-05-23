// Apply the given op on the value. Can throw an exception if the operation
//   is invalid.

use crate::otp::types::{Operation, Path};
use serde_json::Value;
use std::collections::HashMap;
use std::fmt;

type Object = serde_json::Map<String, Value>;

#[derive(Debug)]
pub enum PatchingError {
    KeyError(String),
    Unknown(),
}

impl std::error::Error for PatchingError {
    //     fn provide<'a>(&'a self, request: &mut Request<'a>) {
    //         request
    //             .provide_ref::<MyBacktrace>(&self.backtrace);
    //     }
}

impl fmt::Display for PatchingError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        todo!()
        //         match f.align() {
        //             None => match self {
        //                 Error::Failed() => write!(f, "Failed"),
        //             },
        //             Some(_) => f.pad("!"), // &self.to_string()),
        //         }
    }
}

/// Apply the given op on the value. Can throw an exception if the operation is invalid.
pub fn apply(value: Value, operation: Operation) -> Result<Value, PatchingError> {
    match operation {
        Operation::Set {
            path,
            value: op_value,
        } => {
            if path.is_empty() {
                return Ok(value);
            }

            // delete key (path) if op_Value is empty
            // else insert key (path)
            let ins_or_del = |key: String, map: &mut Object| match op_value {
                Some(v) => map.insert(key, v),
                None => map.remove(&key),
            };
            return change_object(value, path, ins_or_del);
        }
        Operation::Splice {
            path,
            index,
            remove,
            insert,
        } => {
            todo!()

            // Check if the indices are within the allowed range.
            //     when (V.length a < opIndex + opRemove) $
            //         Left $ UnknownPatchError $ mconcat
            //             [ "Index out of range ("
            //             , T.pack (show $ V.length a)
            //             , ","
            //             , T.pack (show opIndex)
            //             , ","
            //             , T.pack (show opRemove)
            //             , ")"
            //             ]

            // The existing array and the elements we want to insert must match
            // structurally (have the same type). Furthermore, if the array consists
            // of objects, each object is required to have an "id" field.
            //     unless (isStructurallyEquivalent opInsert a) $
            //         Left $ UnknownPatchError "Array doesn't match structure"
            //
            //     return $ V.take opIndex a V.++ V.fromList opInsert V.++ V.drop (opIndex + opRemove) a
            //   where
            //     isStructurallyEquivalent :: [Value] -> V.Vector Value -> Bool
            //     isStructurallyEquivalent a b = strings a b || validObjects a b
            //
            //     strings      a b = all isString    a && V.all isString    b
            //     validObjects a b = all hasObjectId a && V.all hasObjectId b
        }
    }
}

// hasObjectId :: Value -> Bool
// hasObjectId (Object o) = M.member "id" o
// hasObjectId _          = False

// pathElements :: Path -> [Text]
// fn path_elements(path: Path) -> Vec<&'static str> {
//     path.split(".").collect()
// }

// fn change_object<F>(value: Value, path: Path, f: F) -> Result<Value, PatchingError>
// where
//     F: FnOnce(String, &mut Object) -> Option<Value>,
// {
//     let mut paths = path_elements(path);
//     let last = paths.pop().expect("paths is non-empty");
//     let update_map = |&mut x| match x {
//         Value::Object(o) => Ok(Value::from(f(last.to_string(), &mut o))),
//         _ => Err(PatchingError::Unknown()),
//     };
//     change_object_at(value, paths, update_map)
// }

fn change_object<F>(mut value: Value, path: Path, f: F) -> Result<Value, PatchingError>
where
    F: FnOnce(String, &mut Object) -> Option<Value>,
{
    // let paths = path_elements(path);
    let paths: Vec<&str> = path.split(".").collect();
    let len = paths.len();
    let key_to_change = paths[len - 1]; //paths.pop().expect("paths is non-empty");

    let mut content = &mut value;

    for key in &paths[..len - 2] {
        match content.get_mut(key) {
            Some(value) => content = value,
            None => return Err(PatchingError::KeyError(key.to_string())),
        }
    }

    let _ = match content {
        Value::Object(o) => Ok(Value::from(f(key_to_change.to_string(), o))),
        _ => Err(PatchingError::Unknown()),
    };

    // FIXME new object?
    return Ok(value);
}

// FIXME handle array
// we can use value.is_array() to check at runtime
// changeArray :: Value -> Path -> (Array -> PatchM Array) -> PatchM Value
// changeArray value path f = changeObjectAt value (pathElements path) $ \x ->
//     case x of
//         Array a -> fmap Array $ f a
//         _       -> Left $ UnknownPatchError "Can not change a non-array"

// TODO just for Value at the moment
// travers the path and then either insert or delete at the very end
// fn change_object_at<F>(value: Value, path: Vec<&str>, f: F) -> Result<Value, PatchingError>
// where
//     F: Fn(&mut Value) -> Result<Value, PatchingError>,
// {
//     let mut content = &value;
//
//     for key in path {
//         match content.get(key) {
//             Some(value) => content = value,
//             None => return Err(PatchingError::KeyError(key.to_string())),
//         }
//     }
//
//     f(&mut content);
//
//     // FIXME new object?
//     return Ok(value);
// }

// trait Change {
//     fn change_object_at<F>(value: &Self, path: Vec<&str>, f: F) -> Result<Object, Error>
//     where
//         F: Fn(&str) -> Result<Object, Error>,
//     {
//         todo!()
//         // try to access the first path in paths for the object
//         // that means we have untyped data in Object I guess.. need to make sure how that works with serde
//         // error if it does not exist
//         // new = change_object_at object_at_key rest_op_path f
//         // and at the end: return insert new
//     }
// }

// FIXME handle array
// changeObjectAt (Array a) (x:xs) f =
//     case V.findIndex (matchObjectId x) a of
//         Nothing    -> Left $ UnknownPatchError $ "Can not find item with id " <> T.pack (show x) <> " in the array"
//         Just index -> do
//             new <- changeObjectAt (a V.! index) xs f
//             return $ Array $ a V.// [(index, new)]

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

// #[cfg(test)]
// mod tests {
//     use super::*;
//     use crate::otp::types::{Operation, ROOT_PATH};
//
//     #[test]
//     fn apply_set_op_root_path() {
//         let op = Operation::Set {
//             path: ROOT_PATH.to_string(),
//             value: Some(2),
//         };
//
//         let res = apply(3, op);
//         assert_eq!(res.ok(), Some(3));
//     }
//
//     #[test]
//     fn apply_set_op_path() {
//         let op = Operation::Set {
//             path: String::from("x.y"),
//             value: Some(2),
//         };
//
//         let _res = apply(3, op);
//         assert!(false);
//     }
// }
