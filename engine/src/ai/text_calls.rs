//! Some model/runtime combinations emit tool calls as text inside the answer instead of
//! structured `tool_calls` (for example Qwen's `<tool_call><function=…>` form when the
//! server's parser misses it). Recognized calls are converted to the structured shape and
//! then validated like any other call; markup that cannot be understood is an error and is
//! never delivered as an answer.
use serde_json::{json, Value};

const OPEN: &str = "<tool_call>";
const CLOSE: &str = "</tool_call>";

/// Whether the text contains tool-call markup at all.
pub(crate) fn contains_markup(text: &str) -> bool {
    text.contains(OPEN) || text.contains("<function=")
}

/// Splits `text` into the prose before the first call and structured OpenAI-style calls.
/// `None` when there is no markup; `Err` when markup is present but not understood.
pub(crate) fn extract(text: &str, turn: u32) -> Option<Result<(String, Vec<Value>), String>> {
    if !contains_markup(text) {
        return None;
    }
    Some(parse(text, turn))
}

fn parse(text: &str, turn: u32) -> Result<(String, Vec<Value>), String> {
    let first = text
        .find(OPEN)
        .or_else(|| text.find("<function="))
        .unwrap_or(text.len());
    let prose = text[..first].trim().to_string();
    let mut calls = vec![];
    let mut rest = &text[first..];
    while !rest.trim().is_empty() {
        let body_start = rest.strip_prefix(OPEN).map_or(0, |_| OPEN.len());
        let body = &rest[body_start..];
        let (inner, next) = match body.find(CLOSE) {
            Some(end) => (&body[..end], &body[end + CLOSE.len()..]),
            None => (body, ""),
        };
        let (name, arguments) = call(inner.trim())?;
        calls.push(
            json!({"id":format!("text-{turn}-{}", calls.len()),"type":"function",
            "function":{"name":name,"arguments":arguments.to_string()}}),
        );
        rest = next.trim_start();
        if !rest.is_empty() && !rest.starts_with(OPEN) && !rest.starts_with("<function=") {
            return Err("text after tool call markup".into());
        }
        if calls.len() > 8 {
            return Err("too many text tool calls".into());
        }
    }
    if calls.is_empty() {
        return Err("empty tool call markup".into());
    }
    Ok((prose, calls))
}

fn call(inner: &str) -> Result<(String, Value), String> {
    // Hermes/JSON form: {"name": "...", "arguments": {...} | "..."}
    if inner.starts_with('{') {
        let v: Value = serde_json::from_str(inner).map_err(|_| "invalid JSON tool call")?;
        let name = v["name"].as_str().ok_or("tool call name")?.to_string();
        let args = match &v["arguments"] {
            Value::String(s) => {
                serde_json::from_str(s).map_err(|_| "invalid tool call arguments")?
            }
            Value::Null => json!({}),
            other => other.clone(),
        };
        return Ok((name, args));
    }
    // Qwen XML form: <function=NAME><parameter=KEY>VALUE</parameter>…</function>
    let body = inner
        .strip_prefix("<function=")
        .ok_or("unrecognized tool call markup")?;
    let (name, mut params) = body.split_once('>').ok_or("tool call name")?;
    let name = name.trim();
    if name.is_empty()
        || !name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    {
        return Err("tool call name".into());
    }
    let mut args = serde_json::Map::new();
    loop {
        params = params.trim_start();
        if params.is_empty() || params.starts_with("</function>") {
            break;
        }
        let p = params
            .strip_prefix("<parameter=")
            .ok_or("tool call parameter")?;
        let (key, after) = p.split_once('>').ok_or("tool call parameter")?;
        let (raw, after) = after
            .split_once("</parameter>")
            .ok_or("unterminated tool call parameter")?;
        let raw = raw.trim();
        // Structured values are JSON; everything else (IDs, kinds, queries) stays a string.
        let value = if raw.starts_with('{') || raw.starts_with('[') {
            serde_json::from_str(raw).map_err(|_| "invalid tool call parameter JSON")?
        } else {
            json!(raw)
        };
        args.insert(key.trim().to_string(), value);
        params = after;
    }
    Ok((name.to_string(), Value::Object(args)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn qwen_xml_calls_become_structured_calls() {
        let text = "I will read the history.\n\n<tool_call>\n<function=monitoring_read>\n<parameter=kind>\nhistory\n</parameter>\n<parameter=id>\nhost-homelab\n</parameter>\n</function>\n</tool_call>";
        let (prose, calls) = extract(text, 2).unwrap().unwrap();
        assert_eq!(prose, "I will read the history.");
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0]["function"]["name"], "monitoring_read");
        let args: Value =
            serde_json::from_str(calls[0]["function"]["arguments"].as_str().unwrap()).unwrap();
        assert_eq!(args, json!({"kind":"history","id":"host-homelab"}));
        assert_eq!(calls[0]["id"], "text-2-0");
    }

    #[test]
    fn json_calls_nested_objects_and_several_calls_parse() {
        let text = "<tool_call>{\"name\":\"monitoring_snapshot\",\"arguments\":{}}</tool_call>\n<tool_call>\n<function=monitoring_probe>\n<parameter=source>homelab-prometheus</parameter>\n<parameter=request>{\"kind\":\"query\",\"query\":\"up\"}</parameter>\n</function>\n</tool_call>";
        let (prose, calls) = extract(text, 1).unwrap().unwrap();
        assert!(prose.is_empty());
        assert_eq!(calls.len(), 2);
        let args: Value =
            serde_json::from_str(calls[1]["function"]["arguments"].as_str().unwrap()).unwrap();
        assert_eq!(args["request"]["kind"], "query");
    }

    #[test]
    fn plain_answers_pass_and_broken_markup_is_rejected() {
        assert!(extract("Disk / is 41% free.", 1).is_none());
        assert!(
            extract("<tool_call>\n<function=x>\n<parameter=id>broken", 1)
                .unwrap()
                .is_err()
        );
        assert!(extract("<tool_call>{not json}</tool_call>", 1)
            .unwrap()
            .is_err());
        assert!(extract(
            "<tool_call><function=monitoring_read></function></tool_call> and then prose",
            1
        )
        .unwrap()
        .is_err());
    }
}
