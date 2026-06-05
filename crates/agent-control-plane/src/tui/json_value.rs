use serde_json::Value;

pub(super) fn value_array<'a>(value: &'a Value, key: &str) -> &'a Vec<Value> {
    value
        .get(key)
        .and_then(Value::as_array)
        .unwrap_or(&EMPTY_VALUES)
}

pub(super) fn value_array_path<'a>(value: &'a Value, path: &[&str]) -> &'a Vec<Value> {
    let mut current = value;
    for key in path {
        let Some(next) = current.get(*key) else {
            return &EMPTY_VALUES;
        };
        current = next;
    }
    current.as_array().unwrap_or(&EMPTY_VALUES)
}

static EMPTY_VALUES: Vec<Value> = Vec::new();

pub(super) fn json_path(value: &Value, path: &[&str]) -> String {
    let mut current = value;
    for key in path {
        let Some(next) = current.get(*key) else {
            return "-".to_owned();
        };
        current = next;
    }
    scalar_text(current)
}

pub(super) fn json_array_len(value: &Value, key: &str) -> usize {
    value.get(key).and_then(Value::as_array).map_or(0, Vec::len)
}

pub(super) fn scalar_text(value: &Value) -> String {
    match value {
        Value::String(value) => value.clone(),
        Value::Bool(value) => value.to_string(),
        Value::Number(value) => value.to_string(),
        Value::Null => "-".to_owned(),
        value => value.to_string(),
    }
}

pub(super) fn archived_label(thread: &Value) -> &'static str {
    if thread
        .get("archived")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        "archived"
    } else {
        "active"
    }
}

pub(super) fn string_field(value: &Value, key: &str) -> Option<String> {
    value.get(key).and_then(Value::as_str).map(str::to_owned)
}
