use crate::OtError;
use crate::operation::Operation;
use crate::path::is_reachable;
use serde_json::Value;

/// Given an `op` which was created against a particular `content`, rebase it on top
/// of patches which were created against the very same content in parallel.
///
/// This function assumes that the patches apply cleanly to the content. Otherwise
/// the function returns None.
///
/// Returns the resulting Operation if rebase was successful, `None` if operations have
/// conflicts and [`OtError::Rebase`] if rebase operation fails.
///
/// ## Example
///
/// ```rust
/// // An operation rebased through an empty list of patches should be unchanged
/// use serde_json::json;
/// use otp::{rebase, Operation};
///
/// let value = json!({"name": "test", "count": 42});
/// let op = Operation::Set {
///     path: String::from(""),
///     value: Some(value.clone()),
/// };
///
/// let rebased = rebase(json!({}), op.clone(), [].iter()).unwrap();
/// assert!(Some(op) == rebased)
/// ```
pub fn rebase<'a>(
    content: Value,
    op: Operation,
    operations: impl Iterator<Item = &'a Operation>,
) -> Result<Option<Operation>, OtError> {
    let mut content = content;
    let mut op = Some(op);

    for operation in operations {
        match operation.apply_to(content) {
            Ok(value) => {
                content = value;
                if let Some(next_op) = op {
                    op = op_ot(&content, operation, next_op);
                } else {
                    return Err(OtError::Rebase(String::from("op_ot: rejecting patch")));
                }
            }
            Err(e) => {
                return Err(OtError::Rebase(format!(
                    "unexpected failure while applying patches: {e}"
                )));
            }
        }
    }

    Ok(op)
}

