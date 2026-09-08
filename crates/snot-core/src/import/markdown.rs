//! Markdown in, ProseMirror out — the inverse of `export::doc_to_markdown`.
//!
//! Parsing is left to `pulldown-cmark` because a Markdown directory rarely
//! comes from us: it comes from Obsidian, Bear, Logseq or a folder of files
//! somebody wrote by hand, and those need CommonMark, not a reader that only
//! understands what we ourselves emit. What the parser hands back is then
//! folded into the small set of nodes the editor's schema actually has. A
//! construct the schema cannot hold degrades to text rather than vanishing.

use std::path::Path;

use pulldown_cmark::{CodeBlockKind, Event, Options, Parser, Tag, TagEnd};
use serde_json::{json, Map, Value};

use crate::error::Result;
use crate::model::Millis;
use crate::store::Store;

use super::{mime_for, read_dir_sorted, text_to_doc, ImportSummary, MAX_DEPTH};

/// Extensions a Markdown directory turns into notes. `.txt` is here because
/// every note app that exports "plain text" produces a directory of them.
const NOTE_EXTENSIONS: [&str; 3] = ["md", "markdown", "txt"];

/// Converts Markdown to a ProseMirror document.
pub fn markdown_to_doc(md: &str) -> Value {
    let mut options = Options::empty();
    options.insert(Options::ENABLE_STRIKETHROUGH);
    options.insert(Options::ENABLE_TASKLISTS);
    // Tables are parsed rather than left alone so that the pipes and dashes
    // of the separator row do not survive as literal text; the cells become
    // ordinary paragraphs, since the editor has no table node to put them in.
    options.insert(Options::ENABLE_TABLES);

    let mut builder = Builder::default();
    for event in Parser::new_ext(md, options) {
        builder.event(event);
    }
    let content = builder.finish();
    json!({
        "type": "doc",
        "content": if content.is_empty() { vec![json!({"type": "paragraph"})] } else { content },
    })
}

// ------------------------------------------------------------------ builder

/// One node under construction. `ty` names the ProseMirror node it will
/// become, or begins with `__` for a frame that exists only to hold events
/// until it can be turned into something the schema knows about.
struct Frame {
    ty: &'static str,
    attrs: Map<String, Value>,
    content: Vec<Value>,
    /// Set on a list item by its `- [ ]` marker, if it has one.
    task: Option<bool>,
    /// Collected on a list frame as its items close, because whether a list
    /// is a task list is only known once its items have been seen.
    items: Vec<(Option<bool>, Vec<Value>)>,
}

impl Frame {
    fn new(ty: &'static str) -> Self {
        Frame {
            ty,
            attrs: Map::new(),
            content: vec![],
            task: None,
            items: vec![],
        }
    }
}

#[derive(Default)]
struct Builder {
    stack: Vec<Frame>,
    /// Marks covering the text being read, innermost last.
    marks: Vec<Value>,
    out: Vec<Value>,
}

impl Builder {
    fn event(&mut self, event: Event<'_>) {
        match event {
            Event::Start(tag) => self.open(tag),
            Event::End(TagEnd::Emphasis | TagEnd::Strong | TagEnd::Strikethrough) => {
                self.marks.pop();
            }
            Event::End(TagEnd::Link) => {
                self.marks.pop();
            }
            Event::End(_) => self.close(),
            Event::Text(text) => self.push_text(&text),
            Event::Code(code) => {
                // A code span swallows every other mark, exactly as the
                // exporter assumes when it writes one back out.
                let node = json!({
                    "type": "text",
                    "text": code.to_string(),
                    "marks": [{"type": "code"}],
                });
                self.push_inline(node);
            }
            // Raw HTML has no home in the schema, so it stays visible as the
            // text it is rather than being dropped on the floor.
            Event::Html(html) | Event::InlineHtml(html) => {
                self.push_plain(html.trim_end_matches('\n'))
            }
            Event::FootnoteReference(name) => self.push_plain(&format!("[^{name}]")),
            Event::InlineMath(math) => self.push_plain(&format!("${math}$")),
            Event::DisplayMath(math) => self.push_plain(&format!("$${math}$$")),
            // A line the author broke is a line they meant; a note is not a
            // reflowed article, so a soft break keeps its break.
            Event::SoftBreak | Event::HardBreak => self.push_inline(json!({"type": "hardBreak"})),
            Event::Rule => self.push_block(json!({"type": "horizontalRule"})),
            Event::TaskListMarker(done) => {
                if let Some(item) = self.stack.iter_mut().rev().find(|f| f.ty == "__item") {
                    item.task = Some(done);
                }
            }
        }
    }

