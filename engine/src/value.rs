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
