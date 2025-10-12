use crate::Path;
use serde_json::Value;

/// Check if path is reachable starting from value
pub(crate) fn is_reachable(path: impl Into<Path>, value: &Value) -> bool {
    let path = path.into();
    if path.is_empty() {
        return true;
    }

    let mut content = value;

    let paths: Vec<&str> = path.split('.').collect();
    for p in paths {
        content = match content {
            Value::Object(o) => match o.get(p) {
                Some(v) => v,
                None => return false,
            },
            Value::Array(a) => match a.iter().find(|element| match element {
                // only can reach objects in list and objects need matching "id"s
                Value::Object(o) => Some(&Value::String(p.to_string())) == o.get("id"),
                // other types in lists are not reachable (primitive types)
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn is_reachable_empty_path() {
        let value = json!(null);
        assert!(is_reachable(Path::from(""), &value));
    }

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
        let value = json!({"id": "foo", "bar": "baz", "xx": {"yy": "zz"}});
        // reachable key
        assert!(is_reachable(Path::from("bar"), &value));
        // reachable nested
        assert!(is_reachable(Path::from("xx.yy"), &value));
        // non-existing keys
        assert!(!is_reachable(Path::from("foo"), &value));
        assert!(!is_reachable(Path::from("abc"), &value));
    }

    #[test]
    fn is_reachable_for_array() {
        let value = json!([]);
        assert!(!is_reachable(Path::from("foo.bar"), &value));

        let value = json!([{}]);
        assert!(!is_reachable(Path::from("foo.bar"), &value));

        // only reachable objects with id in path
        let value = json!([{"id": "some_id", "bar": "baz"}]);
        assert!(is_reachable(Path::from("some_id.bar"), &value));
        // but not if we dont access the id
        let value = json!([{"id": "some_id", "bar": "baz"}]);
        assert!(!is_reachable(Path::from("bar.baz"), &value));

        let value = json!(["a", "b", "c"]);
        assert!(!is_reachable(Path::from("c"), &value));
    }
}