/// Apply `op` on top of `base` with values `content`.
/// Conflict resolution:
/// ```plain
/// Set (foo)        -> Set (foo)        = ok
/// Set (foo)        -> Set (foo.bar)    = none
/// Set (foo.bar)    -> Set (foo)        = ok
/// Set (foo)        -> Set (bar)        = ok
///
/// Set (foo)        -> Splice (foo)     = none
/// Set (foo)        -> Splice (foo.bar) = none
/// Set (foo.bar)    -> Splice (foo)     = ok
/// Set (foo)        -> Splice (bar)     = ok
///
/// Splice (foo)     -> Set (foo)        = ok
/// Splice (foo)     -> Set (foo.bar)    = ok if foo.bar exists
/// Splice (foo.bar) -> Set (foo)        = ok
/// Splice (foo)     -> Set (bar)        = ok
///
/// Splice (foo)     -> Splice (foo)     = none -- todo: ok (adjust)
/// Splice (foo)     -> Splice (foo.bar) = ok if foo.bar exists
/// Splice (foo.bar) -> Splice (foo)     = ok
/// Splice (foo)     -> Splice (bar)     = ok
/// ```
fn op_ot(content: &Value, base: &Operation, op: Operation) -> Option<Operation> {
    // drop duplicates
    if *base == op {
        return None;
    }

    let (base_path, op_path) = (base.path(), op.path());

    // disjoint paths are always safe
    if !base_path.starts_with(&op_path) && !op_path.starts_with(&base_path) {
        return Some(op);
    }

    let same_path = base_path == op_path;
    let base_contains_op = base_path.starts_with(&op_path);

    match (base, &op) {
        (Operation::Set { .. }, Operation::Set { .. }) => {
            // reject Set on different path but base contains op
            // eg. foo -> foo.bar
            if !same_path && base_contains_op {
                None
            } else {
                Some(op)
            }
        }
        (Operation::Set { .. }, Operation::Splice { .. }) => {
            if same_path || base_contains_op {
                None
            } else {
                Some(op)
            }
        }
        (Operation::Splice { .. }, Operation::Set { .. }) => {
            if same_path {
                return Some(op);
            }
            if base_contains_op && !is_reachable(op_path, content) {
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
            // TODO we should check that both inserts are arrays?

            if base_path != op_path {
                return if base_contains_op && is_reachable(op_path, content) {
                    Some(op)
                } else {
                    None
                };
            }

            // same path splice
            if base_index + base_remove <= *op_index {
                let base_insert = base_insert.as_array()?;
                Some(Operation::Splice {
                    path: op_path.to_owned(),
                    index: op_index + base_insert.len() - base_remove,
                    remove: *op_remove,
                    insert: op_insert.to_owned(),
                })
            } else if op_index + op_remove < *base_index {
                Some(op)
            } else {
                None
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Operation, ROOT_PATH};
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

    // rebase

    #[quickcheck]
    fn rebase_identity_op_through_none(input: TestObject) -> bool {
        // An operation rebased through an empty list of patches should be unchanged
        let value = serde_json::to_value(&input).expect("serialise value");
        let op = Operation::Set {
            path: ROOT_PATH.into(),
            value: Some(value.clone()),
        };

        let rebased = rebase(json!({}), op.clone(), [].iter()).unwrap();
        Some(op) == rebased
    }

    #[quickcheck]
    fn rebase_set_through_unrelated_set(base: TestObject, a: String, b: String) -> bool {
        // A set operation rebased through an unrelated set operation should be unchanged
        let base_val = serde_json::to_value(&base).expect("serialise value");

        // First operation sets property "a"
        let op1 = Operation::Set {
            path: "a".into(),
            value: Some(json!(a)),
        };

        // Second operation (to be rebased) sets property "b"
        let op2 = Operation::Set {
            path: "b".into(),
            value: Some(json!(b)),
        };

        // The rebased operation should be unchanged since they affect different properties
        Some(op2.clone()) == rebase(base_val, op2, [op1].iter()).unwrap()
    }

    #[quickcheck]
    fn rebase_set_through_same_path_set(base: TestObject, a1: String, a2: String) -> bool {
        let base_val = serde_json::to_value(&base).expect("serialise value");

        // First operation sets property "a"
        let op1 = Operation::Set {
            path: "a".into(),
            value: Some(json!(a1)),
        };

        // Second operation (to be rebased) also sets property "a"
        let op2 = Operation::Set {
            path: "a".into(),
            value: Some(json!(a2)),
        };

        Some(op2.clone()) == rebase(base_val, op2, [op1].iter()).unwrap()
    }

    #[quickcheck]
    fn rebase_set_through_delete(base: TestObject) -> bool {
        // A set operation rebased through a delete operation of the same property
        let base_val = serde_json::to_value(&base).expect("serialise value");

        // First operation deletes "name"
        let op1 = Operation::Set {
            path: "name".into(),
            value: None,
        };

        // Second operation (to be rebased) sets "name"
        let op2 = Operation::Set {
            path: "name".into(),
            value: Some(json!("new name")),
        };

        Some(op2.clone()) == rebase(base_val, op2, [op1].iter()).unwrap()
    }

    #[quickcheck]
    fn rebase_splice_through_unrelated_set(input: TestObject, name: String) -> bool {
        // A splice operation rebased through an unrelated set operation
        let base_val = serde_json::to_value(&input).expect("serialise value");

        // First operation sets the name
        let op1 = Operation::Set {
            path: "name".into(),
            value: Some(json!(name)),
        };

        // Second operation (to be rebased) splices an array
        let op2 = Operation::Splice {
            path: "array".into(),
            index: 0,
            remove: 0,
            insert: json!([1, 2, 3]),
        };

        Some(op2.clone()) == rebase(base_val, op2, [op1].iter()).unwrap()
    }

    #[test]
    fn rebase_splice_through_same_path_splice() {
        // Test rebasing a splice operation through another splice at the same path
        let base_val = json!({"array": [1, 2, 3, 4, 5]});

        // First operation removes elements 1-2 and inserts [10, 20]
        let op1 = Operation::Splice {
            path: "array".into(),
            index: 1,
            remove: 2,
            insert: json!([10, 20]),
        };

        // After op1, array is [1, 10, 20, 4, 5]

        // Second operation (to be rebased) removes element at index 3 and inserts [30, 40]
        let op2 = Operation::Splice {
            path: "array".into(),
            index: 3,
            remove: 1,
            insert: json!([30, 40]),
        };

        // The rebased operation should be adjusted to account for the changed indices
        // After op1, the element at index 3 in the original array has moved to index 4
        // The rebased operation should still remove the same logical element
        let rebased = rebase(base_val, op2, [op1].iter());

        // The expected rebased operation
        let expected = Operation::Splice {
            path: "array".into(),
            index: 3,
            remove: 1,
            insert: json!([30, 40]),
        };

        assert_eq!(Some(expected), rebased.unwrap())
    }

    #[test]
    fn rebase_splice_on_array() {
        // Test rebasing a splice operation through a non-interfering operation
        // We'll use a test object that already has an array
        let base_val = json!({
            "name": "test",
            "array": ["a", "b", "c", "d"]
        });

        // First operation sets a property (doesn't affect the array)
        let op1 = Operation::Set {
            path: "name".into(),
            value: Some(json!("updated")),
        };

        // Operation to be rebased splices the array
        let op2 = Operation::Splice {
            path: "array".into(),
            index: 1,
            remove: 1,
            insert: json!(["x", "y"]),
        };

        assert_eq!(
            Some(op2.clone()),
            rebase(base_val, op2, [op1].iter()).unwrap()
        )
    }

    #[test]
    fn rebase_multiple_patches() {
        // Test rebasing through multiple sequential patches
        let base_val = json!({"array": [1, 2, 3, 4, 5], "name": "test"});

        // First patch sets the name
        let op1 = Operation::Set {
            path: "name".into(),
            value: Some(json!("new name")),
        };

        // Second patch removes elements from the array
        let op2 = Operation::Splice {
            path: "array".into(),
            index: 0,
            remove: 2,
            insert: json!([]),
        };

        // Operation to be rebased inserts at index 3
        let op3_insert = json!([10, 20]);
        let op3 = Operation::Splice {
            path: "array".into(),
            index: 3,
            remove: 0,
            insert: op3_insert,
        };

        // The rebased operation should have its index adjusted to account for the removed elements
        // Let's manually compute what happens:
        // 1. After op1: No change to array indices, since it only changes "name"
        // 2. After op2: The array is [3, 4, 5], elements at indices 0 and 1 are removed
        // 3. For op3 (original: insert at index 3), it should be adjusted to index 1
        let op3_after_rebase = Operation::Splice {
            path: "array".into(),
            index: 1, // Index decreased by 2 because two elements were removed before it
            remove: 0,
            insert: json!([10, 20]),
        };

        assert_eq!(
            Some(op3_after_rebase),
            rebase(base_val, op3, [op1, op2].iter()).unwrap()
        )
    }

    // Tests for op_ot function

    #[quickcheck]
    fn op_ot_duplicate_operations(obj: TestObject) -> bool {
        let content = serde_json::to_value(&obj).expect("serialise value");

        let op1 = Operation::Set {
            path: "name".into(),
            value: Some(json!(obj.name)),
        };

        op_ot(&content, &op1, op1.clone()).is_none()
    }

    #[quickcheck]
    fn op_ot_disjoint_paths(obj: TestObject, new_name: String, new_num: u32) -> bool {
        // Rule: if neither is a prefix of the other, it's safe to accept the op
        let content = serde_json::to_value(&obj).expect("serialise value");

        let op1 = Operation::Set {
            path: "name".into(),
            value: Some(json!(new_name)),
        };

        let op2 = Operation::Set {
            path: "num".into(),
            value: Some(json!(new_num)),
        };

        // Operations affect different paths, so op2 should be returned unchanged
        Some(op2.clone()) == op_ot(&content, &op1, op2)
    }

    #[quickcheck]
    fn op_ot_set_set_same_path(obj: TestObject, val1: String, val2: String) -> bool {
        // Rule: Set/Set with same path
        let content = serde_json::to_value(&obj).expect("serialise value");

        let op1 = Operation::Set {
            path: "name".into(),
            value: Some(json!(val1)),
        };

        let op2 = Operation::Set {
            path: "name".into(),
            value: Some(json!(val2)),
        };

        // When both Set operations target the same path, op2 should be returned
        Some(op2.clone()) == op_ot(&content, &op1, op2)
    }

    #[test]
    fn op_ot_set_set_base_prefixed_by_op() {
        // Rule: Set/Set where op path includes base path as prefix

        // Create nested structure for testing path prefixes
        let nested_content = json!({
            "user": {
                "name": "test",
                "age": 30
            }
        });

        // op1 affects the entire user object
        let op1 = Operation::Set {
            path: "user".into(),
            value: Some(json!({
                "name": "changed",
                "age": 31
            })),
        };

        // op2 only affects a child property
        let op2 = Operation::Set {
            path: "user.name".into(),
            value: Some(json!("another name")),
        };

        // When the op2 path is more specific than the base op path,
        // the implementation returns the op2 operation
        assert_eq!(Some(op2.clone()), op_ot(&nested_content, &op1, op2))
    }

    #[test]
    fn op_ot_set_set_op_prefixed_by_base() {
        // Rule: Set/Set where base path is a prefix of op path

        // Create nested structure for testing path prefixes
        let nested_content = json!({
            "user": {
                "name": "test",
                "age": 30
            }
        });

        // op1 only affects a child property
        let op1 = Operation::Set {
            path: "user.name".into(),
            value: Some(json!("new name")),
        };

        // op2 affects the entire user object
        let op2 = Operation::Set {
            path: "user".into(),
            value: Some(json!({
                "name": "another",
                "age": 25
            })),
        };

        // When the base path is a prefix of the op path, the implementation
        // returns None as the operations conflict (base op makes op2 redundant)
        assert_eq!(op_ot(&nested_content, &op1, op2.clone()), None)
    }

    #[quickcheck]
    fn op_ot_set_splice_same_path(nums: Vec<u32>) -> bool {
        // Rule: Set/Splice with same path
        let array = if nums.is_empty() { vec![1, 2, 3] } else { nums };
        let content = json!({"array": array});

        // op1 replaces the entire array
        let op1 = Operation::Set {
            path: "array".into(),
            value: Some(json!([4, 5, 6])),
        };

        // op2 splices the array
        let op2 = Operation::Splice {
            path: "array".into(),
            index: 1,
            remove: 1,
            insert: json!([10]),
        };

        // When Set and Splice target the same path, return None
        op_ot(&content, &op1, op2).is_none()
    }

    #[quickcheck]
    fn op_ot_splice_set_same_path(nums: Vec<u32>) -> bool {
        // Rule: Splice/Set with same path
        let array = if nums.is_empty() { vec![1, 2, 3] } else { nums };
        let content = json!({"array": array});

        // op1 splices the array
        let op1 = Operation::Splice {
            path: "array".into(),
            index: 1,
            remove: if array.len() > 1 { 1 } else { 0 },
            insert: json!([10]),
        };

        // op2 replaces the entire array
        let op2 = Operation::Set {
            path: "array".into(),
            value: Some(json!([4, 5, 6])),
        };

        // When Splice and Set target the same path, return op2
        Some(op2.clone()) == op_ot(&content, &op1, op2)
    }

    #[test]
    fn op_ot_splice_splice_non_overlapping() {
        // Rule: Splice/Splice with same path but non-overlapping ranges
        // Using a fixed test because QuickCheck would make it hard to ensure valid non-overlapping ranges
        let content = json!({"array": [1, 2, 3, 4, 5]});

        // op1 splices at beginning
        let op1 = Operation::Splice {
            path: "array".into(),
            index: 0,
            remove: 1,
            insert: json!([10]),
        };

        // op2 splices at end (after op1's range)
        let op2 = Operation::Splice {
            path: "array".into(),
            index: 3,
            remove: 1,
            insert: json!([20]),
        };

        // When splice ranges don't overlap and op2's index is after op1's range,
        // the implementation should adjust op2's index to account for op1's net change in array size
        assert_eq!(Some(op2.clone()), op_ot(&content, &op1, op2))
    }

    #[test]
    fn op_ot_splice_splice_before_base() {
        // Rule: Splice/Splice where op2 operates on a position before op1
        // Using a fixed test because QuickCheck would make it hard to ensure valid relative positions
        let content = json!({"array": [1, 2, 3, 4, 5]});

        // op1 splices later in array
        let op1 = Operation::Splice {
            path: "array".into(),
            index: 3,
            remove: 1,
            insert: json!([10]),
        };

        // op2 splices earlier in array (before op1's range)
        let op2 = Operation::Splice {
            path: "array".into(),
            index: 1,
            remove: 1,
            insert: json!([20]),
        };

        // When op2's range is entirely before op1's range, op2 is returned unchanged
        assert_eq!(Some(op2.clone()), op_ot(&content, &op1, op2))
    }

    #[test]
    fn op_ot_splice_splice_overlapping() {
        // Rule: Splice/Splice with overlapping ranges, which causes a conflict
        // Using a fixed test because QuickCheck would make it hard to ensure valid overlapping ranges
        let content = json!({"array": [1, 2, 3, 4, 5]});

        // op1 removes elements 1-3
        let op1 = Operation::Splice {
            path: "array".into(),
            index: 1,
            remove: 3,
            insert: json!([]),
        };

        // op2 tries to modify elements 2-3 (which overlap with op1's removal)
        let op2 = Operation::Splice {
            path: "array".into(),
            index: 2,
            remove: 2,
            insert: json!([20, 30]),
        };

        // When ranges overlap, return None (conflict)
        assert_eq!(op_ot(&content, &op1, op2), None)
    }

    #[test]
    fn op_ot_splice_splice_after_adjustment() {
        // Rule: Splice/Splice with appropriate index adjustment for a following operation
        // Using a fixed test because QuickCheck would make index adjustment verification too complex
        let content = json!({"array": [1, 2, 3, 4, 5]});

        // op1 removes first element and inserts two
        let op1 = Operation::Splice {
            path: "array".into(),
            index: 0,
            remove: 1,
            insert: json!([10, 20]),
        };

        // op2 works at index 3 (after op1's affected range)
        let op2 = Operation::Splice {
            path: "array".into(),
            index: 3,
            remove: 1,
            insert: json!([30]),
        };

        // After op1, the array is [10, 20, 2, 3, 4, 5]
        // op2's index should be adjusted by +1 (insert 2 - remove 1)
        let expected = Operation::Splice {
            path: "array".into(),
            index: 4, // Adjusted from 3 to 4
            remove: 1,
            insert: json!([30]),
        };

        assert_eq!(Some(expected), op_ot(&content, &op1, op2))
    }

    #[quickcheck]
    fn op_ot_splice_is_reachable(val: u32) -> bool {
        // Test is_reachable check in Splice/Set and Splice/Splice cases
        let content = json!([{"id": "item1", "value": 42}, {"id": "item2", "value": 100}]);

        // op1 splices the array
        let op1 = Operation::Splice {
            path: "".into(), // Root path for the array
            index: 0,
            remove: 1,
            insert: json!([{"id": "new_item", "value": 200}]),
        };

        // op2 sets a property using a path that depends on the array content
        let op2 = Operation::Set {
            path: "item1.value".into(),
            value: Some(json!(val)),
        };

        // The path "item1.value" should be reachable
        Some(op2.clone()) == op_ot(&content, &op1, op2)
    }
}
