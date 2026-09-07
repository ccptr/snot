use serde_json::Value;

/// Renders a ProseMirror document as Markdown. This is the export format and
/// also what lands on the clipboard when a note is copied out of the app, so
/// a note can always leave in a form other tools read.
pub fn doc_to_markdown(doc: &Value) -> String {
    let mut out = String::new();
    if let Some(content) = doc.get("content").and_then(Value::as_array) {
        for node in content {
            block(node, &mut out, 0);
        }
    }
    out.trim_end().to_string() + "\n"
}

fn block(node: &Value, out: &mut String, depth: usize) {
    let ty = node.get("type").and_then(Value::as_str).unwrap_or("");
    let pad = "  ".repeat(depth);
    match ty {
        "heading" => {
            let level = node
                .get("attrs")
                .and_then(|a| a.get("level"))
                .and_then(Value::as_u64)
                .unwrap_or(1)
                .clamp(1, 6) as usize;
            out.push_str(&"#".repeat(level));
            out.push(' ');
            inline(node, out);
            out.push_str("\n\n");
        }
        "paragraph" => {
            let start = out.len();
            inline(node, out);
            if out.len() == start {
                // An empty paragraph is a blank line, not a stray newline pair.
                out.push('\n');
            } else {
                out.push_str("\n\n");
            }
        }
        "bulletList" | "orderedList" => {
            let ordered = ty == "orderedList";
            let mut n = node
                .get("attrs")
                .and_then(|a| a.get("start"))
                .and_then(Value::as_u64)
                .unwrap_or(1);
            for item in children(node) {
                out.push_str(&pad);
                if ordered {
                    out.push_str(&format!("{n}. "));
                    n += 1;
                } else {
                    out.push_str("- ");
                }
                list_item_body(item, out, depth);
            }
            out.push('\n');
        }
        "taskList" => {
            for item in children(node) {
                let done = item
                    .get("attrs")
                    .and_then(|a| a.get("checked"))
                    .and_then(Value::as_bool)
                    .unwrap_or(false);
                out.push_str(&pad);
                out.push_str(if done { "- [x] " } else { "- [ ] " });
                list_item_body(item, out, depth);
            }
            out.push('\n');
        }
        "blockquote" => {
            let mut inner = String::new();
            for child in children(node) {
                block(child, &mut inner, depth);
            }
            for line in inner.trim_end().lines() {
                out.push_str("> ");
                out.push_str(line);
                out.push('\n');
            }
            out.push('\n');
        }
        "codeBlock" => {
            let lang = node
                .get("attrs")
                .and_then(|a| a.get("language"))
                .and_then(Value::as_str)
                .unwrap_or("");
            out.push_str("```");
            out.push_str(lang);
            out.push('\n');
            let mut body = String::new();
            inline(node, &mut body);
            out.push_str(&body);
            if !body.ends_with('\n') {
                out.push('\n');
            }
            out.push_str("```\n\n");
        }
        "horizontalRule" => out.push_str("---\n\n"),
        "image" => {
            let attrs = node.get("attrs");
            let src = attrs
                .and_then(|a| a.get("src"))
                .and_then(Value::as_str)
                .unwrap_or("");
            let alt = attrs
                .and_then(|a| a.get("alt"))
                .and_then(Value::as_str)
                .unwrap_or("image");
            out.push_str(&format!("![{alt}]({src})\n\n"));
        }
        // Ink has no Markdown spelling; name it so the export is not silently
        // lossy about a note that is mostly a drawing.
        "ink" => out.push_str("*[handwritten drawing]*\n\n"),
        _ => {
            for child in children(node) {
                block(child, out, depth);
            }
        }
    }
}

fn list_item_body(item: &Value, out: &mut String, depth: usize) {
    let kids: Vec<&Value> = children(item).collect();
    let mut first = true;
    for child in kids {
        let ty = child.get("type").and_then(Value::as_str).unwrap_or("");
        if matches!(ty, "bulletList" | "orderedList" | "taskList") {
            block(child, out, depth + 1);
        } else if first {
            inline(child, out);
            out.push('\n');
            first = false;
        } else {
            out.push_str(&"  ".repeat(depth + 1));
            inline(child, out);
            out.push('\n');
        }
    }
    if first {
        out.push('\n');
    }
}

fn children(node: &Value) -> impl Iterator<Item = &Value> {
    node.get("content")
        .and_then(Value::as_array)
        .map(|v| v.iter())
        .unwrap_or_else(|| [].iter())
}

fn inline(node: &Value, out: &mut String) {
    for child in children(node) {
        match child.get("type").and_then(Value::as_str) {
            Some("text") => {
                let text = child.get("text").and_then(Value::as_str).unwrap_or("");
                let marks: Vec<&str> = child
                    .get("marks")
                    .and_then(Value::as_array)
                    .map(|m| {
                        m.iter()
                            .filter_map(|x| x.get("type").and_then(Value::as_str))
                            .collect()
                    })
                    .unwrap_or_default();
                let (open, close) = wrap_for(&marks);
                out.push_str(&open);
                out.push_str(text);
                out.push_str(&close);
            }
            Some("hardBreak") => out.push_str("  \n"),
            Some("image") => {
                let attrs = child.get("attrs");
                let src = attrs
                    .and_then(|a| a.get("src"))
                    .and_then(Value::as_str)
                    .unwrap_or("");
                let alt = attrs
                    .and_then(|a| a.get("alt"))
                    .and_then(Value::as_str)
                    .unwrap_or("image");
                out.push_str(&format!("![{alt}]({src})"));
            }
            _ => inline(child, out),
        }
    }
}

fn wrap_for(marks: &[&str]) -> (String, String) {
    let mut open = String::new();
    let mut close = String::new();
    // `code` swallows the others: `**bold**` inside a code span is literal.
    if marks.contains(&"code") {
        return ("`".into(), "`".into());
    }
    for (mark, delim) in [
        ("bold", "**"),
        ("italic", "*"),
        ("strike", "~~"),
        ("highlight", "=="),
    ] {
        if marks.contains(&mark) {
            open.push_str(delim);
            close.insert_str(0, delim);
        }
    }
    (open, close)
}
