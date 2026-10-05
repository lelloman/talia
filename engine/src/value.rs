use serde_json::{json, Value};
pub const MAX_BYTES: usize = 131072;
/// Wire trees are always tagged, including user objects: tag-shaped user data cannot collide.
pub fn validate(wire: &Value) -> Result<(), String> {
    if serde_json::to_vec(wire).map_err(|e| e.to_string())?.len() > MAX_BYTES {
        return Err("value size limit".into());
    }
    let obj = wire.as_object().ok_or("value envelope")?;
    if obj.len() != 2 || wire["version"] != 1 {
        return Err("value version/envelope".into());
    }
    node(&wire["value"], 0, &mut 0)
}
fn node(n: &Value, depth: usize, count: &mut usize) -> Result<(), String> {
    *count += 1;
    if depth > 48 || *count > 16000 {
        return Err("value complexity".into());
    }
    let a = n.as_array().ok_or("value shape")?;
    let tag = a.first().and_then(Value::as_str).ok_or("value tag")?;
    if ["undefined", "null"].contains(&tag) && a.len() == 1 {
        return Ok(());
    }
    if a.len() != 2 {
        return Err("value arity".into());
    }
    let v = &a[1];
    match tag {
        "boolean" if v.is_boolean() => Ok(()),
        "string" if v.is_string() => Ok(()),
        "number"
            if v.as_f64()
                .is_some_and(|n| n.is_finite() && !(n == 0.0 && n.is_sign_negative()))
                || v.as_str()
                    .is_some_and(|s| ["NaN", "Infinity", "-Infinity", "-0"].contains(&s)) =>
        {
            Ok(())
        }
        "array" => {
            for x in v.as_array().ok_or("array")? {
                node(x, depth + 1, count)?;
            }
            Ok(())
        }
        "object" => {
            let mut keys = std::collections::HashSet::new();
            for p in v.as_array().ok_or("object")? {
                let p = p.as_array().filter(|p| p.len() == 2).ok_or("entry")?;
                let k = p[0].as_str().ok_or("key")?;
                if !keys.insert(k) {
                    return Err("duplicate key".into());
                }
                node(&p[1], depth + 1, count)?;
            }
            Ok(())
        }
        _ => Err("unknown/invalid value tag".into()),
    }
}
pub fn undefined() -> Value {
    json!({"version":1,"value":["undefined"]})
}
pub fn number(n: f64) -> Value {
    json!({"version":1,"value":["number",if n.is_nan(){json!("NaN")}else if n==f64::INFINITY{json!("Infinity")}else if n==f64::NEG_INFINITY{json!("-Infinity")}else if n==0.0&&n.is_sign_negative(){json!("-0")}else{json!(n)}]})
}
/// Convert ordinary JSON into the lossless envelope; special numeric samples use `number`.
pub fn from_json(v: &Value) -> Value {
    fn convert(v: &Value) -> Value {
        match v {
            Value::Null => json!(["null"]),
            Value::Bool(x) => json!(["boolean", x]),
            Value::Number(x) => number(x.as_f64().unwrap())["value"].clone(),
            Value::String(x) => json!(["string", x]),
            Value::Array(xs) => json!(["array", xs.iter().map(convert).collect::<Vec<_>>()]),
            Value::Object(xs) => json!([
                "object",
                xs.iter()
                    .map(|(k, v)| json!([k, convert(v)]))
                    .collect::<Vec<_>>()
            ]),
        }
    }
    json!({"version":1,"value":convert(v)})
}
pub fn matches_schema(v: &Value, schema: &str) -> bool {
    validate(v).is_ok() && (schema == "any" || v["value"][0].as_str() == Some(schema))
}
pub fn validate_schema(s: &str) -> Result<(), String> {
    if [
        "any",
        "undefined",
        "null",
        "number",
        "string",
        "boolean",
        "array",
        "object",
    ]
    .contains(&s)
    {
        Ok(())
    } else {
        Err("unknown value schema".into())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn boundaries() {
        for v in [
            undefined(),
            number(f64::NAN),
            number(f64::INFINITY),
            number(-0.0),
        ] {
            validate(&v).unwrap();
        }
        for v in [
            json!({"version":2,"value":["null"]}),
            json!({"version":1,"value":["object",[["x",["null"]],["x",["null"]]]]}),
            json!({"version":1,"value":["number","oops"]}),
        ] {
            assert!(validate(&v).is_err());
        }
    }
    #[test]
    fn embedded_round_trip() {
        let rt = rquickjs::Runtime::new().unwrap();
        let ctx = rquickjs::Context::full(&rt).unwrap();
        ctx.with(|c|{c.eval::<(),_>(include_str!("../shared/value.js")).unwrap();let s:String=c.eval("TaliaValue.stringify({a:undefined,b:NaN,c:Infinity,d:-Infinity,e:null,f:-0,user:{version:1,value:['undefined']}})").unwrap();let v:Value=serde_json::from_str(&s).unwrap();validate(&v).unwrap();let ok:bool=c.eval(format!("(()=>{{let v=TaliaValue.parse({});return 'a' in v&&v.a===undefined&&Number.isNaN(v.b)&&v.c===Infinity&&Object.is(v.f,-0)&&v.user.version===1}})()",serde_json::to_string(&s).unwrap())).unwrap();assert!(ok);});
    }
}

/// Bounded, readable JSON preview of a wire value for AI tools. Long numeric series
/// become statistics (`series`, `samples`, `min`, `max`, `mean`, `last`), other long
/// lists keep their first items, and long strings are clipped. Not a lossless decode:
/// callers read the full value separately when exact data matters.
pub fn preview(wire: &Value) -> Value {
    const SERIES: usize = 16;
    fn number(v: &Value) -> Option<f64> {
        let a = v.as_array()?;
        (a.first()?.as_str()? == "number").then(|| a.get(1)?.as_f64()).flatten()
    }
    fn round(n: f64) -> Value {
        json!((n * 1000.0).round() / 1000.0)
    }
    fn walk(n: &Value, depth: usize) -> Value {
        let Some(a) = n.as_array() else { return Value::Null };
        let tag = a.first().and_then(Value::as_str).unwrap_or("");
        let v = a.get(1).unwrap_or(&Value::Null);
        if depth > 8 {
            return json!("…");
        }
        match tag {
            "undefined" | "null" => Value::Null,
            "boolean" => v.clone(),
            "number" => v.clone(),
            "string" => {
                let s = v.as_str().unwrap_or("");
                if s.chars().count() > 300 {
                    json!(format!("{}…", s.chars().take(300).collect::<String>()))
                } else {
                    json!(s)
                }
            }
            "array" => {
                let items = v.as_array().map(Vec::as_slice).unwrap_or(&[]);
                let numeric = items.iter().all(|x| number(x).is_some() || x.get(0).and_then(Value::as_str).is_some_and(|t| t == "null" || t == "undefined"));
                if items.len() > SERIES && numeric {
                    let values: Vec<f64> = items.iter().filter_map(number).filter(|n| n.is_finite()).collect();
                    let mut out = json!({"series": items.len(), "samples": values.len()});
                    if !values.is_empty() {
                        out["min"] = round(values.iter().cloned().fold(f64::INFINITY, f64::min));
                        out["max"] = round(values.iter().cloned().fold(f64::NEG_INFINITY, f64::max));
                        out["mean"] = round(values.iter().sum::<f64>() / values.len() as f64);
                        out["last"] = round(*values.last().unwrap());
                    }
                    out
                } else if items.len() > SERIES {
                    json!({"items": items.len(), "first": items.iter().take(8).map(|x| walk(x, depth + 1)).collect::<Vec<_>>()})
                } else {
                    Value::Array(items.iter().map(|x| walk(x, depth + 1)).collect())
                }
            }
            "object" => {
                let mut out = serde_json::Map::new();
                for p in v.as_array().map(Vec::as_slice).unwrap_or(&[]) {
                    if let (Some(k), Some(x)) = (p.get(0).and_then(Value::as_str), p.get(1)) {
                        out.insert(k.to_string(), walk(x, depth + 1));
                    }
                }
                Value::Object(out)
            }
            _ => Value::Null,
        }
    }
    walk(&wire["value"], 0)
}
