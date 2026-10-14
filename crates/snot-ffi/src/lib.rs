//! snot-core as seen from Kotlin.
//!
//! The surface is deliberately coarse: a library object, an editor object
//! for the open note, and plain records for everything a screen draws.
//! Text offsets are counted in Unicode scalar values, as in the core; the
//! Kotlin side converts to and from UTF-16 at its edge. Bulk float data —
//! stroke input and ink outlines — crosses as little-endian `f32` bytes,
//! which Kotlin reads without boxing every number.

use std::sync::{Arc, Mutex, MutexGuard};

use snot_core::doc::Content;
use snot_core::editor::{Caret, EditSession, Insert};
use snot_core::ink::{self, Paper, Point, Stroke, Tool};
use snot_core::{Library, Mark, ParaStyle, Scope, SortBy, color, markdown};

uniffi::setup_scaffolding!();

#[derive(Debug, thiserror::Error, uniffi::Error)]
#[uniffi(flat_error)]
pub enum SnotError {
    #[error("{0}")]
    NotFound(String),
    #[error("{0}")]
    Failed(String),
}

impl From<snot_core::Error> for SnotError {
    fn from(e: snot_core::Error) -> Self {
        match e {
            snot_core::Error::NotFound(what) => SnotError::NotFound(what),
            other => SnotError::Failed(other.to_string()),
        }
    }
}

type Res<T> = Result<T, SnotError>;

// ---- records ---------------------------------------------------------------

#[derive(uniffi::Record)]
pub struct FolderInfo {
    pub id: String,
    pub parent_id: Option<String>,
    pub name: String,
    pub color: Option<String>,
    pub note_count: i64,
}

#[derive(uniffi::Record)]
pub struct CountsInfo {
    pub all: i64,
    pub unfiled: i64,
    pub favorites: i64,
    pub trash: i64,
}

#[derive(uniffi::Record)]
pub struct NoteInfo {
    pub id: String,
    pub folder_id: Option<String>,
    pub title: String,
    pub preview: String,
    pub color: Option<String>,
    pub pinned: bool,
    pub favorite: bool,
    pub has_ink: bool,
    pub has_image: bool,
    pub created_at: i64,
    pub updated_at: i64,
    pub trashed_at: Option<i64>,
    /// Search hits wrapped in U+0002 … U+0003.
    pub snippet: Option<String>,
}

impl From<snot_core::NoteSummary> for NoteInfo {
    fn from(n: snot_core::NoteSummary) -> Self {
        NoteInfo {
            id: n.id,
            folder_id: n.folder_id,
            title: n.title,
            preview: n.preview,
            color: n.color,
            pinned: n.pinned,
            favorite: n.favorite,
            has_ink: n.has_ink,
            has_image: n.has_image,
            created_at: n.created_at,
            updated_at: n.updated_at,
            trashed_at: n.trashed_at,
            snippet: n.snippet,
        }
    }
}

#[derive(uniffi::Enum, Clone)]
pub enum ScopeArg {
    All,
    Folder { id: String },
    Unfiled,
    Favorites,
    Trash,
}

#[derive(uniffi::Enum, Clone, Copy)]
pub enum SortArg {
    Modified,
    Created,
    Title,
}

#[derive(uniffi::Enum, Clone, Copy, PartialEq, Eq)]
pub enum StyleArg {
    Body,
    Heading1,
    Heading2,
    Heading3,
    Bullet,
    Numbered,
    Checklist,
    Quote,
    Code,
}

impl From<ParaStyle> for StyleArg {
    fn from(s: ParaStyle) -> Self {
        match s {
            ParaStyle::Body => StyleArg::Body,
            ParaStyle::Heading1 => StyleArg::Heading1,
            ParaStyle::Heading2 => StyleArg::Heading2,
            ParaStyle::Heading3 => StyleArg::Heading3,
            ParaStyle::Bullet => StyleArg::Bullet,
            ParaStyle::Numbered => StyleArg::Numbered,
            ParaStyle::Checklist => StyleArg::Checklist,
            ParaStyle::Quote => StyleArg::Quote,
            ParaStyle::Code => StyleArg::Code,
        }
    }
}

