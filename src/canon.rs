use serde_json::{Map, Value};

pub fn canonical_json_string(value: &Value) -> String {
    fn stable(v: &Value) -> Value {
        match v {
            Value::Object(m) => {
                let mut keys: Vec<_> = m.keys().cloned().collect();
                keys.sort();
                let mut out = Map::new();
                for k in keys {
                    out.insert(k.clone(), stable(&m[&k]));
                }
                Value::Object(out)
            }
            Value::Array(arr) => Value::Array(arr.iter().map(stable).collect()),
            _ => v.clone(),
        }
    }
    let s = stable(value);
    serde_json::to_string(&s).unwrap()
}