use serde::{Deserialize, Serialize};

use crate::doc::Doc;

/// Milliseconds since the Unix epoch.
pub type Millis = i64;

/// Wall-clock milliseconds, forced to increase strictly within a process so
/// that two notes saved in the same millisecond still sort in the order
/// they were written.
pub fn now_ms() -> Millis {
    use std::sync::atomic::{AtomicI64, Ordering};
    static LAST: AtomicI64 = AtomicI64::new(0);
    let wall = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0);
    let mut prev = LAST.load(Ordering::Relaxed);
    loop {
        let next = wall.max(prev + 1);
        match LAST.compare_exchange_weak(prev, next, Ordering::SeqCst, Ordering::Relaxed) {
            Ok(_) => return next,
            Err(actual) => prev = actual,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Note {
    pub id: String,
    pub folder_id: Option<String>,
    /// The title the writer typed. Empty means "use the first line".
    pub title: String,
    pub doc: Doc,
    pub color: Option<String>,
    pub pinned: bool,
    pub favorite: bool,
    pub created_at: Millis,
    pub updated_at: Millis,
    pub trashed_at: Option<Millis>,
}

impl Note {
    /// What a list shows as the title: the typed title, else the first line
    /// of text, else nothing (the front-end words an untitled note itself).
    pub fn display_title(&self) -> String {
        display_title(&self.title, &self.doc.plain_text())
    }
}

pub(crate) fn display_title(title: &str, plain: &str) -> String {
    if !title.trim().is_empty() {
        return title.trim().to_string();
    }
    let line = plain
        .lines()
        .find(|l| !l.trim().is_empty())
        .unwrap_or("")
        .trim();
    truncate(line, 100)
}

/// The text a list shows under the title.
pub(crate) fn preview(title: &str, plain: &str) -> String {
    let mut lines = plain
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .peekable();
    // With no typed title, the first line already is the title.
    if title.trim().is_empty() {
        lines.next();
    }
    truncate(&lines.collect::<Vec<_>>().join("  "), 240)
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let mut out: String = s.chars().take(max).collect();
    out.push('\u{2026}');
    out
}

/// A note without its body, for lists.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct NoteSummary {
    pub id: String,
    pub folder_id: Option<String>,
    pub title: String,
    pub preview: String,
    pub color: Option<String>,
    pub pinned: bool,
    pub favorite: bool,
    pub has_ink: bool,
    pub has_image: bool,
    pub created_at: Millis,
    pub updated_at: Millis,
    pub trashed_at: Option<Millis>,
    /// For search results: the matching passage, with the hits wrapped in
    /// `\u{2}` … `\u{3}` so a front-end can emphasise them without parsing
    /// markup.
    pub snippet: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Folder {
    pub id: String,
    pub parent_id: Option<String>,
    pub name: String,
    pub color: Option<String>,
    pub position: i64,
    /// Notes directly in this folder, trash excluded.
    pub note_count: i64,
}

/// Which notes a list shows.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum Scope {
    #[default]
    All,
    Folder(String),
    Unfiled,
    Favorites,
    Trash,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SortBy {
    #[default]
    Modified,
    Created,
    Title,
}

/// Note counts for the navigation pane.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Counts {
    pub all: i64,
    pub unfiled: i64,
    pub favorites: i64,
    pub trash: i64,
}

pub const HIT_START: char = '\u{2}';
pub const HIT_END: char = '\u{3}';
