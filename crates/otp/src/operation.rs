use crate::OtError;
use crate::types::Path;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fmt;

type SerdeObject = serde_json::Map<String, Value>;

// TODO hide behind struct to disallow use outside?
// TODO serde serializer also needs to check
#[derive(Serialize, Deserialize, PartialEq, Debug, Clone)]
#[serde(tag = "type")]
#[serde(rename_all = "camelCase")]
pub enum Operation {
    /// applied to Value::Object for adding, updating and inserting multiple elements in a single op
    Set { path: Path, value: Option<Value> },

    /// manipulate Value::Array (remove, insert multiple elements in a single op) mimicing js/rust splice
    /// implementation
    Splice {
        path: Path,
        index: usize,
        remove: usize,
        /// this is actually a Value::Array, everything else will result in an error
        insert: Value,
    },
}

impl fmt::Display for Operation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Operation::Set { path, value } => write!(f, "Set: {path}, value={value:?}"),
            Operation::Splice {
                path,
                index,
                remove,
                insert,
            } => write!(
                f,
                "Splice: {path} @ {index}, remove={remove}, insert={insert}"
            ),
        }
    }
}

impl Operation {
    pub fn new_set(path: impl Into<Path>, value: Value) -> Self {
        Self::Set {
            path: path.into(),
            value: Some(value),
        }
    }

    pub fn try_new_set(path: impl Into<Path>, value: Option<Value>) -> Result<Self, OtError> {
        let path = path.into();
        if path.is_empty() && value.is_none() {
            Err(OtError::InvalidSetOp())
        } else {
            Ok(Self::Set { path, value })
        }
    }

    pub fn try_new_splice(
        path: impl Into<Path>,
        index: usize,
        remove: usize,
        insert: Value,
    ) -> Result<Self, OtError> {
        let path = path.into();
        if insert.is_array() {
            Err(OtError::ValueIsNotArray())
        } else {
            Ok(Self::Splice {
                path,
                index,
                remove,
                insert,
            })
        }
    }

    pub fn path(&self) -> Path {
        match self {
            Operation::Set { path, value: _ } => path.to_owned(),
            Operation::Splice {
                path,
                index: _,
                remove: _,
                insert: _,
            } => path.to_owned(),
        }
    }

    /// Apply an [`Operation`] (with a non-empty [`Path`]) to a [`Value`].
    ///
    /// Support Operations are [`Operation::Set`] and [`Operation::Splice`].
    ///
    /// Returns the [`Value`] after applying the [`Operation`] if the operation is successful.
    /// Otherwise
    /// - [`OperationError::Key`] if splice operation contains invalid keys
    /// - [`OperationError::Type`] if array types dont match or we dont work on object
    /// - [`OperationError::ValueIsNotArray`] if splice insert opertion does not contain arrays
    ///
    /// ## Set
    ///
    /// Applied to [`serde_json::Value::Object`] for adding, updating and inserting multiple
    /// elements in a single op.
    ///
    /// A set operation with empty path and no value is undefined and will return an error.
    ///
    /// ## Splice
    ///
    /// Manipulate [`serde_json::Value::Array`] (remove, insert multiple elements in a single op)
    /// mimicing js/rust splice implementation.
    /// - elements of arrays to be changed must have the same type
    /// - if the array consists of objects, each object is required to have an "id" field
    ///
    /// ## Example
    ///
    /// ```rust
    /// use serde_json::json;
    /// use otp::Operation;
    /// use otp::types::{Object, ObjectType};
    ///
    /// // An operation rebased through an empty list of patches should be unchanged
    /// let object = Object::new(ObjectType::Account);
    /// let value = serde_json::to_value(&object).unwrap();
    /// let op = Operation::Set {
    ///     path: String::from(""),
    ///     value: None,
    /// };
    ///
    /// assert!(op.apply_to(value).ok().is_none());
    /// ```
    pub fn apply_to(&self, value: Value) -> Result<Value, OtError> {
        match self {
            Operation::Set {
                path,
                value: op_value,
            } => {
                // the combination of root path and an operation with no value is invalid
                if path.is_empty() {
                    return op_value.to_owned().ok_or(OtError::Operation(String::from(
                        "set operation with an empty path and no value is undefined",
                    )));
                }

                // delete key (path) if op_Value is empty else insert key (path)
                let ins_or_del = |key: String, map: &mut SerdeObject| match op_value {
                    Some(v) => map.insert(key, v.to_owned()),
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
                    None => Err(OtError::ValueIsNotArray()),
                }?;

                let f = |mut a: Vec<Value>| {
                    // check if the indices are within the allowed range
                    if a.len() < op_index + op_remove {
                        return Err(OtError::Index(format!(
                            "len {} <= index {op_index} + remove {op_remove}",
                            a.len(),
                        )));
                    };

                    check_type_consistency(&a, op_insert)?;
                    let _ = a.splice(op_index..&(op_index + op_remove), op_insert.iter().cloned());
                    Ok(a)
                };

                change_array(value, path, f)
            }
        }
    }
}