    fn open(&mut self, tag: Tag<'_>) {
        match tag {
            Tag::Emphasis => self.marks.push(json!({"type": "italic"})),
            Tag::Strong => self.marks.push(json!({"type": "bold"})),
            Tag::Strikethrough => self.marks.push(json!({"type": "strike"})),
            Tag::Link { dest_url, .. } => self
                .marks
                .push(json!({"type": "link", "attrs": {"href": dest_url.to_string()}})),
            Tag::Paragraph => self.stack.push(Frame::new("paragraph")),
            Tag::Heading { level, .. } => {
                let mut frame = Frame::new("heading");
                // The editor offers three heading levels; storing a fourth
                // would leave the document saying something the editor then
                // silently renders as something else.
                let level = (level as u8).clamp(1, 3);
                frame.attrs.insert("level".into(), json!(level));
                self.stack.push(frame);
            }
            Tag::BlockQuote(_) => self.stack.push(Frame::new("blockquote")),
            Tag::CodeBlock(kind) => {
                let mut frame = Frame::new("codeBlock");
                let language = match kind {
                    CodeBlockKind::Fenced(lang) => lang.to_string(),
                    CodeBlockKind::Indented => String::new(),
                };
                frame.attrs.insert(
                    "language".into(),
                    if language.is_empty() {
                        Value::Null
                    } else {
                        json!(language)
                    },
                );
                self.stack.push(frame);
            }
            Tag::List(start) => {
                let mut frame = Frame::new("__list");
                if let Some(start) = start {
                    frame.attrs.insert("start".into(), json!(start));
                }
                self.stack.push(frame);
            }
            Tag::Item => self.stack.push(Frame::new("__item")),
            Tag::Image {
                dest_url, title, ..
            } => {
                let mut frame = Frame::new("__image");
                frame
                    .attrs
                    .insert("src".into(), json!(dest_url.to_string()));
                if !title.is_empty() {
                    frame.attrs.insert("title".into(), json!(title.to_string()));
                }
                self.stack.push(frame);
            }
            Tag::Table(_) => self.stack.push(Frame::new("__table")),
            Tag::TableHead | Tag::TableRow => self.stack.push(Frame::new("__row")),
            Tag::TableCell => self.stack.push(Frame::new("__cell")),
            // Everything else — an HTML block, a footnote definition, a
            // definition list — becomes a frame that pours its children into
            // whatever encloses it, so the text survives without a node type
            // being invented for it.
            _ => self.stack.push(Frame::new("__transparent")),
        }
    }