impl From<StyleArg> for ParaStyle {
    fn from(s: StyleArg) -> Self {
        match s {
            StyleArg::Body => ParaStyle::Body,
            StyleArg::Heading1 => ParaStyle::Heading1,
            StyleArg::Heading2 => ParaStyle::Heading2,
            StyleArg::Heading3 => ParaStyle::Heading3,
            StyleArg::Bullet => ParaStyle::Bullet,
            StyleArg::Numbered => ParaStyle::Numbered,
            StyleArg::Checklist => ParaStyle::Checklist,
            StyleArg::Quote => ParaStyle::Quote,
            StyleArg::Code => ParaStyle::Code,
        }
    }
}

#[derive(uniffi::Enum, Clone, PartialEq, Eq)]
pub enum MarkArg {
    Bold,
    Italic,
    Underline,
    Strike,
    Highlight { color: String },
    Color { color: String },
}

impl From<MarkArg> for Mark {
    fn from(m: MarkArg) -> Self {
        match m {
            MarkArg::Bold => Mark::Bold,
            MarkArg::Italic => Mark::Italic,
            MarkArg::Underline => Mark::Underline,
            MarkArg::Strike => Mark::Strike,
            MarkArg::Highlight { color } => Mark::Highlight(color),
            MarkArg::Color { color } => Mark::Color(color),
        }
    }
}

impl From<Mark> for MarkArg {
    fn from(m: Mark) -> Self {
        match m {
            Mark::Bold => MarkArg::Bold,
            Mark::Italic => MarkArg::Italic,
            Mark::Underline => MarkArg::Underline,
            Mark::Strike => MarkArg::Strike,
            Mark::Highlight(color) => MarkArg::Highlight { color },
            Mark::Color(color) => MarkArg::Color { color },
        }
    }
}

#[derive(uniffi::Enum, Clone, Copy, PartialEq, Eq)]
pub enum ToolArg {
    Pen,
    Marker,
    Highlighter,
}

impl From<ToolArg> for Tool {
    fn from(t: ToolArg) -> Self {
        match t {
            ToolArg::Pen => Tool::Pen,
            ToolArg::Marker => Tool::Marker,
            ToolArg::Highlighter => Tool::Highlighter,
        }
    }
}

impl From<Tool> for ToolArg {
    fn from(t: Tool) -> Self {
        match t {
            Tool::Pen => ToolArg::Pen,
            Tool::Marker => ToolArg::Marker,
            Tool::Highlighter => ToolArg::Highlighter,
        }
    }
}

#[derive(uniffi::Enum, Clone, Copy, PartialEq, Eq)]
pub enum PaperArg {
    Plain,
    Lined,
    Grid,
    Dotted,
}

impl From<PaperArg> for Paper {
    fn from(p: PaperArg) -> Self {
        match p {
            PaperArg::Plain => Paper::Plain,
            PaperArg::Lined => Paper::Lined,
            PaperArg::Grid => Paper::Grid,
            PaperArg::Dotted => Paper::Dotted,
        }
    }
}

impl From<Paper> for PaperArg {
    fn from(p: Paper) -> Self {
        match p {
            Paper::Plain => PaperArg::Plain,
            Paper::Lined => PaperArg::Lined,
            Paper::Grid => PaperArg::Grid,
            Paper::Dotted => PaperArg::Dotted,
        }
    }
}

#[derive(uniffi::Record)]
pub struct RunView {
    pub start: i32,
    pub end: i32,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub strike: bool,
    pub highlight: Option<String>,
    pub color: Option<String>,
}

#[derive(uniffi::Enum)]
pub enum BlockKind {
    Text {
        style: StyleArg,
        indent: i32,
        checked: bool,
        text: String,
        runs: Vec<RunView>,
        /// The label of a numbered-list item.
        number: Option<i32>,
    },
    Divider,
    Image {
        attachment: String,
        aspect: f32,
        caption: String,
    },
    Ink {
        height: f32,
        paper: PaperArg,
        strokes: i32,
    },
}

#[derive(uniffi::Record)]
pub struct BlockView {
    pub id: String,
    /// Changes whenever the block does; equal revisions mean nothing to
    /// redraw.
    pub revision: i64,
    pub kind: BlockKind,
}

#[derive(uniffi::Record)]
pub struct CaretInfo {
    pub block: String,
    pub offset: i32,
}

impl From<Caret> for CaretInfo {
    fn from(c: Caret) -> Self {
        CaretInfo {
            block: c.block,
            offset: c.offset as i32,
        }
    }
}

