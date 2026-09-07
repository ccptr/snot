use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Milliseconds since the Unix epoch. Everything in the store is stored this
/// way so that ordering is a plain integer comparison.
pub type Millis = i64;

static LAST_MS: std::sync::atomic::AtomicI64 = std::sync::atomic::AtomicI64::new(0);

/// Wall-clock milliseconds, forced to be strictly increasing within a process.
///
/// Two notes created in the same millisecond would otherwise tie in every
/// "most recent first" list and come back in arbitrary order; nudging the
/// clock forward by a millisecond keeps the ordering stable, and also stops a
/// backwards clock adjustment from reshuffling the library.
pub fn now_ms() -> Millis {
    use std::sync::atomic::Ordering::{Relaxed, SeqCst};
    let wall = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0);
    let mut prev = LAST_MS.load(Relaxed);
    loop {
        let next = if wall > prev { wall } else { prev + 1 };
        match LAST_MS.compare_exchange_weak(prev, next, SeqCst, Relaxed) {
            Ok(_) => return next,
            Err(actual) => prev = actual,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Folder {
    pub id: String,
    pub parent_id: Option<String>,
    pub name: String,
    pub color: Option<String>,
    pub position: i64,
    pub created_at: Millis,
    pub updated_at: Millis,
    /// Notes living directly in this folder, trash excluded.
    #[serde(default)]
    pub note_count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Tag {
    pub id: String,
    pub name: String,
    pub color: Option<String>,
    #[serde(default)]
    pub note_count: i64,
}

/// A note without its body. Note lists can get long, and the body of a note
/// carrying ink or images is large, so listing never pays for it.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NoteSummary {
    pub id: String,
    pub folder_id: Option<String>,
    pub title: String,
    pub preview: String,
    pub color: Option<String>,
    pub pinned: bool,
    pub favorite: bool,
    pub locked: bool,
    pub created_at: Millis,
    pub updated_at: Millis,
    pub trashed_at: Option<Millis>,
    #[serde(default)]
    pub tags: Vec<String>,
    /// Whether the note carries page ink, so a list can show a pen glyph
    /// without loading the strokes themselves.
    #[serde(default)]
    pub has_ink: bool,
    /// Search-result highlight, only populated by `search`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub snippet: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Note {
    #[serde(flatten)]
    pub summary: NoteSummary,
    /// ProseMirror document JSON.
    pub doc: Value,
    /// Handwriting drawn across the whole page, as an array of vector
    /// strokes. It sits beside the document rather than inside it because it
    /// is not part of the text flow — a stroke can cross any number of
    /// paragraphs, or sit on a blank part of the page.
    #[serde(default)]
    pub ink: Value,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NotePatch {
    pub title: Option<String>,
    pub doc: Option<Value>,
    pub ink: Option<Value>,
    pub folder_id: Option<Option<String>>,
    pub color: Option<Option<String>>,
    pub pinned: Option<bool>,
    pub favorite: Option<bool>,
    pub locked: Option<bool>,
    pub tags: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Attachment {
    pub id: String,
    pub note_id: Option<String>,
    pub mime: String,
    pub name: String,
    pub size: i64,
    pub sha256: String,
    pub created_at: Millis,
}

/// Which slice of the library a note list should show.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Scope {
    /// Every note that is not in the trash.
    #[default]
    All,
    /// Notes filed directly under one folder.
    Folder {
        id: String,
    },
    /// Notes with no folder.
    Unfiled,
    Favorites,
    Tag {
        id: String,
    },
    Trash,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum SortBy {
    #[default]
    Updated,
    Created,
    Title,
}

/// Flattens a ProseMirror document to plain text: one line per block node.
/// Used for the list preview and as the body fed to the full-text index, so
/// what you see in search results is what search actually matched on.
pub fn doc_to_text(doc: &Value) -> String {
    let mut out = String::new();
    walk(doc, &mut out);
    // Collapse the runs of blank lines that block nesting tends to produce.
    let mut cleaned = String::with_capacity(out.len());
    let mut blank = true;
    for line in out.lines() {
        let line = line.trim_end();
        if line.is_empty() {
            if !blank {
                cleaned.push('\n');
                blank = true;
            }
        } else {
            cleaned.push_str(line);
            cleaned.push('\n');
            blank = false;
        }
    }
    cleaned.trim_end().to_string()
}

fn walk(node: &Value, out: &mut String) {
    match node.get("type").and_then(Value::as_str) {
        Some("text") => {
            if let Some(t) = node.get("text").and_then(Value::as_str) {
                out.push_str(t);
            }
        }
        // Images carry no text; a marker keeps the preview honest about a
        // note that is entirely pictures.
        Some("image") => {
            let alt = node
                .get("attrs")
                .and_then(|a| a.get("alt"))
                .and_then(Value::as_str);
            out.push_str(alt.unwrap_or("\u{1f5bc} image"));
            out.push('\n');
        }
        Some("hardBreak") => out.push('\n'),
        _ => {}
    }

    let is_block = node
        .get("type")
        .and_then(Value::as_str)
        .is_some_and(|t| !matches!(t, "text" | "hardBreak" | "image"));

    if let Some(children) = node.get("content").and_then(Value::as_array) {
        for child in children {
            walk(child, out);
        }
    }
    if is_block && !out.ends_with('\n') {
        out.push('\n');
    }
}

/// Samsung Notes titles a note by its first line unless you rename it; so do we.
pub fn derive_title(text: &str) -> String {
    let line = text
        .lines()
        .find(|l| !l.trim().is_empty())
        .unwrap_or("")
        .trim();
    let mut title: String = line.chars().take(120).collect();
    if line.chars().count() > 120 {
        title.push('\u{2026}');
    }
    title
}

pub fn derive_preview(text: &str) -> String {
    // Skip the line that became the title.
    let mut lines = text.lines().skip_while(|l| l.trim().is_empty());
    lines.next();
    let rest: Vec<&str> = lines.map(str::trim).filter(|l| !l.is_empty()).collect();
    let joined = rest.join("  ");
    let mut preview: String = joined.chars().take(300).collect();
    if joined.chars().count() > 300 {
        preview.push('\u{2026}');
    }
    preview
}