    fn close(&mut self) {
        let Some(frame) = self.stack.pop() else {
            return;
        };
        match frame.ty {
            "paragraph" => {
                for block in group_blocks(frame.content) {
                    self.push_block(block);
                }
            }
            "heading" => {
                // A heading holds inline content only. An image inside one
                // follows it as its own block instead of being thrown away.
                let (inline, blocks) = split_off_blocks(frame.content);
                self.push_block(json!({
                    "type": "heading",
                    "attrs": Value::Object(frame.attrs),
                    "content": inline,
                }));
                for block in blocks {
                    self.push_block(block);
                }
            }
            "blockquote" => {
                let content = group_blocks(frame.content);
                self.push_block(json!({"type": "blockquote", "content": content}));
            }
            "codeBlock" => {
                let text: String = frame
                    .content
                    .iter()
                    .filter_map(|n| n.get("text").and_then(Value::as_str))
                    .collect();
                let content = if text.is_empty() {
                    vec![]
                } else {
                    vec![json!({"type": "text", "text": text})]
                };
                self.push_block(json!({
                    "type": "codeBlock",
                    "attrs": Value::Object(frame.attrs),
                    "content": content,
                }));
            }
            "__item" => {
                let body = group_blocks(frame.content);
                if let Some(list) = self.stack.last_mut() {
                    list.items.push((frame.task, body));
                }
            }
            "__list" => self.close_list(frame),
            "__cell" => {
                if let Some(row) = self.stack.last_mut() {
                    if !row.content.is_empty() {
                        row.content
                            .push(json!({"type": "text", "text": " \u{2502} "}));
                    }
                    row.content.extend(frame.content);
                }
            }
            "__row" => {
                if !frame.content.is_empty() {
                    self.push_block(json!({"type": "paragraph", "content": frame.content}));
                }
            }
            _ => {
                // `__table`, `__image` handled below, and anything transparent.
                if frame.ty == "__image" {
                    let alt: String = frame
                        .content
                        .iter()
                        .filter_map(|n| n.get("text").and_then(Value::as_str))
                        .collect();
                    let mut attrs = frame.attrs;
                    attrs.insert("alt".into(), json!(alt));
                    self.push_inline(json!({"type": "image", "attrs": Value::Object(attrs)}));
                } else {
                    for node in frame.content {
                        self.push_inline(node);
                    }
                }
            }
        }
    }

    /// Turns a parsed list into the node the schema has for it. A list whose
    /// items carry checkboxes is a `taskList`; one whose items do not is a
    /// bullet or ordered list. A list that mixes the two becomes a run of
    /// sibling lists, because collapsing it either way would either invent a
    /// checkbox or throw one away.
    fn close_list(&mut self, frame: Frame) {
        let ordered = frame.attrs.contains_key("start");
        let mut run: Vec<(Option<bool>, Vec<Value>)> = vec![];
        let mut run_is_task = None;
        let mut start = frame
            .attrs
            .get("start")
            .and_then(Value::as_u64)
            .unwrap_or(1);

        for (task, body) in frame.items {
            // An ordered list has no checkbox spelling, so a marker inside one
            // stays as the text it was written as.
            let is_task = task.is_some() && !ordered;
            if run_is_task.is_some_and(|was| was != is_task) {
                let emitted = run.len() as u64;
                self.emit_list(
                    run_is_task == Some(true),
                    ordered,
                    start,
                    std::mem::take(&mut run),
                );
                start += emitted;
            }
            run_is_task = Some(is_task);
            let body = match task {
                Some(done) if ordered => prefix_text(body, if done { "[x] " } else { "[ ] " }),
                _ => body,
            };
            run.push((task, body));
        }
        if !run.is_empty() {
            self.emit_list(run_is_task == Some(true), ordered, start, run);
        }
    }

    fn emit_list(
        &mut self,
        task: bool,
        ordered: bool,
        start: u64,
        items: Vec<(Option<bool>, Vec<Value>)>,
    ) {
        let content: Vec<Value> = items
            .into_iter()
            .map(|(checked, body)| {
                if task {
                    json!({
                        "type": "taskItem",
                        "attrs": {"checked": checked.unwrap_or(false)},
                        "content": body,
                    })
                } else {
                    json!({"type": "listItem", "content": body})
                }
            })
            .collect();
        let node = if task {
            json!({"type": "taskList", "content": content})
        } else if ordered {
            json!({"type": "orderedList", "attrs": {"start": start}, "content": content})
        } else {
            json!({"type": "bulletList", "content": content})
        };
        self.push_block(node);
    }