/// Elements of arrays we want to merge/change must have the same type.
/// Furthermore, if the array consists of objects, each object is required to have an "id" field.
fn check_type_consistency(a: &[Value], b: &[Value]) -> Result<(), OtError> {
    match (a.first(), b.first()) {
        (Some(_), None) => {
            // if we only remove elements there is nothing to check
            Ok(())
        }
        (Some(Value::Number(_)), Some(Value::Number(_))) => {
            if a.iter().all(|a| a.is_number()) && b.iter().all(|a| a.is_number()) {
                Ok(())
            } else {
                Err(OtError::Type(String::from(
                    "not all array elements of type Number",
                )))
            }
        }
        (Some(Value::Bool(_)), Some(Value::Bool(_))) => {
            if a.iter().all(|a| a.is_boolean()) && b.iter().all(|a| a.is_boolean()) {
                Ok(())
            } else {
                Err(OtError::Type(String::from(
                    "not all array elements of type Bool",
                )))
            }
        }
        (Some(Value::String(_)), Some(Value::String(_))) => {
            if a.iter().all(|a| a.is_string()) && b.iter().all(|a| a.is_string()) {
                Ok(())
            } else {
                Err(OtError::Type(String::from(
                    "not all array elements of type String",
                )))
            }
        }
        (Some(Value::Object(_)), Some(Value::Object(_))) => {
            if !(a.iter().all(|a| a.is_object()) && b.iter().all(|a| a.is_object())) {
                return Err(OtError::Type(String::from(
                    "not all array elements of type Object",
                )));
            }

            // all elements are objects - do they have all have an id?
            if a.iter().all(|a| a.get("id").is_some()) && b.iter().all(|a| a.get("id").is_some()) {
                Ok(())
            } else {
                Err(OtError::NoId())
            }
        }
        _ => Err(OtError::Type(String::from("arrays have different types"))),
    }
}

/// Travers the path and then either insert or delete at the very end
fn change_object<F>(mut value: Value, path: impl Into<Path>, f: F) -> Result<Value, OtError>
where
    F: FnOnce(String, &mut SerdeObject) -> Option<Value>,
{
    let path = path.into();
    let paths: Vec<&str> = path.split('.').collect();
    let key_to_change = *paths.last().ok_or(OtError::Path(path.to_owned()))?;

    let mut content = &mut value;
    for key in &paths[..(paths.len() - 1)] {
        match content.get_mut(key) {
            Some(value) => content = value,
            None => return Err(OtError::Key(key.to_string())),
        }
    }

    match content {
        Value::Object(o) => Ok(Value::from(f(key_to_change.to_string(), o))),
        _ => Err(OtError::Type(String::from(
            "value is expected to be a Value::Object",
        ))),
    }?;
    Ok(value)
}

fn change_array<F>(mut value: Value, path: impl Into<Path>, f: F) -> Result<Value, OtError>
where
    F: FnOnce(Vec<Value>) -> Result<Vec<Value>, OtError>,
{
    // FIXME almost like change_object - combine as trait?
    // resolving path and then depending on the Value::Array | Object do something different
    let path = path.into();
    let paths: Vec<&str> = path.split('.').collect();
    let key_to_change = *paths.last().ok_or(OtError::Path(path.clone()))?;

    let mut content = &mut value;

    let len = paths.len();
    for key in &paths[..(len - 1)] {
        match content.get_mut(key) {
            Some(value) => content = value,
            None => return Err(OtError::Key(key.to_string())),
        }
    }

    let new_array = match content.get_mut(key_to_change) {
        Some(value) => match value {
            Value::Array(array) => Ok(Value::from(f(array.to_vec())?)),
            _ => Err(OtError::ValueIsNotArray()),
        },
        None => Err(OtError::Key(key_to_change.to_string())),
    }?;

    match content {
        Value::Object(o) => Ok(o.insert(key_to_change.to_string(), new_array)),
        _ => Err(OtError::Type(String::from(
            "value is expected to be a Value::Object",
        ))),
    }?;
    Ok(value)
}