#[derive(uniffi::Record)]
pub struct InkPath {
    /// `0xAARRGGBB`, alpha included.
    pub argb: u32,
    pub tool: ToolArg,
    /// Closed polygon as little-endian `f32` x, y pairs in ink units.
    pub outline: Vec<u8>,
}

#[derive(uniffi::Record)]
pub struct ImageInfo {
    pub attachment: String,
    pub aspect: f32,
}

// ---- library ---------------------------------------------------------------

#[derive(uniffi::Object)]
pub struct SnotLibrary {
    lib: Library,
}

#[uniffi::export]
impl SnotLibrary {
    #[uniffi::constructor]
    pub fn open(path: String) -> Res<Arc<Self>> {
        Ok(Arc::new(SnotLibrary {
            lib: Library::open(path)?,
        }))
    }

    pub fn folders(&self) -> Res<Vec<FolderInfo>> {
        Ok(self
            .lib
            .folders()?
            .into_iter()
            .map(|f| FolderInfo {
                id: f.id,
                parent_id: f.parent_id,
                name: f.name,
                color: f.color,
                note_count: f.note_count,
            })
            .collect())
    }

    pub fn counts(&self) -> Res<CountsInfo> {
        let c = self.lib.counts()?;
        Ok(CountsInfo {
            all: c.all,
            unfiled: c.unfiled,
            favorites: c.favorites,
            trash: c.trash,
        })
    }

    pub fn notes(&self, scope: ScopeArg, sort: SortArg) -> Res<Vec<NoteInfo>> {
        let scope = match scope {
            ScopeArg::All => Scope::All,
            ScopeArg::Folder { id } => Scope::Folder(id),
            ScopeArg::Unfiled => Scope::Unfiled,
            ScopeArg::Favorites => Scope::Favorites,
            ScopeArg::Trash => Scope::Trash,
        };
        let sort = match sort {
            SortArg::Modified => SortBy::Modified,
            SortArg::Created => SortBy::Created,
            SortArg::Title => SortBy::Title,
        };
        Ok(self
            .lib
            .notes(&scope, sort)?
            .into_iter()
            .map(Into::into)
            .collect())
    }

    pub fn search(&self, query: String) -> Res<Vec<NoteInfo>> {
        Ok(self
            .lib
            .search(&query)?
            .into_iter()
            .map(Into::into)
            .collect())
    }

    /// Creates a note and returns its id. `handwriting` starts it on a
    /// blank drawing rather than a line of text.
    pub fn create_note(&self, folder_id: Option<String>, handwriting: bool) -> Res<String> {
        let note = if handwriting {
            self.lib.create_ink_note(folder_id.as_deref())?
        } else {
            self.lib.create_note(folder_id.as_deref())?
        };
        Ok(note.id)
    }

    pub fn open_note(&self, id: String) -> Res<Arc<NoteEditor>> {
        let note = self.lib.note(&id)?;
        Ok(Arc::new(NoteEditor {
            color: note.color.clone(),
            trashed: note.trashed_at.is_some(),
            session: Mutex::new(EditSession::new(note)),
        }))
    }

    /// Saves an editor's changes, if it has any. Returns whether anything
    /// was written.
    pub fn save(&self, editor: Arc<NoteEditor>) -> Res<bool> {
        let mut s = editor.lock();
        if !s.is_dirty() {
            return Ok(false);
        }
        let written = self
            .lib
            .save_content(&s.note().id, s.title(), s.doc())?
            .is_some();
        s.mark_saved();
        Ok(written)
    }

    pub fn set_pinned(&self, id: String, on: bool) -> Res<()> {
        Ok(self.lib.set_pinned(&id, on)?)
    }

    pub fn set_favorite(&self, id: String, on: bool) -> Res<()> {
        Ok(self.lib.set_favorite(&id, on)?)
    }

    pub fn set_note_color(&self, id: String, color: Option<String>) -> Res<()> {
        Ok(self.lib.set_note_color(&id, color.as_deref())?)
    }

    pub fn move_note(&self, id: String, folder_id: Option<String>) -> Res<()> {
        Ok(self.lib.move_note(&id, folder_id.as_deref())?)
    }

    pub fn trash_note(&self, id: String) -> Res<()> {
        Ok(self.lib.trash_note(&id)?)
    }

    pub fn restore_note(&self, id: String) -> Res<()> {
        Ok(self.lib.restore_note(&id)?)
    }

    pub fn delete_note(&self, id: String) -> Res<()> {
        Ok(self.lib.delete_note(&id)?)
    }