    fn push_text(&mut self, text: &str) {
        // Inside a fence every byte is content: a `==` there is two equals
        // signs, and a mark applied to it would come back out as four.
        if self.stack.iter().any(|f| f.ty == "codeBlock") {
            self.push_inline(json!({"type": "text", "text": text}));
            return;
        }
        let highlighted = self.marks.iter().any(|m| m["type"] == "highlight");
        for (chunk, mark) in split_highlight(text) {
            if chunk.is_empty() {
                continue;
            }
            let mut marks = self.marks.clone();
            if mark && !highlighted {
                marks.push(json!({"type": "highlight"}));
            }
            let mut node = json!({"type": "text", "text": chunk});
            if !marks.is_empty() {
                node["marks"] = Value::Array(marks);
            }
            self.push_inline(node);
        }
    }

    /// Text that must survive verbatim, with no mark of any kind read into it.
    fn push_plain(&mut self, text: &str) {
        if !text.is_empty() {
            self.push_inline(json!({"type": "text", "text": text}));
        }
    }

    fn push_inline(&mut self, node: Value) {
        match self.stack.last_mut() {
            Some(frame) => frame.content.push(node),
            None => self.out.push(node),
        }
    }

    fn push_block(&mut self, node: Value) {
        match self.stack.last_mut() {
            Some(frame) => frame.content.push(node),
            None => self.out.push(node),
        }
    }

    fn finish(mut self) -> Vec<Value> {
        while !self.stack.is_empty() {
            self.close();
        }
        group_blocks(self.out)
    }
}

/// Whether a node is one that lives inside a paragraph rather than beside it.
fn is_inline(node: &Value) -> bool {
    matches!(
        node.get("type").and_then(Value::as_str),
        Some("text") | Some("hardBreak")
    )
}

/// Wraps every run of inline nodes in a paragraph and leaves block nodes
/// alone. This is what makes a tight list item — which the parser reports as
/// bare text — and an image sitting in the middle of a paragraph both come
/// out as something the schema accepts.
fn group_blocks(content: Vec<Value>) -> Vec<Value> {
    let mut out: Vec<Value> = vec![];
    let mut run: Vec<Value> = vec![];
    for node in content {
        if is_inline(&node) {
            run.push(node);
        } else {
            if !run.is_empty() {
                out.push(json!({"type": "paragraph", "content": std::mem::take(&mut run)}));
            }
            out.push(node);
        }
    }
    if !run.is_empty() {
        out.push(json!({"type": "paragraph", "content": run}));
    }
    out
}

fn split_off_blocks(content: Vec<Value>) -> (Vec<Value>, Vec<Value>) {
    content.into_iter().partition(is_inline)
}

/// Puts a literal prefix in front of a list item's first line.
fn prefix_text(mut body: Vec<Value>, prefix: &str) -> Vec<Value> {
    let node = json!({"type": "text", "text": prefix});
    match body.first_mut().and_then(|b| b.get_mut("content")) {
        Some(Value::Array(content)) => content.insert(0, node),
        _ => body.insert(0, json!({"type": "paragraph", "content": [node]})),
    }
    body
}

/// Splits text on `==highlight==`. That spelling is Obsidian's and Bear's,
/// not CommonMark's, so the parser hands it over as ordinary text and the
/// marking is ours to do. An unpaired or empty `==` stays as written.
fn split_highlight(text: &str) -> Vec<(&str, bool)> {
    let mut out = vec![];
    let mut plain = 0;
    let mut cursor = 0;
    while let Some(open) = text[cursor..].find("==").map(|i| cursor + i) {
        match text[open + 2..].find("==").map(|i| open + 2 + i) {
            Some(close) if close > open + 2 => {
                if open > plain {
                    out.push((&text[plain..open], false));
                }
                out.push((&text[open + 2..close], true));
                cursor = close + 2;
                plain = cursor;
            }
            Some(_) => cursor = open + 2,
            None => break,
        }
    }
    if plain < text.len() {
        out.push((&text[plain..], false));
    }
    out
}

// ------------------------------------------------------------ front matter

/// What a Markdown file can say about itself before its body starts.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FrontMatter {
    pub title: Option<String>,
    pub tags: Vec<String>,
    pub created: Option<Millis>,
    pub updated: Option<Millis>,
}

