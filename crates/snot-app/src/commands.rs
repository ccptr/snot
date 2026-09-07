use std::path::PathBuf;

use serde::Serialize;
use serde_json::Value;
use snot_core::{Attachment, Folder, Note, NotePatch, NoteSummary, Scope, SortBy, Tag};
use tauri::State;

use crate::AppState;

/// Commands hand the webview a plain string on failure; the UI turns it into
/// a toast. Nothing here is worth a structured error code yet.
pub struct CommandError(String);

impl<E: std::fmt::Display> From<E> for CommandError {
    fn from(err: E) -> Self {
        CommandError(err.to_string())
    }
}

impl Serialize for CommandError {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.0)
    }
}

type Res<T> = Result<T, CommandError>;

/// Runs `f` against the library, holding the lock only for the call itself.
macro_rules! with_store {
    ($state:expr, |$store:ident| $body:expr) => {{
        let guard = $state.store.lock().map_err(|_| "library lock poisoned")?;
        let $store = &*guard;
        $body
    }};
}

#[tauri::command]
pub fn library_root(state: State<'_, AppState>) -> Res<String> {
    with_store!(state, |s| Ok(s.root().to_string_lossy().into_owned()))
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Stats {
    pub notes: i64,
    pub trashed: i64,
    pub folders: i64,
}

#[tauri::command]
pub fn stats(state: State<'_, AppState>) -> Res<Stats> {
    with_store!(state, |s| {
        let (notes, trashed, folders) = s.stats()?;
        Ok(Stats { notes, trashed, folders })
    })
}

// ---------------------------------------------------------------- folders

#[tauri::command]
pub fn list_folders(state: State<'_, AppState>) -> Res<Vec<Folder>> {
    with_store!(state, |s| Ok(s.list_folders()?))
}

#[tauri::command]
pub fn create_folder(
    state: State<'_, AppState>,
    name: String,
    parent_id: Option<String>,
) -> Res<Folder> {
    with_store!(state, |s| Ok(s.create_folder(&name, parent_id.as_deref())?))
}

#[tauri::command]
pub fn rename_folder(state: State<'_, AppState>, id: String, name: String) -> Res<()> {
    with_store!(state, |s| Ok(s.rename_folder(&id, &name)?))
}

#[tauri::command]
pub fn set_folder_color(
    state: State<'_, AppState>,
    id: String,
    color: Option<String>,
) -> Res<()> {
    with_store!(state, |s| Ok(s.set_folder_color(&id, color.as_deref())?))
}

#[tauri::command]
pub fn move_folder(
    state: State<'_, AppState>,
    id: String,
    parent_id: Option<String>,
) -> Res<()> {
    with_store!(state, |s| Ok(s.move_folder(&id, parent_id.as_deref())?))
}

#[tauri::command]
pub fn delete_folder(state: State<'_, AppState>, id: String) -> Res<()> {
    with_store!(state, |s| Ok(s.delete_folder(&id)?))
}

// ------------------------------------------------------------------ notes

#[tauri::command]
pub fn list_notes(
    state: State<'_, AppState>,
    scope: Scope,
    sort: SortBy,
) -> Res<Vec<NoteSummary>> {
    with_store!(state, |s| Ok(s.list_notes(&scope, sort)?))
}

#[tauri::command]
pub fn search_notes(
    state: State<'_, AppState>,
    query: String,
    scope: Scope,
) -> Res<Vec<NoteSummary>> {
    with_store!(state, |s| Ok(s.search(&query, &scope)?))
}

#[tauri::command]
pub fn get_note(state: State<'_, AppState>, id: String) -> Res<Note> {
    with_store!(state, |s| Ok(s.get_note(&id)?))
}

#[tauri::command]
pub fn create_note(
    state: State<'_, AppState>,
    folder_id: Option<String>,
    doc: Option<Value>,
) -> Res<Note> {
    with_store!(state, |s| Ok(s.create_note(folder_id.as_deref(), doc)?))
}

#[tauri::command]
pub fn update_note(state: State<'_, AppState>, id: String, patch: NotePatch) -> Res<Note> {
    with_store!(state, |s| Ok(s.update_note(&id, patch)?))
}

#[tauri::command]
pub fn trash_note(state: State<'_, AppState>, id: String) -> Res<()> {
    with_store!(state, |s| Ok(s.trash_note(&id)?))
}

#[tauri::command]
pub fn restore_note(state: State<'_, AppState>, id: String) -> Res<()> {
    with_store!(state, |s| Ok(s.restore_note(&id)?))
}

#[tauri::command]
pub fn purge_note(state: State<'_, AppState>, id: String) -> Res<()> {
    with_store!(state, |s| Ok(s.purge_note(&id)?))
}

#[tauri::command]
pub fn empty_trash(state: State<'_, AppState>) -> Res<usize> {
    with_store!(state, |s| Ok(s.empty_trash()?))
}

// ------------------------------------------------------------------- tags

#[tauri::command]
pub fn list_tags(state: State<'_, AppState>) -> Res<Vec<Tag>> {
    with_store!(state, |s| Ok(s.list_tags()?))
}

#[tauri::command]
pub fn ensure_tag(state: State<'_, AppState>, name: String) -> Res<Tag> {
    with_store!(state, |s| Ok(s.ensure_tag(&name)?))
}

#[tauri::command]
pub fn delete_tag(state: State<'_, AppState>, id: String) -> Res<()> {
    with_store!(state, |s| Ok(s.delete_tag(&id)?))
}

// ------------------------------------------------------------ attachments

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StoredAttachment {
    #[serde(flatten)]
    pub attachment: Attachment,
    /// Absolute path; the UI turns it into a webview URL with `convertFileSrc`.
    pub path: String,
}

#[tauri::command]
pub fn put_attachment(
    state: State<'_, AppState>,
    note_id: Option<String>,
    name: String,
    mime: Option<String>,
    bytes: Vec<u8>,
) -> Res<StoredAttachment> {
    let mime = mime.unwrap_or_else(|| {
        mime_guess::from_path(&name).first_or_octet_stream().essence_str().to_string()
    });
    with_store!(state, |s| {
        let (attachment, path) = s.put_attachment(note_id.as_deref(), &name, &mime, &bytes)?;
        Ok(StoredAttachment { attachment, path: path.to_string_lossy().into_owned() })
    })
}

#[tauri::command]
pub fn attachment_path(state: State<'_, AppState>, id: String) -> Res<String> {
    with_store!(state, |s| Ok(s.attachment_path(&id)?.to_string_lossy().into_owned()))
}

// ----------------------------------------------------------------- export

#[tauri::command]
pub fn export_markdown(state: State<'_, AppState>, id: String) -> Res<String> {
    with_store!(state, |s| {
        let note = s.get_note(&id)?;
        let mut out = String::new();
        if !note.summary.title.trim().is_empty() {
            out.push_str("# ");
            out.push_str(&note.summary.title);
            out.push_str("\n\n");
        }
        out.push_str(&snot_core::doc_to_markdown(&note.doc));
        Ok(out)
    })
}

#[tauri::command]
pub fn write_text_file(path: String, contents: String) -> Res<()> {
    let path = PathBuf::from(path);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, contents)?;
    Ok(())
}