// impl<'de> Deserialize<'de> for Operation {
//     fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
//     where
//         D: Deserializer<'de>,
//     {
//         // deserialize using the derived implementation into a temporary structure
//         #[derive(Deserialize)]
//         #[serde(tag = "type")]
//         #[serde(rename_all = "camelCase")]
//         enum TempOperation {
//             Set {
//                 path: Path,
//                 value: Option<Value>,
//             },
//             Splice {
//                 path: Path,
//                 index: usize,
//                 remove: usize,
//                 insert: Value,
//             },
//         }
//
//         let temp = TempOperation::deserialize(deserializer)?;
//
//         match temp {
//             TempOperation::Set { path, value } => {
//                 if path.is_empty() && value.is_none() {
//                     return Err(de::Error::custom(
//                         "Invalid Set operation: empty path with no value",
//                     ));
//                 }
//                 Ok(Operation::Set { path, value })
//             }
//             TempOperation::Splice {
//                 path,
//                 index,
//                 remove,
//                 insert,
//             } => {
//                 if !insert.is_array() {
//                     return Err(de::Error::custom("Insert value must be an array"));
//                 }
//                 Ok(Operation::Splice {
//                     path,
//                     index,
//                     remove,
//                     insert,
//                 })
//             }
//         }
//     }
// }

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::ROOT_PATH;
    use quickcheck::{Arbitrary, Gen};
    use quickcheck_macros::quickcheck;
    use serde::{Deserialize, Serialize};
    use serde_json::json;

    #[derive(Serialize, Deserialize, Debug, Clone)]
    pub struct TestObject {
        pub name: String,
        pub num: u32,
        pub maybe: bool,
    }

    impl Arbitrary for TestObject {
        fn arbitrary(g: &mut Gen) -> TestObject {
            TestObject {
                name: String::arbitrary(g),
                num: u32::arbitrary(g),
                maybe: bool::arbitrary(g),
            }
        }
    }

    #[quickcheck]
    fn apply_set_none(input: TestObject) -> bool {
        let value = serde_json::to_value(&input).expect("serialise value");
        let op = Operation::Set {
            path: ROOT_PATH.to_string(),
            value: None,
        };

        op.apply_to(value).ok().is_none()
    }

    #[quickcheck]
    fn apply_set_on_empty(input: TestObject) -> bool {
        let value = serde_json::to_value(&input).expect("serialise value");
        let op = Operation::Set {
            path: ROOT_PATH.into(),
            value: Some(value.clone()),
        };

        Some(value) == op.apply_to(json!({})).ok()
    }

    #[quickcheck]
    fn apply_set_full_overwrite(base: TestObject, overwrite: TestObject) -> bool {
        let base = serde_json::to_value(&base).expect("serialise value");
        let overwrite = serde_json::to_value(&overwrite).expect("serialise value");
        let op = Operation::Set {
            path: ROOT_PATH.into(),
            value: Some(overwrite.clone()),
        };

        Some(overwrite) == op.apply_to(base).ok()
    }

    #[quickcheck]
    fn apply_set_partial_overwrite(base: TestObject, overwrite: TestObject) -> bool {
        let expected = TestObject {
            name: base.name.clone(),
            num: overwrite.num,
            maybe: base.maybe,
        };
        let base = serde_json::to_value(&base).expect("serialise value");
        let op = Operation::Set {
            path: "num".into(),
            value: Some(json!(overwrite.num)),
        };

        let expected = serde_json::to_value(&expected).expect("serialise value");
        Some(expected) == op.apply_to(base).ok()
    }

    #[quickcheck]
    fn apply_set_path_delete(base: TestObject) -> bool {
        let expected = json!({ "name": base.name.clone(), "maybe": base.maybe});
        let base = serde_json::to_value(&base).expect("serialise value");
        let op = Operation::Set {
            path: "num".to_string(),
            value: None,
        };

        Some(expected) == op.apply_to(base).ok()
    }

    #[quickcheck]
    fn apply_set_path_insert_object(object: TestObject) -> bool {
        let expected = json!({ "new": object.clone()});
        let object = serde_json::to_value(&object).expect("serialise value");
        let op = Operation::Set {
            path: "new".to_string(),
            value: Some(object),
        };

        Some(expected) == op.apply_to(json!({})).ok()
    }

    // splice

    #[test]
    fn apply_splice_op_number() {
        let op = Operation::Splice {
            path: "x".to_string(),
            index: 1,
            remove: 0,
            insert: json!([42, 43]),
        };

        let val = json!({ "x": [1,2,3,4], "z": "z"});
        let res = op.apply_to(val);
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
        let res = op.apply_to(val);
        match res {
            Ok(_) => panic!(),
            Err(e) => match e {
                OtError::Type(_) => (),
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
        let exp = json!({ "x": ["a", "42", "43", "b", "c", "d"], "z": "z"});
        assert_eq!(Some(exp), op.apply_to(val).ok())
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
        let exp = json!({ "x": [1, 42, 43, 4], "z": "z"});
        assert_eq!(Some(exp), op.apply_to(val).ok())
    }
}