/// Peels a YAML front-matter block off the top of a file.
///
/// Hand-parsed rather than handed to a YAML crate on purpose: the four keys
/// below are the whole of what a note can usefully say about itself, and
/// everything else in the block — nested maps, anchors, whatever the source
/// app happened to write — is none of our business and is skipped. A note is
/// never rejected for having front matter we do not understand.
pub fn split_front_matter(src: &str) -> (FrontMatter, &str) {
    let body = src.strip_prefix("---").and_then(|rest| {
        let rest = rest.strip_prefix('\r').unwrap_or(rest);
        rest.strip_prefix('\n')
    });
    let Some(body) = body else {
        return (FrontMatter::default(), src);
    };
    let Some(end) = body
        .match_indices('\n')
        .map(|(i, _)| i + 1)
        .chain([0])
        .find(|&i| {
            let line = body[i..].lines().next().unwrap_or("").trim_end();
            line == "---" || line == "..."
        })
    else {
        return (FrontMatter::default(), src);
    };

    let mut front = FrontMatter::default();
    let mut list_key = String::new();
    for line in body[..end].lines() {
        let trimmed = line.trim();
        if let Some(item) = trimmed.strip_prefix("- ") {
            if list_key == "tags" {
                front.tags.extend(split_tags(item));
            }
            continue;
        }
        let Some((key, value)) = trimmed.split_once(':') else {
            continue;
        };
        let key = key.trim().to_ascii_lowercase();
        let value = unquote(value.trim());
        list_key = if value.is_empty() {
            key.clone()
        } else {
            String::new()
        };
        match key.as_str() {
            "title" | "name" if !value.is_empty() => front.title = Some(value.to_string()),
            "tags" | "tag" | "keywords" => front.tags.extend(split_tags(value)),
            "created" | "date" | "created_at" | "createdat" => front.created = parse_date(value),
            "updated" | "modified" | "updated_at" | "updatedat" | "last_modified" => {
                front.updated = parse_date(value)
            }
            _ => {}
        }
    }
    // Skip the closing fence line as well as the block itself.
    let rest = &body[end..];
    let rest = rest.split_once('\n').map(|(_, r)| r).unwrap_or("");
    (front, rest)
}

fn unquote(value: &str) -> &str {
    for quote in ['"', '\''] {
        if let Some(inner) = value
            .strip_prefix(quote)
            .and_then(|v| v.strip_suffix(quote))
        {
            return inner;
        }
    }
    value
}

fn split_tags(value: &str) -> Vec<String> {
    let value = value.trim().trim_start_matches('[').trim_end_matches(']');
    value
        .split(',')
        .map(|t| unquote(t.trim()).trim_start_matches('#').trim().to_string())
        .filter(|t| !t.is_empty())
        .collect()
}

/// Reads the date spellings a note export actually uses: an ISO date, an ISO
/// timestamp with or without an offset, or plain epoch milliseconds. Anything
/// else leaves the note dated by when it was imported, which is honest.
fn parse_date(value: &str) -> Option<Millis> {
    let value = unquote(value.trim());
    if value.is_empty() {
        return None;
    }
    if let Ok(epoch) = value.parse::<i64>() {
        // Seconds and milliseconds are told apart by magnitude: a plausible
        // note is not dated in the year 33658.
        return Some(if epoch.abs() < 100_000_000_000 {
            epoch * 1000
        } else {
            epoch
        });
    }
    let (date, rest) = value.split_once(['T', ' ']).unwrap_or((value, ""));
    let mut parts = date.split('-');
    let year: i64 = parts.next()?.parse().ok()?;
    let month: i64 = parts.next()?.parse().ok()?;
    let day: i64 = parts.next()?.parse().ok()?;
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }

    let (time, offset) = match rest.find(['Z', '+']) {
        Some(i) => rest.split_at(i),
        None => match rest.rfind('-') {
            Some(i) if i > 0 => rest.split_at(i),
            _ => (rest, ""),
        },
    };
    let mut clock = time.trim_end_matches('Z').split(':');
    let hour: i64 = clock.next().and_then(|h| h.parse().ok()).unwrap_or(0);
    let minute: i64 = clock.next().and_then(|m| m.parse().ok()).unwrap_or(0);
    let second: i64 = clock
        .next()
        .and_then(|s| s.split('.').next()?.parse().ok())
        .unwrap_or(0);

    let mut millis =
        (days_from_civil(year, month, day) * 86_400 + hour * 3600 + minute * 60 + second) * 1000;
    if let Some(sign) = offset.chars().next().filter(|c| *c == '+' || *c == '-') {
        let mut fields = offset[1..].split(':');
        let oh: i64 = fields.next().and_then(|h| h.parse().ok()).unwrap_or(0);
        let om: i64 = fields.next().and_then(|m| m.parse().ok()).unwrap_or(0);
        let shift = (oh * 3600 + om * 60) * 1000;
        millis += if sign == '+' { -shift } else { shift };
    }
    Some(millis)
}

