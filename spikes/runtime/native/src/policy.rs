use serde_json::Value;
pub const WIRE_BYTES: usize = 32768;
pub const COMMAND_BYTES: usize = 65536;
pub const QUEUED: usize = 32;
pub const RESOURCES: usize = 16;

pub fn request(raw: &str) -> Result<Value, String> {
    if raw.len() > WIRE_BYTES {
        return Err("wire size".into());
    }
    let value: Value = serde_json::from_str(raw).map_err(|_| "invalid JSON")?;
    fn visit(value: &Value, depth: usize, count: &mut usize) -> Result<(), String> {
        *count += 1;
        if depth > 16 || *count > 2048 {
            return Err("JSON complexity".into());
        }
        match value {
            Value::Array(items) => {
                for item in items {
                    visit(item, depth + 1, count)?;
                }
            }
            Value::Object(items) => {
                for item in items.values() {
                    visit(item, depth + 1, count)?;
                }
            }
            _ => (),
        }
        Ok(())
    }
    visit(&value, 0, &mut 0)?;
    let object = value.as_object().ok_or("request envelope")?;
    if object.len() != 3
        || !["id", "op", "value"]
            .iter()
            .all(|k| object.contains_key(*k))
    {
        return Err("request envelope".into());
    }
    let id = value["id"].as_u64().ok_or("request ID")?;
    if id == 0 || id > 9_007_199_254_740_991 {
        return Err("request ID".into());
    }
    let arg = &value["value"];
    match value["op"].as_str().ok_or("operation")? {
        "echo" | "state.commit" | "state.result" => (),
        "state.read" if arg.is_null() => (),
        "read" | "subscribe" if arg == "value" => (),
        "write" | "publish" if arg.as_f64().is_some_and(f64::is_finite) => (),
        "unsubscribe"
            if arg.as_str().is_some_and(|s| {
                s.strip_prefix('s').is_some_and(|n| {
                    !n.starts_with('0') && !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit())
                })
            }) => {}
        "delay" if arg.is_null() || arg == "long" => (),
        "fail" | "stall" | "late" if arg.is_null() => (),
        _ => return Err("capability/argument".into()),
    }
    Ok(value)
}
