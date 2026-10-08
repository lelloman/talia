//! Render CommonMark to Telegram text/entities, then split at UTF-16 boundaries.
//! No model-authored HTML or Telegram parse-mode syntax reaches the API.
use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};
use serde_json::{json, Value};

pub(crate) struct Part {
    pub text: String,
    pub entities: Vec<Value>,
}

#[derive(Clone)]
struct Span {
    start: usize,
    end: usize,
    kind: &'static str,
    url: Option<String>,
}

#[derive(Default)]
struct Render {
    text: String,
    units: usize,
    spans: Vec<Span>,
}
impl Render {
    fn push(&mut self, text: &str) {
        self.text.push_str(text);
        self.units += text.encode_utf16().count();
    }
    fn newline(&mut self) {
        if !self.text.is_empty() && !self.text.ends_with('\n') {
            self.push("\n");
        }
    }
}

pub(crate) fn markdown(input: &str, limit: usize) -> Vec<Part> {
    assert!(limit >= 2);
    let mut r = Render::default();
    let mut stack = Vec::new();
    let mut lists: Vec<Option<u64>> = Vec::new();
    for event in Parser::new_ext(input, Options::ENABLE_STRIKETHROUGH) {
        match event {
            Event::Start(tag) => {
                match &tag {
                    Tag::Heading { .. } | Tag::CodeBlock(_) => r.newline(),
                    Tag::List(start) => {
                        r.newline();
                        lists.push(*start);
                    }
                    Tag::Item => {
                        r.newline();
                        r.push(&"  ".repeat(lists.len().saturating_sub(1)));
                        if let Some(Some(n)) = lists.last_mut() {
                            r.push(&format!("{n}. "));
                            *n = n.saturating_add(1);
                        } else {
                            r.push("• ");
                        }
                    }
                    Tag::BlockQuote(_) => {
                        r.newline();
                        r.push("> ");
                    }
                    _ => {}
                }
                let (kind, url) = match &tag {
                    Tag::Strong | Tag::Heading { .. } => (Some("bold"), None),
                    Tag::Emphasis => (Some("italic"), None),
                    Tag::Strikethrough => (Some("strikethrough"), None),
                    Tag::CodeBlock(_) => (Some("pre"), None),
                    Tag::Link { dest_url, .. } if safe_link(dest_url) => {
                        (Some("text_link"), Some(dest_url.to_string()))
                    }
                    _ => (None, None),
                };
                stack.push((r.units, kind, url));
            }
            Event::End(tag) => {
                if let Some((start, Some(kind), url)) = stack.pop() {
                    r.spans.push(Span {
                        start,
                        end: r.units,
                        kind,
                        url,
                    });
                }
                match tag {
                    TagEnd::Paragraph => {
                        r.newline();
                        if lists.is_empty() {
                            r.push("\n");
                        }
                    }
                    TagEnd::Heading(_)
                    | TagEnd::CodeBlock
                    | TagEnd::Item
                    | TagEnd::BlockQuote(_) => r.newline(),
                    TagEnd::List(_) => {
                        lists.pop();
                        r.newline();
                    }
                    _ => {}
                }
            }
            Event::Text(text) | Event::Html(text) | Event::InlineHtml(text) => r.push(&text),
            Event::Code(text) => {
                let start = r.units;
                r.push(&text);
                r.spans.push(Span {
                    start,
                    end: r.units,
                    kind: "code",
                    url: None,
                });
            }
            Event::SoftBreak | Event::HardBreak => r.push("\n"),
            Event::Rule => {
                r.newline();
                r.push("────────\n");
            }
            _ => {}
        }
    }
    // Telegram forbids any entity overlapping code/pre. Split surrounding styles.
    let code: Vec<_> = r
        .spans
        .iter()
        .filter(|s| matches!(s.kind, "code" | "pre"))
        .cloned()
        .collect();
    let mut spans = Vec::new();
    for span in r.spans {
        let mut pieces = vec![span.clone()];
        if !matches!(span.kind, "code" | "pre") {
            for c in &code {
                pieces = pieces
                    .into_iter()
                    .flat_map(|s| {
                        if c.end <= s.start || c.start >= s.end {
                            return vec![s];
                        }
                        let mut out = Vec::new();
                        if s.start < c.start {
                            out.push(Span {
                                end: c.start,
                                ..s.clone()
                            });
                        }
                        if c.end < s.end {
                            out.push(Span { start: c.end, ..s });
                        }
                        out
                    })
                    .collect();
            }
        }
        spans.extend(pieces);
    }
    // Keep markup-only messages deliverable as literal text.
    let text = r.text.trim_end_matches('\n');
    let text = if text.trim().is_empty() { input } else { text };
    let mut parts = Vec::new();
    let mut byte_start = 0;
    let mut unit_start = 0;
    let mut units = 0;
    for (byte, ch) in text.char_indices() {
        if units + ch.len_utf16() > limit {
            parts.push(part(&text[byte_start..byte], unit_start, units, &spans));
            byte_start = byte;
            unit_start += units;
            units = 0;
        }
        units += ch.len_utf16();
    }
    if byte_start < text.len() {
        parts.push(part(&text[byte_start..], unit_start, units, &spans));
    }
    parts
}