/// Days between 1970-01-01 and the given date, by Howard Hinnant's algorithm.
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let year = year - i64::from(month <= 2);
    let era = if year >= 0 { year } else { year - 399 } / 400;
    let yoe = year - era * 400;
    let doy = (153 * (month + if month > 2 { -3 } else { 9 }) + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// Takes a leading `# Heading` off the body and returns it as the title.
///
/// `export_markdown` writes the note's title as exactly that heading, so
/// consuming it is what keeps a note exported and re-imported from growing a
/// second copy of its own name.
pub fn split_leading_title(body: &str) -> (Option<String>, &str) {
    let start = body.len() - body.trim_start_matches(['\n', '\r', ' ', '\t']).len();
    let rest = &body[start..];
    let Some(line) = rest.lines().next() else {
        return (None, body);
    };
    let Some(title) = line.strip_prefix("# ") else {
        return (None, body);
    };
    let after = rest[line.len()..].strip_prefix('\n').unwrap_or("");
    (Some(title.trim().to_string()), after)
}

// -------------------------------------------------------- directory import

/// Imports a directory tree of Markdown as notes and folders.
///
/// The shape on disk is the shape in the library: a file becomes a note, a
/// subdirectory becomes a folder holding what was inside it.
pub fn import_markdown_dir(
    store: &Store,
    dir: &Path,
    parent_folder: Option<&str>,
) -> Result<ImportSummary> {
    let mut summary = ImportSummary::default();
    walk(store, dir, parent_folder, 0, &mut summary)?;
    Ok(summary)
}

fn walk(
    store: &Store,
    dir: &Path,
    folder: Option<&str>,
    depth: usize,
    summary: &mut ImportSummary,
) -> Result<()> {
    if depth > MAX_DEPTH {
        return Ok(());
    }
    for entry in read_dir_sorted(dir)? {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        // A dot-directory is the source app's own bookkeeping — `.obsidian`,
        // `.git`, `.trash` — and never the user's notes.
        if name.starts_with('.') {
            continue;
        }
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        if kind.is_symlink() {
            continue;
        }
        if kind.is_dir() {
            // The folder is created only if the subtree has a note in it, so
            // that an `images/` directory holding nothing but attachments
            // does not turn into an empty folder in the sidebar.
            if !holds_notes(&path, depth) {
                continue;
            }
            let child = store.create_folder(&name, folder)?;
            summary.folders += 1;
            walk(store, &path, Some(&child.id), depth + 1, summary)?;
        } else if is_note_file(&path) {
            if let Err(err) = import_file(store, &path, folder, summary) {
                summary.fail(&path, err.to_string());
            }
        }
    }
    Ok(())
}

fn is_note_file(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .is_some_and(|e| NOTE_EXTENSIONS.contains(&e.as_str()))
}

fn holds_notes(dir: &Path, depth: usize) -> bool {
    if depth > MAX_DEPTH {
        return false;
    }
    let Ok(entries) = read_dir_sorted(dir) else {
        return false;
    };
    entries.into_iter().any(|entry| {
        let path = entry.path();
        if entry.file_name().to_string_lossy().starts_with('.') {
            return false;
        }
        match entry.file_type() {
            Ok(kind) if kind.is_dir() => holds_notes(&path, depth + 1),
            Ok(kind) if kind.is_file() => is_note_file(&path),
            _ => false,
        }
    })
}

fn import_file(
    store: &Store,
    path: &Path,
    folder: Option<&str>,
    summary: &mut ImportSummary,
) -> Result<()> {
    let raw = std::fs::read(path)?;
    let text = String::from_utf8_lossy(&raw);
    let text = text.strip_prefix('\u{feff}').unwrap_or(&text);
    let stem = path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();

    // A `.txt` is read as the plain text it says it is. Running it through a
    // Markdown parser would turn a shopping list's asterisks into bullets and
    // an underscore in a filename into italics.
    let plain = path
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("txt"));

    let (front, body) = if plain {
        (FrontMatter::default(), text)
    } else {
        split_front_matter(text)
    };
    let (heading, body) = if plain {
        (None, body)
    } else {
        split_leading_title(body)
    };
    let title = front
        .title
        .clone()
        .or(heading)
        .filter(|t| !t.trim().is_empty())
        .unwrap_or(stem);

    let mut doc = if plain {
        text_to_doc(body)
    } else {
        markdown_to_doc(body)
    };

    let note = store.create_note(folder, None)?;
    let id = note.summary.id;
    let base = path.parent().unwrap_or(Path::new("."));
    summary.attachments += attach_local_images(store, &mut doc, base, &id)?;

    let tags = front
        .tags
        .iter()
        .map(|name| Ok(store.ensure_tag(name)?.id))
        .collect::<Result<Vec<String>>>()?;
    store.update_note(
        &id,
        crate::model::NotePatch {
            title: Some(title),
            doc: Some(doc),
            tags: if tags.is_empty() { None } else { Some(tags) },
            ..Default::default()
        },
    )?;

    if front.created.is_some() || front.updated.is_some() {
        let fallback = store.get_note(&id)?.summary;
        store.backdate(
            &id,
            front.created.unwrap_or(fallback.created_at),
            front
                .updated
                .or(front.created)
                .unwrap_or(fallback.updated_at),
        )?;
    }
    summary.notes += 1;
    Ok(())
}