    pub fn empty_trash(&self) -> Res<i64> {
        Ok(self.lib.empty_trash()? as i64)
    }

    pub fn duplicate_note(&self, id: String) -> Res<String> {
        Ok(self.lib.duplicate_note(&id)?.id)
    }

    pub fn create_folder(&self, name: String, parent_id: Option<String>) -> Res<String> {
        Ok(self.lib.create_folder(&name, parent_id.as_deref())?.id)
    }

    pub fn rename_folder(&self, id: String, name: String) -> Res<()> {
        Ok(self.lib.rename_folder(&id, &name)?)
    }

    pub fn set_folder_color(&self, id: String, color: Option<String>) -> Res<()> {
        Ok(self.lib.set_folder_color(&id, color.as_deref())?)
    }

    pub fn delete_folder(&self, id: String) -> Res<()> {
        Ok(self.lib.delete_folder(&id)?)
    }

    /// Stores a picture. Fails for anything that is not PNG, JPEG, GIF or
    /// WebP.
    pub fn add_image(&self, bytes: Vec<u8>, name: String) -> Res<ImageInfo> {
        let mime = markdown::sniff_image(&bytes)
            .ok_or_else(|| SnotError::Failed("That file is not a picture Snot can show.".into()))?;
        let att = self.lib.add_attachment(&bytes, mime, &name)?;
        Ok(ImageInfo {
            attachment: att.id,
            aspect: markdown::image_aspect(&bytes).unwrap_or(0.75),
        })
    }

    pub fn attachment_path(&self, id: String) -> String {
        self.lib.attachment_path(&id).to_string_lossy().into_owned()
    }

    /// Exports a note as Markdown into `dir`; returns the file written.
    pub fn export_markdown(&self, id: String, dir: String) -> Res<String> {
        Ok(self
            .lib
            .export_markdown(&id, dir.as_ref())?
            .to_string_lossy()
            .into_owned())
    }

    /// The note as Markdown text alone, for sharing to another app.
    pub fn markdown_text(&self, id: String) -> Res<String> {
        let note = self.lib.note(&id)?;
        Ok(markdown::export(&note, &self.lib, "assets").markdown)
    }

    /// Imports Markdown text as a new note; returns its id.
    pub fn import_markdown_text(
        &self,
        text: String,
        fallback_title: String,
        folder_id: Option<String>,
    ) -> Res<String> {
        let (mut title, doc) = markdown::import(&text, None, Some(&self.lib));
        if title.is_empty() && doc.plain_text().is_empty() {
            title = fallback_title;
        }
        let note = self.lib.create_note(folder_id.as_deref())?;
        self.lib.save_content(&note.id, &title, &doc)?;
        Ok(note.id)
    }
}

// ---- editor ----------------------------------------------------------------

#[derive(uniffi::Object)]
pub struct NoteEditor {
    session: Mutex<EditSession>,
    color: Option<String>,
    trashed: bool,
}

impl NoteEditor {
    fn lock(&self) -> MutexGuard<'_, EditSession> {
        self.session.lock().unwrap_or_else(|e| e.into_inner())
    }
}

fn marks(m: Option<Vec<MarkArg>>) -> Option<Vec<Mark>> {
    m.map(|v| v.into_iter().map(Into::into).collect())
}

fn range(start: i32, end: i32) -> std::ops::Range<u32> {
    let s = start.max(0) as u32;
    s..(end.max(0) as u32).max(s)
}

fn view(s: &EditSession, index: usize, numbers: &[Option<u32>]) -> BlockView {
    let block = &s.doc().blocks[index];
    let kind = match &block.content {
        Content::Text(t) => BlockKind::Text {
            style: t.style.into(),
            indent: t.indent as i32,
            checked: t.checked,
            text: t.text.clone(),
            runs: t
                .runs()
                .into_iter()
                .map(|r| RunView {
                    start: r.start as i32,
                    end: r.end as i32,
                    bold: r.bold,
                    italic: r.italic,
                    underline: r.underline,
                    strike: r.strike,
                    highlight: r.highlight,
                    color: r.color,
                })
                .collect(),
            number: numbers.get(index).copied().flatten().map(|n| n as i32),
        },
        Content::Divider => BlockKind::Divider,
        Content::Image(img) => BlockKind::Image {
            attachment: img.attachment.clone(),
            aspect: img.aspect,
            caption: img.caption.clone(),
        },
        Content::Ink(ink) => BlockKind::Ink {
            height: ink.height,
            paper: ink.paper.into(),
            strokes: ink.strokes.len() as i32,
        },
    };
    BlockView {
        id: block.id.clone(),
        revision: s.revision(&block.id) as i64,
        kind,
    }
}

