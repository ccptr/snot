//! Bringing other people's notes in.
//!
//! Two shapes are supported: a directory of Markdown, which is what Obsidian,
//! Bear, Logseq and our own exporter produce, and a Samsung Notes export.
//! Both walk a tree on disk and mirror it into the library, and both hold to
//! the same rule — a file the importer meets is never silently dropped. If it
//! cannot be converted it still becomes a note carrying the original as an
//! attachment, so the data is in the library and can be got at again.

mod markdown;
mod samsung;

use std::path::Path;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::error::Result;
use crate::model::NotePatch;
use crate::store::Store;

pub use markdown::{import_markdown_dir, markdown_to_doc, split_front_matter, FrontMatter};
pub use samsung::import_samsung_dir;

/// How deep a directory tree is followed. Deep enough for any real note
/// library, shallow enough that a pathological tree cannot run us out of
/// stack.
const MAX_DEPTH: usize = 24;

/// What an import did, in the terms the user cares about.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportSummary {
    /// Notes created, converted and unconverted alike.
    pub notes: usize,
    pub folders: usize,
    /// Images and other files pulled into the attachment store.
    pub attachments: usize,
    /// Files that could not be read as notes and are attached as they are.
    pub unconverted: usize,
    pub failures: Vec<ImportFailure>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportFailure {
    pub path: String,
    pub reason: String,
}

impl ImportSummary {
    fn fail(&mut self, path: &Path, reason: String) {
        self.failures.push(ImportFailure {
            path: path.to_string_lossy().into_owned(),
            reason,
        });
    }
}

/// Plain text as a document: a blank line starts a paragraph, and every other
/// line break is kept as one.
pub fn text_to_doc(text: &str) -> Value {
    let mut blocks = vec![];
    for para in text.replace("\r\n", "\n").split("\n\n") {
        let lines: Vec<&str> = para
            .split('\n')
            .skip_while(|l| l.trim().is_empty())
            .collect();
        let mut content = vec![];
        for (i, line) in lines.iter().enumerate() {
            if i > 0 {
                content.push(json!({"type": "hardBreak"}));
            }
            if !line.is_empty() {
                content.push(json!({"type": "text", "text": line}));
            }
        }
        while content.last().is_some_and(|n| n["type"] == "hardBreak") {
            content.pop();
        }
        if content.is_empty() {
            continue;
        }
        blocks.push(json!({"type": "paragraph", "content": content}));
    }
    if blocks.is_empty() {
        blocks.push(json!({"type": "paragraph"}));
    }
    json!({"type": "doc", "content": blocks})
}

/// Directory entries in a settled order, so importing the same tree twice
/// lays the notes down the same way round.
fn read_dir_sorted(dir: &Path) -> Result<Vec<std::fs::DirEntry>> {
    let mut entries: Vec<std::fs::DirEntry> =
        std::fs::read_dir(dir)?.collect::<std::result::Result<Vec<_>, _>>()?;
    entries.sort_by_key(|e| e.file_name());
    Ok(entries)
}

/// Enough of a MIME table to label what an import actually carries. The core
/// has no business pulling in a MIME database for this.
fn mime_for(name: &str) -> &'static str {
    let ext = Path::new(name)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    match ext.as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "bmp" => "image/bmp",
        "avif" => "image/avif",
        "heic" | "heif" => "image/heic",
        "svg" => "image/svg+xml",
        "pdf" => "application/pdf",
        "txt" => "text/plain",
        "md" | "markdown" => "text/markdown",
        "html" | "htm" => "text/html",
        "xml" => "application/xml",
        "json" => "application/json",
        "zip" => "application/zip",
        "m4a" => "audio/mp4",
        "mp3" => "audio/mpeg",
        "mp4" => "video/mp4",
        _ => "application/octet-stream",
    }
}

fn is_image(mime: &str) -> bool {
    mime.starts_with("image/")
}

/// Keeps a file that could not be converted. It becomes a note that says so
/// and carries the original, which is the whole of the promise: an import
/// never leaves anything behind.
fn attach_unconverted(
    store: &Store,
    path: &Path,
    folder: Option<&str>,
    summary: &mut ImportSummary,
) -> Result<()> {
    let bytes = std::fs::read(path)?;
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "file".into());
    let title = path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| name.clone());
    let doc = json!({"type": "doc", "content": [{
        "type": "paragraph",
        "content": [{
            "type": "text",
            "text": format!(
                "Snot could not read {name}, so the original file is kept \
                 with this note in the library's attachments folder."
            ),
        }],
    }]});
    let note = store.create_note(folder, None)?;
    let id = note.summary.id;
    store.put_attachment(Some(&id), &name, mime_for(&name), &bytes)?;
    store.update_note(
        &id,
        NotePatch {
            title: Some(title),
            doc: Some(doc),
            ..Default::default()
        },
    )?;
    summary.notes += 1;
    summary.attachments += 1;
    summary.unconverted += 1;
    Ok(())
}

/// Brings a PDF in as a page to write on, the way the app's own PDF import
/// does: the document becomes the note's background and the text layer starts
/// empty on top of it.
fn import_pdf_file(
    store: &Store,
    path: &Path,
    folder: Option<&str>,
    summary: &mut ImportSummary,
) -> Result<()> {
    let bytes = std::fs::read(path)?;
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "document.pdf".into());
    let title = path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| name.clone());
    let note = store.create_note(folder, None)?;
    let id = note.summary.id;
    let (attachment, _) = store.put_attachment(Some(&id), &name, "application/pdf", &bytes)?;
    store.update_note(
        &id,
        NotePatch {
            title: Some(title),
            background: Some(json!({
                "kind": "pdf",
                "attachmentId": attachment.id,
                "name": name,
            })),
            ..Default::default()
        },
    )?;
    summary.notes += 1;
    summary.attachments += 1;
    Ok(())
}