/// Pulls the pictures a note points at into the library.
///
/// A relative link is read against the file's own directory, stored by
/// content like any other attachment, and rewritten to where it now lives. A
/// link that does not resolve — a URL, or a path to something that is not
/// there — is left exactly as written, because a broken attachment is worse
/// than a link the user can still read and fix.
fn attach_local_images(
    store: &Store,
    doc: &mut Value,
    base: &Path,
    note_id: &str,
) -> Result<usize> {
    let mut count = 0;
    for node in image_nodes(doc) {
        let Some(src) = node
            .get("attrs")
            .and_then(|a| a.get("src"))
            .and_then(Value::as_str)
        else {
            continue;
        };
        if src.contains("://") || src.starts_with("data:") || src.is_empty() {
            continue;
        }
        let relative = percent_decode(src.split(['?', '#']).next().unwrap_or(src));
        let path = base.join(relative.trim_start_matches("./"));
        let Ok(bytes) = std::fs::read(&path) else {
            continue;
        };
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "image".into());
        let mime = mime_for(&name);
        let (_, stored) = store.put_attachment(Some(note_id), &name, mime, &bytes)?;
        node["attrs"]["src"] = json!(stored.to_string_lossy());
        count += 1;
    }
    Ok(count)
}

fn image_nodes(doc: &mut Value) -> Vec<&mut Value> {
    let mut found = vec![];
    let mut frontier = vec![doc];
    while let Some(node) = frontier.pop() {
        if node.get("type").and_then(Value::as_str) == Some("image") {
            found.push(node);
            continue;
        }
        if let Some(children) = node.get_mut("content").and_then(Value::as_array_mut) {
            frontier.extend(children.iter_mut());
        }
    }
    found
}

fn percent_decode(src: &str) -> String {
    let bytes = src.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(byte) = u8::from_str_radix(&src[i + 1..i + 3], 16) {
                out.push(byte);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}