fn pack(poly: &[[f32; 2]]) -> Vec<u8> {
    let mut out = Vec::with_capacity(poly.len() * 8);
    for [x, y] in poly {
        out.extend_from_slice(&x.to_le_bytes());
        out.extend_from_slice(&y.to_le_bytes());
    }
    out
}

/// Reads `x, y, pressure` triples of little-endian `f32`.
fn unpack_points(bytes: &[u8]) -> Vec<Point> {
    bytes
        .as_chunks::<12>()
        .0
        .iter()
        .map(|c| {
            let f = |i: usize| f32::from_le_bytes([c[i], c[i + 1], c[i + 2], c[i + 3]]);
            Point {
                x: f(0),
                y: f(4),
                pressure: f(8),
            }
        })
        .collect()
}

#[uniffi::export]
impl NoteEditor {
    pub fn note_id(&self) -> String {
        self.lock().note().id.clone()
    }

    /// The note's page colour token when it was opened.
    pub fn color(&self) -> Option<String> {
        self.color.clone()
    }

    pub fn is_trashed(&self) -> bool {
        self.trashed
    }

    pub fn title(&self) -> String {
        self.lock().title().to_string()
    }

    pub fn set_title(&self, title: String) {
        self.lock().set_title(&title)
    }

    pub fn blocks(&self) -> Vec<BlockView> {
        let s = self.lock();
        let numbers = s.doc().list_numbers();
        (0..s.doc().blocks.len())
            .map(|i| view(&s, i, &numbers))
            .collect()
    }

    pub fn is_dirty(&self) -> bool {
        self.lock().is_dirty()
    }

    pub fn can_undo(&self) -> bool {
        self.lock().can_undo()
    }

    pub fn can_redo(&self) -> bool {
        self.lock().can_redo()
    }

    pub fn undo(&self) -> Option<CaretInfo> {
        self.lock().undo().map(Into::into)
    }

    pub fn redo(&self) -> Option<CaretInfo> {
        self.lock().redo().map(Into::into)
    }

    pub fn end_gesture(&self) {
        self.lock().end_gesture()
    }

    pub fn set_text(
        &self,
        block: String,
        text: String,
        marks_override: Option<Vec<MarkArg>>,
    ) -> bool {
        self.lock()
            .set_text(&block, &text, marks(marks_override).as_deref())
    }

    pub fn replace(
        &self,
        block: String,
        start: i32,
        end: i32,
        text: String,
        marks_override: Option<Vec<MarkArg>>,
    ) -> bool {
        self.lock().replace(
            &block,
            range(start, end),
            &text,
            marks(marks_override).as_deref(),
        )
    }

    pub fn paste(&self, block: String, start: i32, end: i32, text: String) -> Option<CaretInfo> {
        self.lock()
            .paste(&block, range(start, end), &text)
            .map(Into::into)
    }

    pub fn split(&self, block: String, at: i32) -> Option<CaretInfo> {
        self.lock().split(&block, at.max(0) as u32).map(Into::into)
    }

    pub fn backspace_at_start(&self, block: String) -> Option<CaretInfo> {
        self.lock().backspace_at_start(&block).map(Into::into)
    }

    pub fn delete_at_end(&self, block: String) -> Option<CaretInfo> {
        self.lock().delete_at_end(&block).map(Into::into)
    }

    pub fn set_style(&self, block: String, style: StyleArg) {
        self.lock().set_style(&block, style.into())
    }

    pub fn toggle_checked(&self, block: String) {
        self.lock().toggle_checked(&block)
    }

    pub fn indent(&self, block: String, delta: i32) {
        self.lock().indent(&block, delta)
    }

    pub fn toggle_mark(&self, block: String, start: i32, end: i32, mark: MarkArg) {
        self.lock()
            .toggle_mark(&block, range(start, end), &mark.into())
    }

    pub fn clear_marks(&self, block: String, start: i32, end: i32) {
        self.lock().clear_marks(&block, range(start, end))
    }

    pub fn marks_in(&self, block: String, start: i32, end: i32) -> Vec<MarkArg> {
        self.lock()
            .marks_in(&block, range(start, end))
            .into_iter()
            .map(Into::into)
            .collect()
    }