fn safe_link(link: &str) -> bool {
    url::Url::parse(link).is_ok_and(|u| matches!(u.scheme(), "http" | "https" | "mailto"))
}

fn part(text: &str, start: usize, length: usize, spans: &[Span]) -> Part {
    let mut entities = Vec::new();
    for s in spans {
        let from = s.start.max(start);
        let to = s.end.min(start + length);
        if from < to {
            let mut entity = json!({"type":s.kind,"offset":from-start,"length":to-from});
            if let Some(url) = &s.url {
                entity["url"] = json!(url);
            }
            entities.push(entity);
        }
    }
    entities.sort_by_key(|e| {
        (
            e["offset"].as_u64().unwrap(),
            std::cmp::Reverse(e["length"].as_u64().unwrap()),
        )
    });
    entities.dedup();
    // Highly fragmented input should still deliver readable text.
    if entities.len() > 100 {
        entities.clear();
    }
    Part {
        text: text.into(),
        entities,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn common_mark_and_utf16_offsets() {
        let p = markdown(
            "😀 **bold** _italic_ ~~old~~ [site](https://example.com) `a_b <x>`",
            3600,
        );
        assert_eq!(p[0].text, "😀 bold italic old site a_b <x>");
        assert!(p[0]
            .entities
            .contains(&json!({"type":"bold","offset":3,"length":4})));
        assert!(p[0]
            .entities
            .iter()
            .any(|e| e["type"] == "text_link" && e["url"] == "https://example.com"));
        assert!(p[0]
            .entities
            .iter()
            .any(|e| e["type"] == "code" && e["length"] == 7));
    }

    #[test]
    fn long_code_and_emphasis_survive_chunk_boundaries() {
        for (source, kind, expected) in [
            (
                format!("**{}**", "😀x".repeat(2500)),
                "bold",
                "😀x".repeat(2500),
            ),
            (
                format!("```sh\n{}\n```", "😀x".repeat(2500)),
                "pre",
                "😀x".repeat(2500),
            ),
        ] {
            let parts = markdown(&source, 3600);
            assert_eq!(
                parts.iter().map(|p| p.text.as_str()).collect::<String>(),
                expected
            );
            assert!(parts.len() > 1);
            for p in parts {
                let units = p.text.encode_utf16().count();
                assert!(units <= 3600);
                assert_eq!(
                    p.entities,
                    vec![json!({"type":kind,"offset":0,"length":units})]
                );
            }
        }
    }

    #[test]
    fn code_does_not_overlap_styles_and_raw_html_is_literal() {
        let p = markdown(
            "**bold `code` end** <b>literal</b> [bad](javascript:evil)",
            3600,
        );
        assert_eq!(p[0].text, "bold code end <b>literal</b> bad");
        assert_eq!(
            p[0].entities,
            vec![
                json!({"type":"bold","offset":0,"length":5}),
                json!({"type":"code","offset":5,"length":4}),
                json!({"type":"bold","offset":9,"length":4}),
            ]
        );
        assert_eq!(
            markdown("unclosed **bold and `code", 3600)[0].text,
            "unclosed **bold and `code"
        );
        assert_eq!(
            markdown("# Heading\n\n- one\n- two", 3600)[0].text,
            "Heading\n• one\n• two"
        );
    }
}