    pub fn insert_divider(&self, after: Option<String>) -> String {
        self.lock().insert(after.as_deref(), Insert::Divider)
    }

    pub fn insert_ink(&self, after: Option<String>) -> String {
        self.lock().insert(after.as_deref(), Insert::Ink)
    }

    pub fn insert_image(&self, after: Option<String>, attachment: String, aspect: f32) -> String {
        self.lock()
            .insert(after.as_deref(), Insert::Image { attachment, aspect })
    }

    pub fn set_caption(&self, block: String, caption: String) {
        self.lock().set_caption(&block, &caption)
    }

    pub fn delete_block(&self, block: String) -> Option<CaretInfo> {
        self.lock().delete_block(&block).map(Into::into)
    }

    pub fn move_block(&self, block: String, delta: i32) {
        self.lock().move_block(&block, delta)
    }

    /// Adds a stroke from `x, y, pressure` triples of little-endian `f32`
    /// in ink units.
    pub fn add_stroke(
        &self,
        block: String,
        tool: ToolArg,
        color: String,
        width: f32,
        points: Vec<u8>,
    ) -> bool {
        let pts = unpack_points(&points);
        self.lock()
            .add_stroke(&block, Stroke::new(tool.into(), color, width, &pts))
    }

    pub fn erase(&self, block: String, x: f32, y: f32, radius: f32) -> i32 {
        self.lock().erase(&block, x, y, radius) as i32
    }

    pub fn set_paper(&self, block: String, paper: PaperArg) {
        self.lock().set_paper(&block, paper.into())
    }

    pub fn clear_ink(&self, block: String) {
        self.lock().clear_ink(&block)
    }

    pub fn extend_ink(&self, block: String, by: f32) {
        self.lock().extend_ink(&block, by)
    }

    /// Every stroke of an ink block as a filled outline, highlighter first
    /// so it paints underneath.
    pub fn ink_paths(&self, block: String, dark: bool) -> Vec<InkPath> {
        let s = self.lock();
        let Some(ink) = s.ink(&block) else {
            return Vec::new();
        };
        let mut strokes: Vec<&Stroke> = ink
            .strokes
            .iter()
            .filter(|s| s.tool == Tool::Highlighter)
            .collect();
        strokes.extend(ink.strokes.iter().filter(|s| s.tool != Tool::Highlighter));
        strokes
            .into_iter()
            .map(|st| InkPath {
                argb: with_alpha(color::ink(&st.color, dark), st.tool.alpha()),
                tool: st.tool.into(),
                outline: pack(&st.outline()),
            })
            .collect()
    }
}

fn with_alpha(argb: u32, alpha: f32) -> u32 {
    let a = ((argb >> 24) as f32 * alpha).round() as u32;
    (a << 24) | (argb & 0xff_ffff)
}

// ---- free functions --------------------------------------------------------

/// The outline of a stroke still being drawn, from `x, y, pressure` triples
/// of little-endian `f32`; returns packed `x, y` pairs.
#[uniffi::export]
pub fn stroke_outline(points: Vec<u8>, tool: ToolArg, width: f32) -> Vec<u8> {
    pack(&ink::outline(&unpack_points(&points), tool.into(), width))
}

#[uniffi::export]
pub fn ink_color(token: String, dark: bool) -> u32 {
    color::ink(&token, dark)
}

#[uniffi::export]
pub fn highlight_color(token: String, dark: bool) -> u32 {
    color::highlight(&token, dark)
}

#[uniffi::export]
pub fn note_background(token: Option<String>, dark: bool) -> u32 {
    color::note_background(token.as_deref(), dark)
}

#[uniffi::export]
pub fn ink_colors() -> Vec<String> {
    color::TOKENS.iter().map(|s| s.to_string()).collect()
}

#[uniffi::export]
pub fn highlight_colors() -> Vec<String> {
    color::HIGHLIGHTS.iter().map(|s| s.to_string()).collect()
}

#[uniffi::export]
pub fn note_colors() -> Vec<String> {
    color::NOTE_COLORS.iter().map(|s| s.to_string()).collect()
}

#[uniffi::export]
pub fn ink_width() -> f32 {
    ink::INK_WIDTH
}

#[uniffi::export]
pub fn paper_spacing() -> f32 {
    ink::PAPER_SPACING
}
