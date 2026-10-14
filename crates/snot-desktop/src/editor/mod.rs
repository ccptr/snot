//! The note editor: one open note, its caret, and the drawing tools.
//!
//! Document changes all go through [`EditSession`], so Enter, Backspace,
//! lists and undo behave exactly as on Android. This module owns what is
//! particular to a desktop: the caret and selection, keyboard commands,
//! mouse hit-testing and the platform text-input protocol.

mod ink;
mod tiles;
mod view;

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::ops::Range;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use gpui::{
    App, AppContext, Bounds, ClipboardItem, Context, Entity, EntityInputHandler, EventEmitter,
    FocusHandle, Focusable, Pixels, Point, ScrollHandle, Subscription, Task, TextLayout,
    UTF16Selection, Window, actions, point, px,
};
use snot_core::editor::{Caret, EditSession, Insert};
use snot_core::{Library, Mark, Note, Paper, ParaStyle};
use unicode_segmentation::UnicodeSegmentation;

use crate::text_field::{FieldEvent, TextField};
use crate::theme::Theme;
use crate::utf16;

pub use ink::InkTool;

actions!(
    note_editor,
    [
        Backspace,
        Delete,
        Enter,
        Left,
        Right,
        Up,
        Down,
        SelectLeft,
        SelectRight,
        SelectUp,
        SelectDown,
        WordLeft,
        WordRight,
        SelectWordLeft,
        SelectWordRight,
        Home,
        End,
        SelectHome,
        SelectEnd,
        DocStart,
        DocEnd,
        SelectAll,
        Copy,
        Cut,
        Paste,
        Undo,
        Redo,
        Bold,
        Italic,
        Underline,
        Strike,
        Indent,
        Outdent,
        Heading1,
        Heading2,
        Heading3,
        BodyText,
        BulletList,
        NumberedList,
        Checklist,
        ToggleCheck,
        Escape,
        ZoomIn,
        ZoomOut,
        ZoomReset,
    ]
);

pub const CONTEXT: &str = "NoteEditor";

/// What the editor tells the rest of the app.
pub enum NoteEvent {
    /// The note was written to disk; lists showing it are stale.
    Saved,
    /// The note went to the trash; the editor should close.
    Closed,
    /// Another note should open in this one's place.
    Open(String),
}

/// A caret, or a selection when `anchor != head`, inside one text block.
/// Offsets are UTF-8 bytes into the block's text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Selection {
    pub block: String,
    pub anchor: usize,
    pub head: usize,
}

impl Selection {
    pub fn caret(block: impl Into<String>, at: usize) -> Self {
        Selection {
            block: block.into(),
            anchor: at,
            head: at,
        }
    }

    pub fn range(&self) -> Range<usize> {
        self.anchor.min(self.head)..self.anchor.max(self.head)
    }

    pub fn is_empty(&self) -> bool {
        self.anchor == self.head
    }
}

/// Layout from the last frame, used to hit-test the next event.
#[derive(Default)]
pub struct Geometry {
    pub text: HashMap<String, TextLayout>,
    /// Blocks whose text layout has been measured and painted, so asking
    /// it for positions will not panic.
    pub ready: HashSet<String>,
    /// Top left of the page, in window pixels.
    pub page: Point<Pixels>,
    /// Bottom of the last block, in window pixels.
    pub blocks_bottom: Pixels,
    /// Height of all the blocks together.
    pub blocks_height: Pixels,
    /// Top and bottom of each block, from the top of the scrolled content.
    pub rects: HashMap<String, (Pixels, Pixels)>,
    /// A block to bring into view on the next frame.
    pub reveal: Option<String>,
    /// A block brought into view this frame from where it was last laid
    /// out, to look at again once it has been laid out afresh.
    pub recheck: Option<String>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Popover {
    PenColor,
    Style,
    Insert,
    Highlight,
    TextColor,
    NoteColor,
    MoveTo,
    More,
}

pub struct NoteView {
    pub(crate) lib: Arc<Library>,
    pub(crate) session: EditSession,
    pub(crate) focus: FocusHandle,
    pub(crate) title: Entity<TextField>,
    pub(crate) sel: Option<Selection>,
    /// A picture, drawing or divider picked with the mouse, which Delete
    /// removes.
    pub(crate) picked: Option<String>,
    pub(crate) tool: InkTool,
    pub(crate) ink: ink::InkState,
    pending_marks: Option<Vec<Mark>>,
    marked: Option<Range<usize>>,
    pub(crate) geometry: Rc<RefCell<Geometry>>,
    /// The blocks, as a view of their own: see [`view::Blocks`].
    pub(crate) blocks: Option<Entity<view::Blocks>>,
    pub(crate) scroll: ScrollHandle,
    pub(crate) theme: Theme,
    pub(crate) popover: Option<(Popover, Point<Pixels>)>,
    pub(crate) color: Option<String>,
    pub(crate) favorite: bool,
    pub(crate) pinned: bool,
    pub(crate) folder: Option<String>,
    pub(crate) updated_at: i64,
    pub(crate) trashed: bool,
    goal_x: Option<Pixels>,
    selecting: bool,
    save_task: Option<Task<()>>,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<NoteEvent> for NoteView {}

impl Focusable for NoteView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

/// Start of the word at or before `at`.
pub fn word_start(text: &str, at: usize) -> usize {
    text.split_word_bound_indices()
        .rev()
        .find(|(i, w)| *i < at && w.chars().any(char::is_alphanumeric))
        .map(|(i, _)| i)
        .unwrap_or(0)
}

/// End of the word at or after `at`.
pub fn word_end(text: &str, at: usize) -> usize {
    text.split_word_bound_indices()
        .map(|(i, w)| (i + w.len(), w))
        .find(|(end, w)| *end > at && w.chars().any(char::is_alphanumeric))
        .map(|(end, _)| end)
        .unwrap_or(text.len())
}

fn prev_grapheme(text: &str, at: usize) -> usize {
    text.grapheme_indices(true)
        .rev()
        .find_map(|(i, _)| (i < at).then_some(i))
        .unwrap_or(0)
}

fn next_grapheme(text: &str, at: usize) -> usize {
    text.grapheme_indices(true)
        .find_map(|(i, _)| (i > at).then_some(i))
        .unwrap_or(text.len())
}

impl NoteView {
    pub fn new(
        lib: Arc<Library>,
        note: Note,
        theme: Theme,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let title = cx.new(|cx| {
            let mut f = TextField::new("Title", cx);
            f.set_text(&note.title, cx);
            f
        });
        let subs = vec![cx.subscribe_in(&title, window, Self::on_title_event)];
        let mut view = NoteView {
            color: note.color.clone(),
            favorite: note.favorite,
            pinned: note.pinned,
            folder: note.folder_id.clone(),
            updated_at: note.updated_at,
            trashed: note.trashed_at.is_some(),
            lib,
            session: EditSession::new(note),
            focus: cx.focus_handle(),
            title,
            sel: None,
            picked: None,
            tool: InkTool::Type,
            ink: ink::InkState::default(),
            pending_marks: None,
            marked: None,
            geometry: Rc::new(RefCell::new(Geometry::default())),
            blocks: None,
            scroll: ScrollHandle::new(),
            theme,
            popover: None,
            goal_x: None,
            selecting: false,
            save_task: None,
            _subscriptions: subs,
        };
        view.style_title_in(cx);
        view
    }

    pub fn note_id(&self) -> &str {
        &self.session.note().id
    }

    pub fn set_theme(&mut self, theme: Theme, cx: &mut Context<Self>) {
        self.theme = theme;
        self.style_title_in(cx);
        cx.notify();
    }

    fn style_title_in(&mut self, cx: &mut Context<Self>) {
        let t = self.theme;
        self.title.update(cx, |f, cx| {
            f.color = Some(t.text);
            f.caret_color = Some(t.caret);
            f.selection_color = Some(t.selection);
            f.placeholder_color = Some(t.text_faint);
            cx.notify();
        });
    }

    /// Puts the caret at the end of the body, or in the title of an empty
    /// note, ready to type.
    pub fn focus_initial(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.style_title_in(cx);
        let doc = self.session.doc();
        let empty = doc.plain_text().is_empty() && !doc.has_ink() && !doc.has_image();
        if empty && self.tool == InkTool::Type {
            window.focus(&self.title.focus_handle(cx), cx);
            return;
        }
        if let Some(b) = doc.blocks.iter().rev().find(|b| b.as_text().is_some()) {
            let len = b.as_text().map(|t| t.text.len()).unwrap_or(0);
            self.sel = Some(Selection::caret(b.id.clone(), len));
        }
        window.focus(&self.focus, cx);
    }

    fn on_title_event(
        &mut self,
        _: &Entity<TextField>,
        e: &FieldEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match e {
            FieldEvent::Changed(text) => {
                self.session.set_title(text);
                self.changed(cx);
            }
            FieldEvent::Submit | FieldEvent::Down => {
                let first = self
                    .session
                    .doc()
                    .blocks
                    .iter()
                    .find(|b| b.as_text().is_some())
                    .map(|b| b.id.clone());
                if let Some(id) = first {
                    self.sel = Some(Selection::caret(id, 0));
                    self.tool = InkTool::Type;
                    window.focus(&self.focus, cx);
                    cx.notify();
                }
            }
            FieldEvent::Cancel => window.focus(&self.focus, cx),
            FieldEvent::Up | FieldEvent::Blur => {}
        }
    }

    // ---- saving ----------------------------------------------------------

    /// Called after every change: redraw, and save once typing pauses.
    pub(crate) fn changed(&mut self, cx: &mut Context<Self>) {
        cx.notify();
        self.save_task = Some(cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(600))
                .await;
            let _ = this.update(cx, |this, cx| this.save(cx));
        }));
    }

    /// Writes pending changes now.
    pub fn save(&mut self, cx: &mut Context<Self>) {
        self.save_task = None;
        if !self.session.is_dirty() {
            return;
        }
        let note = self.session.note();
        match self.lib.save_content(&note.id, &note.title, &note.doc) {
            Ok(written) => {
                self.session.mark_saved();
                if let Some(at) = written {
                    self.updated_at = at;
                    cx.emit(NoteEvent::Saved);
                }
            }
            Err(e) => eprintln!("snot: could not save note: {e}"),
        }
    }

    // ---- helpers ---------------------------------------------------------

    fn text_of(&self, block: &str) -> Option<&str> {
        self.session.text(block).map(|t| t.text.as_str())
    }

    pub(crate) fn layout(&self, block: &str) -> Option<TextLayout> {
        let g = self.geometry.borrow();
        g.ready
            .contains(block)
            .then(|| g.text.get(block).cloned())
            .flatten()
    }

    fn text_blocks(&self) -> Vec<String> {
        self.session
            .doc()
            .blocks
            .iter()
            .filter(|b| b.as_text().is_some())
            .map(|b| b.id.clone())
            .collect()
    }

    fn set_caret_from(&mut self, caret: Option<Caret>) {
        if let Some(c) = caret
            && let Some(text) = self.text_of(&c.block)
        {
            let at = utf16::byte_at(text, c.offset);
            self.sel = Some(Selection::caret(c.block, at));
        }
        self.marked = None;
        self.goal_x = None;
        self.scroll_to_caret();
    }

    pub(crate) fn scroll_to_caret(&self) {
        if let Some(sel) = &self.sel {
            self.geometry.borrow_mut().reveal = Some(sel.block.clone());
        }
    }

    /// Marks for the next typed text: what the toolbar was set to while
    /// nothing was selected, else inherited by the core.
    fn take_pending(&mut self) -> Option<Vec<Mark>> {
        self.pending_marks.take()
    }

    /// Replaces the selection (or `range`) with `text`, which may hold
    /// line breaks.
    fn insert_text(&mut self, range: Option<Range<usize>>, text: &str, cx: &mut Context<Self>) {
        let Some(sel) = self.sel.clone() else { return };
        let Some(current) = self.text_of(&sel.block).map(str::to_string) else {
            return;
        };
        let range = range.or(self.marked.clone()).unwrap_or(sel.range());
        let range = range.start.min(current.len())..range.end.min(current.len());
        let chars = utf16::chars(&current, &range);
        if text.contains(['\n', '\r']) {
            let caret = self.session.paste(&sel.block, chars, text);
            self.set_caret_from(caret);
        } else {
            let marks = self.take_pending();
            self.session
                .replace(&sel.block, chars, text, marks.as_deref());
            let at = range.start + text.len();
            self.sel = Some(Selection::caret(sel.block, at));
        }
        self.marked = None;
        self.picked = None;
        self.goal_x = None;
        self.changed(cx);
    }

    fn delete_range(&mut self, range: Range<usize>, cx: &mut Context<Self>) {
        let Some(sel) = self.sel.clone() else { return };
        self.sel = Some(Selection {
            block: sel.block,
            anchor: range.start,
            head: range.end,
        });
        self.insert_text(None, "", cx);
    }

    fn move_to(&mut self, block: String, at: usize, extend: bool, cx: &mut Context<Self>) {
        match &mut self.sel {
            Some(sel) if extend && sel.block == block => sel.head = at,
            _ => self.sel = Some(Selection::caret(block, at)),
        }
        self.pending_marks = None;
        self.marked = None;
        self.picked = None;
        self.scroll_to_caret();
        cx.notify();
    }

    fn neighbour_text_block(&self, block: &str, dir: i32) -> Option<String> {
        let ids = self.text_blocks();
        let i = ids.iter().position(|b| b == block)? as i32 + dir;
        ids.get(usize::try_from(i).ok()?).cloned()
    }

    // ---- keyboard --------------------------------------------------------

    fn backspace(&mut self, _: &Backspace, _: &mut Window, cx: &mut Context<Self>) {
        if self.delete_picked(cx) {
            return;
        }
        let Some(sel) = self.sel.clone() else { return };
        let Some(text) = self.text_of(&sel.block).map(str::to_string) else {
            return;
        };
        if !sel.is_empty() {
            return self.delete_range(sel.range(), cx);
        }
        if sel.head == 0 {
            let caret = self.session.backspace_at_start(&sel.block);
            if caret.is_none() {
                // Stopped at a picture or drawing: pick it, so a second
                // press deletes it.
                if let Some(ix) = self.session.doc().index_of(&sel.block)
                    && ix > 0
                {
                    self.picked = Some(self.session.doc().blocks[ix - 1].id.clone());
                    cx.notify();
                }
                return;
            }
            self.set_caret_from(caret);
            self.changed(cx);
            return;
        }
        let prev = prev_grapheme(&text, sel.head);
        self.delete_range(prev..sel.head, cx);
    }

    fn delete(&mut self, _: &Delete, _: &mut Window, cx: &mut Context<Self>) {
        if self.delete_picked(cx) {
            return;
        }
        let Some(sel) = self.sel.clone() else { return };
        let Some(text) = self.text_of(&sel.block).map(str::to_string) else {
            return;
        };
        if !sel.is_empty() {
            return self.delete_range(sel.range(), cx);
        }
        if sel.head >= text.len() {
            let caret = self.session.delete_at_end(&sel.block);
            if caret.is_some() {
                self.set_caret_from(caret);
                self.changed(cx);
            }
            return;
        }
        let next = next_grapheme(&text, sel.head);
        self.delete_range(sel.head..next, cx);
    }

    fn delete_picked(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(id) = self.picked.take() else {
            return false;
        };
        let caret = self.session.delete_block(&id);
        self.set_caret_from(caret);
        self.changed(cx);
        true
    }

    fn enter(&mut self, _: &Enter, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(id) = self.picked.take() {
            // Enter on a picked picture or drawing opens a line after it.
            let ix = self.session.doc().index_of(&id);
            if let Some(next) = ix
                .and_then(|i| self.session.doc().blocks.get(i + 1))
                .filter(|b| b.as_text().is_some())
            {
                self.sel = Some(Selection::caret(next.id.clone(), 0));
            }
            cx.notify();
            return;
        }
        let Some(sel) = self.sel.clone() else { return };
        if !sel.is_empty() {
            self.delete_range(sel.range(), cx);
        }
        let Some(sel) = self.sel.clone() else { return };
        let Some(text) = self.text_of(&sel.block) else {
            return;
        };
        let at = utf16::char_at(text, sel.head);
        let caret = self.session.split(&sel.block, at);
        self.set_caret_from(caret);
        self.changed(cx);
    }

    fn horizontal(&mut self, dir: i32, extend: bool, word: bool, cx: &mut Context<Self>) {
        let Some(sel) = self.sel.clone() else { return };
        let Some(text) = self.text_of(&sel.block).map(str::to_string) else {
            return;
        };
        self.goal_x = None;
        if !extend && !sel.is_empty() && !word {
            let r = sel.range();
            let at = if dir < 0 { r.start } else { r.end };
            return self.move_to(sel.block, at, false, cx);
        }
        let at = sel.head;
        if dir < 0 && at == 0 || dir > 0 && at >= text.len() {
            if extend {
                return;
            }
            if let Some(next) = self.neighbour_text_block(&sel.block, dir) {
                let end = if dir < 0 {
                    self.text_of(&next).map(str::len).unwrap_or(0)
                } else {
                    0
                };
                self.move_to(next, end, false, cx);
            }
            return;
        }
        let to = match (dir < 0, word) {
            (true, false) => prev_grapheme(&text, at),
            (false, false) => next_grapheme(&text, at),
            (true, true) => word_start(&text, at),
            (false, true) => word_end(&text, at),
        };
        self.move_to(sel.block, to, extend, cx);
    }

    fn left(&mut self, _: &Left, _: &mut Window, cx: &mut Context<Self>) {
        self.horizontal(-1, false, false, cx)
    }
    fn right(&mut self, _: &Right, _: &mut Window, cx: &mut Context<Self>) {
        self.horizontal(1, false, false, cx)
    }
    fn select_left(&mut self, _: &SelectLeft, _: &mut Window, cx: &mut Context<Self>) {
        self.horizontal(-1, true, false, cx)
    }
    fn select_right(&mut self, _: &SelectRight, _: &mut Window, cx: &mut Context<Self>) {
        self.horizontal(1, true, false, cx)
    }
    fn word_left(&mut self, _: &WordLeft, _: &mut Window, cx: &mut Context<Self>) {
        self.horizontal(-1, false, true, cx)
    }
    fn word_right(&mut self, _: &WordRight, _: &mut Window, cx: &mut Context<Self>) {
        self.horizontal(1, false, true, cx)
    }
    fn select_word_left(&mut self, _: &SelectWordLeft, _: &mut Window, cx: &mut Context<Self>) {
        self.horizontal(-1, true, true, cx)
    }
    fn select_word_right(&mut self, _: &SelectWordRight, _: &mut Window, cx: &mut Context<Self>) {
        self.horizontal(1, true, true, cx)
    }

    /// Up and Down move by visual line, crossing into the neighbouring
    /// block at the edges and keeping the caret's column as they go.
    fn vertical(&mut self, dir: i32, extend: bool, window: &mut Window, cx: &mut Context<Self>) {
        let Some(sel) = self.sel.clone() else { return };
        let Some(layout) = self.layout(&sel.block) else {
            return;
        };
        let Some(pos) = layout.position_for_index(sel.head) else {
            return;
        };
        let lh = layout.line_height();
        let x = *self.goal_x.get_or_insert(pos.x);
        let target = point(x, pos.y + lh * 0.5 + lh * dir as f32);
        let b = layout.bounds();
        if target.y >= b.top() && target.y <= b.bottom() {
            let at = closest_index(&layout, target);
            let goal = self.goal_x;
            self.move_to(sel.block, at, extend, cx);
            self.goal_x = goal;
            return;
        }
        if extend {
            let at = if dir < 0 {
                0
            } else {
                self.text_of(&sel.block).map(str::len).unwrap_or(0)
            };
            let goal = self.goal_x;
            self.move_to(sel.block, at, true, cx);
            self.goal_x = goal;
            return;
        }
        let Some(next) = self.neighbour_text_block(&sel.block, dir) else {
            if dir < 0 {
                window.focus(&self.title.focus_handle(cx), cx);
            }
            return;
        };
        let at = match self.layout(&next) {
            Some(l) => {
                let nb = l.bounds();
                let y = if dir < 0 {
                    nb.bottom() - l.line_height() * 0.5
                } else {
                    nb.top() + l.line_height() * 0.5
                };
                closest_index(&l, point(x, y))
            }
            None => 0,
        };
        let goal = self.goal_x;
        self.move_to(next, at, false, cx);
        self.goal_x = goal;
    }

    fn up(&mut self, _: &Up, window: &mut Window, cx: &mut Context<Self>) {
        self.vertical(-1, false, window, cx)
    }
    fn down(&mut self, _: &Down, window: &mut Window, cx: &mut Context<Self>) {
        self.vertical(1, false, window, cx)
    }
    fn select_up(&mut self, _: &SelectUp, window: &mut Window, cx: &mut Context<Self>) {
        self.vertical(-1, true, window, cx)
    }
    fn select_down(&mut self, _: &SelectDown, window: &mut Window, cx: &mut Context<Self>) {
        self.vertical(1, true, window, cx)
    }

    fn line_edge(&mut self, end: bool, extend: bool, cx: &mut Context<Self>) {
        let Some(sel) = self.sel.clone() else { return };
        let Some(text) = self.text_of(&sel.block).map(str::to_string) else {
            return;
        };
        // The edge of the visual line, falling back to the block's edge.
        let at = self
            .layout(&sel.block)
            .and_then(|l| {
                let pos = l.position_for_index(sel.head)?;
                let b = l.bounds();
                let x = if end {
                    b.right() + px(1000.)
                } else {
                    b.left() - px(1000.)
                };
                Some(closest_index(&l, point(x, pos.y + l.line_height() * 0.5)))
            })
            .unwrap_or(if end { text.len() } else { 0 });
        self.move_to(sel.block, at, extend, cx);
    }

    fn home(&mut self, _: &Home, _: &mut Window, cx: &mut Context<Self>) {
        self.line_edge(false, false, cx)
    }
    fn end(&mut self, _: &End, _: &mut Window, cx: &mut Context<Self>) {
        self.line_edge(true, false, cx)
    }
    fn select_home(&mut self, _: &SelectHome, _: &mut Window, cx: &mut Context<Self>) {
        self.line_edge(false, true, cx)
    }
    fn select_end(&mut self, _: &SelectEnd, _: &mut Window, cx: &mut Context<Self>) {
        self.line_edge(true, true, cx)
    }

    fn doc_start(&mut self, _: &DocStart, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(first) = self.text_blocks().first().cloned() {
            self.move_to(first, 0, false, cx);
        }
    }

    fn doc_end(&mut self, _: &DocEnd, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(last) = self.text_blocks().last().cloned() {
            let len = self.text_of(&last).map(str::len).unwrap_or(0);
            self.move_to(last, len, false, cx);
        }
    }

    fn select_all(&mut self, _: &SelectAll, _: &mut Window, cx: &mut Context<Self>) {
        let Some(sel) = self.sel.clone() else { return };
        let len = self.text_of(&sel.block).map(str::len).unwrap_or(0);
        self.sel = Some(Selection {
            block: sel.block,
            anchor: 0,
            head: len,
        });
        cx.notify();
    }

    fn selected_text(&self) -> Option<String> {
        let sel = self.sel.as_ref().filter(|s| !s.is_empty())?;
        let text = self.text_of(&sel.block)?;
        Some(text[sel.range()].to_string())
    }

    fn copy(&mut self, _: &Copy, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(text) = self.selected_text() {
            cx.write_to_clipboard(ClipboardItem::new_string(text));
        }
    }

    fn cut(&mut self, _: &Cut, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(text) = self.selected_text() {
            cx.write_to_clipboard(ClipboardItem::new_string(text));
            self.insert_text(None, "", cx);
        }
    }

    fn paste(&mut self, _: &Paste, window: &mut Window, cx: &mut Context<Self>) {
        let Some(item) = cx.read_from_clipboard() else {
            return;
        };
        if let Some(text) = item.text() {
            self.insert_text(None, &text, cx);
            return;
        }
        // A copied picture.
        for entry in item.entries() {
            if let gpui::ClipboardEntry::Image(image) = entry {
                self.insert_image_bytes(&image.bytes, "pasted image", window, cx);
                return;
            }
        }
    }

    pub fn undo(&mut self, _: &Undo, window: &mut Window, cx: &mut Context<Self>) {
        let caret = self.session.undo();
        self.after_history(caret, window, cx);
    }

    pub fn redo(&mut self, _: &Redo, window: &mut Window, cx: &mut Context<Self>) {
        let caret = self.session.redo();
        self.after_history(caret, window, cx);
    }

    fn after_history(&mut self, caret: Option<Caret>, _: &mut Window, cx: &mut Context<Self>) {
        let title = self.session.title().to_string();
        self.title.update(cx, |f, cx| f.set_text(&title, cx));
        if caret.is_some() {
            self.set_caret_from(caret);
        } else if let Some(sel) = &self.sel {
            // Keep the caret if its block survived, clamped to its text.
            match self.text_of(&sel.block).map(str::len) {
                Some(len) => {
                    let b = sel.block.clone();
                    let at = sel.head.min(len);
                    let text = self.text_of(&b).unwrap_or_default();
                    let at = (0..=at)
                        .rev()
                        .find(|i| text.is_char_boundary(*i))
                        .unwrap_or(0);
                    self.sel = Some(Selection::caret(b, at));
                }
                None => self.sel = None,
            }
        }
        self.picked = None;
        self.changed(cx);
    }

    // ---- formatting ------------------------------------------------------

    pub fn toggle_mark(&mut self, mark: Mark, cx: &mut Context<Self>) {
        let Some(sel) = self.sel.clone() else { return };
        let Some(text) = self.text_of(&sel.block).map(str::to_string) else {
            return;
        };
        if sel.is_empty() {
            // Nothing selected: the next typed text takes the mark.
            let at = utf16::char_at(&text, sel.head);
            let mut marks = self
                .pending_marks
                .clone()
                .unwrap_or_else(|| self.session.marks_in(&sel.block, at..at));
            let same_kind = |m: &Mark| std::mem::discriminant(m) == std::mem::discriminant(&mark);
            if marks.contains(&mark) {
                marks.retain(|m| m != &mark);
            } else {
                marks.retain(|m| !same_kind(m));
                marks.push(mark);
            }
            self.pending_marks = Some(marks);
            cx.notify();
            return;
        }
        self.session
            .toggle_mark(&sel.block, utf16::chars(&text, &sel.range()), &mark);
        self.changed(cx);
    }

    pub fn clear_marks(&mut self, cx: &mut Context<Self>) {
        let Some(sel) = self.sel.clone() else { return };
        let Some(text) = self.text_of(&sel.block).map(str::to_string) else {
            return;
        };
        self.pending_marks = Some(Vec::new());
        self.session
            .clear_marks(&sel.block, utf16::chars(&text, &sel.range()));
        self.changed(cx);
    }

    /// The marks the toolbar shows as active.
    pub fn active_marks(&self) -> Vec<Mark> {
        if let Some(p) = &self.pending_marks {
            return p.clone();
        }
        let Some(sel) = &self.sel else {
            return Vec::new();
        };
        let Some(text) = self.text_of(&sel.block) else {
            return Vec::new();
        };
        self.session
            .marks_in(&sel.block, utf16::chars(text, &sel.range()))
    }

    pub fn current_style(&self) -> Option<ParaStyle> {
        let sel = self.sel.as_ref()?;
        self.session.text(&sel.block).map(|t| t.style)
    }

    pub fn set_style(&mut self, style: ParaStyle, cx: &mut Context<Self>) {
        let Some(sel) = self.sel.clone() else { return };
        self.session.set_style(&sel.block, style);
        self.changed(cx);
    }

    fn bold(&mut self, _: &Bold, _: &mut Window, cx: &mut Context<Self>) {
        self.toggle_mark(Mark::Bold, cx)
    }
    fn italic(&mut self, _: &Italic, _: &mut Window, cx: &mut Context<Self>) {
        self.toggle_mark(Mark::Italic, cx)
    }
    fn underline(&mut self, _: &Underline, _: &mut Window, cx: &mut Context<Self>) {
        self.toggle_mark(Mark::Underline, cx)
    }
    fn strike(&mut self, _: &Strike, _: &mut Window, cx: &mut Context<Self>) {
        self.toggle_mark(Mark::Strike, cx)
    }
    fn heading1(&mut self, _: &Heading1, _: &mut Window, cx: &mut Context<Self>) {
        self.set_style(ParaStyle::Heading1, cx)
    }
    fn heading2(&mut self, _: &Heading2, _: &mut Window, cx: &mut Context<Self>) {
        self.set_style(ParaStyle::Heading2, cx)
    }
    fn heading3(&mut self, _: &Heading3, _: &mut Window, cx: &mut Context<Self>) {
        self.set_style(ParaStyle::Heading3, cx)
    }
    fn body_text(&mut self, _: &BodyText, _: &mut Window, cx: &mut Context<Self>) {
        if self.current_style() != Some(ParaStyle::Body) {
            let style = self.current_style().unwrap_or(ParaStyle::Body);
            self.set_style(style, cx)
        }
    }
    fn bullet_list(&mut self, _: &BulletList, _: &mut Window, cx: &mut Context<Self>) {
        self.set_style(ParaStyle::Bullet, cx)
    }
    fn numbered_list(&mut self, _: &NumberedList, _: &mut Window, cx: &mut Context<Self>) {
        self.set_style(ParaStyle::Numbered, cx)
    }
    fn checklist(&mut self, _: &Checklist, _: &mut Window, cx: &mut Context<Self>) {
        self.set_style(ParaStyle::Checklist, cx)
    }

    fn toggle_check(&mut self, _: &ToggleCheck, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(sel) = self.sel.clone() {
            self.session.toggle_checked(&sel.block);
            self.changed(cx);
        }
    }

    pub fn toggle_checked(&mut self, block: &str, cx: &mut Context<Self>) {
        self.session.toggle_checked(block);
        self.changed(cx);
    }

    fn indent(&mut self, _: &Indent, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(sel) = self.sel.clone() {
            if self.current_style().is_some_and(|s| s.is_list()) {
                self.session.indent(&sel.block, 1);
                self.changed(cx);
            } else {
                self.insert_text(None, "    ", cx);
            }
        }
    }

    fn outdent(&mut self, _: &Outdent, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(sel) = self.sel.clone() {
            self.session.indent(&sel.block, -1);
            self.changed(cx);
        }
    }

    fn escape(&mut self, _: &Escape, _: &mut Window, cx: &mut Context<Self>) {
        if self.popover.take().is_some() {
        } else if self.tool != InkTool::Type {
            self.tool = InkTool::Type;
        } else if let Some(sel) = &mut self.sel {
            sel.anchor = sel.head;
        }
        self.picked = None;
        cx.notify();
    }

    fn zoom_in(&mut self, _: &ZoomIn, _: &mut Window, cx: &mut Context<Self>) {
        self.zoom_by(1.25, cx)
    }

    fn zoom_out(&mut self, _: &ZoomOut, _: &mut Window, cx: &mut Context<Self>) {
        self.zoom_by(0.8, cx)
    }

    fn zoom_reset(&mut self, _: &ZoomReset, _: &mut Window, cx: &mut Context<Self>) {
        self.zoom_by(0., cx)
    }

    /// Starts on the pen, for a note made to be written by hand.
    pub fn start_drawing(&mut self, cx: &mut Context<Self>) {
        self.tool = InkTool::Pen;
        cx.notify();
    }

    // ---- blocks ----------------------------------------------------------

    fn insertion_point(&self) -> Option<String> {
        self.picked
            .clone()
            .or_else(|| self.sel.as_ref().map(|s| s.block.clone()))
    }

    pub fn insert_divider(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let after = self.insertion_point();
        let id = self.session.insert(after.as_deref(), Insert::Divider);
        self.after_insert(&id, window, cx);
    }

    pub fn insert_image_bytes(
        &mut self,
        bytes: &[u8],
        name: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(mime) = snot_core::markdown::sniff_image(bytes) else {
            eprintln!("snot: {name} is not a PNG, JPEG, GIF or WebP picture");
            return;
        };
        let att = match self.lib.add_attachment(bytes, mime, name) {
            Ok(a) => a,
            Err(e) => return eprintln!("snot: could not store picture: {e}"),
        };
        let aspect = snot_core::markdown::image_aspect(bytes).unwrap_or(0.75);
        let after = self.insertion_point();
        let id = self.session.insert(
            after.as_deref(),
            Insert::Image {
                attachment: att.id,
                aspect,
            },
        );
        self.after_insert(&id, window, cx);
    }

    /// After a block goes in, the caret moves to the line below it.
    fn after_insert(&mut self, id: &str, window: &mut Window, cx: &mut Context<Self>) {
        let doc = self.session.doc();
        if let Some(ix) = doc.index_of(id)
            && let Some(next) = doc.blocks.get(ix + 1).filter(|b| b.as_text().is_some())
        {
            self.sel = Some(Selection::caret(next.id.clone(), 0));
            self.scroll_to_caret();
        }
        window.focus(&self.focus, cx);
        self.changed(cx);
    }

    pub fn delete_block(&mut self, id: &str, cx: &mut Context<Self>) {
        let caret = self.session.delete_block(id);
        if self.picked.as_deref() == Some(id) {
            self.picked = None;
        }
        self.set_caret_from(caret);
        self.changed(cx);
    }

    pub fn move_block(&mut self, id: &str, delta: i32, cx: &mut Context<Self>) {
        self.session.move_block(id, delta);
        self.changed(cx);
    }

    pub fn set_paper(&mut self, paper: Paper, cx: &mut Context<Self>) {
        self.session.set_paper(paper);
        self.changed(cx);
    }

    pub fn clear_ink(&mut self, cx: &mut Context<Self>) {
        self.session.clear_ink();
        self.changed(cx);
    }

    // ---- mouse -----------------------------------------------------------

    pub(crate) fn text_mouse_down(
        &mut self,
        block: &str,
        pos: Point<Pixels>,
        clicks: usize,
        shift: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        window.focus(&self.focus, cx);
        self.popover = None;
        let Some(text) = self.text_of(block).map(str::to_string) else {
            return;
        };
        let at = self
            .layout(block)
            .map(|l| closest_index(&l, pos))
            .unwrap_or(text.len());
        self.picked = None;
        self.pending_marks = None;
        self.goal_x = None;
        match clicks {
            2 => {
                let start = word_start(&text, next_grapheme(&text, at).min(text.len()));
                let end = word_end(&text, at);
                self.sel = Some(Selection {
                    block: block.into(),
                    anchor: start.min(at),
                    head: end.max(at),
                });
            }
            n if n >= 3 => {
                self.sel = Some(Selection {
                    block: block.into(),
                    anchor: 0,
                    head: text.len(),
                })
            }
            _ => {
                match &mut self.sel {
                    Some(sel) if shift && sel.block == block => sel.head = at,
                    _ => self.sel = Some(Selection::caret(block, at)),
                }
                self.selecting = true;
            }
        }
        cx.notify();
    }

    pub(crate) fn text_mouse_drag(&mut self, pos: Point<Pixels>, cx: &mut Context<Self>) {
        if !self.selecting {
            return;
        }
        let Some(sel) = self.sel.clone() else { return };
        if let Some(l) = self.layout(&sel.block) {
            let at = closest_index(&l, pos);
            if at != sel.head {
                self.sel = Some(Selection { head: at, ..sel });
                cx.notify();
            }
        }
    }

    pub(crate) fn mouse_up(&mut self, cx: &mut Context<Self>) {
        self.selecting = false;
        if self.ink.finish(&mut self.session) {
            self.changed(cx);
        }
    }

    pub(crate) fn pick(&mut self, block: &str, window: &mut Window, cx: &mut Context<Self>) {
        window.focus(&self.focus, cx);
        self.picked = Some(block.to_string());
        self.popover = None;
        cx.notify();
    }

    // ---- note metadata -----------------------------------------------------

    pub fn set_note_color(&mut self, color: Option<String>, cx: &mut Context<Self>) {
        if self
            .lib
            .set_note_color(self.note_id(), color.as_deref())
            .is_ok()
        {
            self.color = color;
            cx.emit(NoteEvent::Saved);
        }
        self.popover = None;
        cx.notify();
    }

    pub fn toggle_favorite(&mut self, cx: &mut Context<Self>) {
        if self
            .lib
            .set_favorite(self.note_id(), !self.favorite)
            .is_ok()
        {
            self.favorite = !self.favorite;
            cx.emit(NoteEvent::Saved);
        }
        cx.notify();
    }

    pub fn toggle_pinned(&mut self, cx: &mut Context<Self>) {
        if self.lib.set_pinned(self.note_id(), !self.pinned).is_ok() {
            self.pinned = !self.pinned;
            cx.emit(NoteEvent::Saved);
        }
        cx.notify();
    }

    pub fn move_to_folder(&mut self, folder: Option<String>, cx: &mut Context<Self>) {
        if self
            .lib
            .move_note(self.note_id(), folder.as_deref())
            .is_ok()
        {
            self.folder = folder;
            cx.emit(NoteEvent::Saved);
        }
        self.popover = None;
        cx.notify();
    }

    pub fn open_popover(&mut self, which: Popover, at: Point<Pixels>, cx: &mut Context<Self>) {
        self.popover = match self.popover {
            Some((p, _)) if p == which => None,
            _ => Some((which, at)),
        };
        cx.notify();
    }

    /// Re-reads flags that may have been changed from the note list.
    pub fn refresh_meta(&mut self, cx: &mut Context<Self>) {
        if let Ok(n) = self.lib.note(self.note_id()) {
            self.color = n.color;
            self.favorite = n.favorite;
            self.pinned = n.pinned;
            self.folder = n.folder_id;
            self.trashed = n.trashed_at.is_some();
            cx.notify();
        }
    }
}

/// The byte index nearest `pos`, clicking past a line's end landing on it.
pub fn closest_index(layout: &TextLayout, pos: Point<Pixels>) -> usize {
    let b = layout.bounds();
    let lh = layout.line_height();
    if pos.y < b.top() {
        return 0;
    }
    if pos.y > b.bottom() {
        return layout.len();
    }
    let Some(line) = layout.line_layouts().into_iter().next() else {
        return 0;
    };
    let local = point(
        pos.x - b.left(),
        (pos.y - b.top()).min(b.size.height - px(1.)).max(px(0.)),
    );
    match line.closest_index_for_position(local, lh) {
        Ok(i) | Err(i) => i.min(layout.len()),
    }
}

impl EntityInputHandler for NoteView {
    fn text_for_range(
        &mut self,
        range: Range<usize>,
        actual: &mut Option<Range<usize>>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<String> {
        let sel = self.sel.as_ref()?;
        let text = self.text_of(&sel.block)?;
        let r = utf16::range_from(text, &range);
        actual.replace(utf16::range_to(text, &r));
        Some(text[r].to_string())
    }

    fn selected_text_range(
        &mut self,
        _: bool,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        let sel = self.sel.as_ref()?;
        let text = self.text_of(&sel.block)?;
        Some(UTF16Selection {
            range: utf16::range_to(text, &sel.range()),
            reversed: sel.head < sel.anchor,
        })
    }

    fn marked_text_range(&self, _: &mut Window, _: &mut Context<Self>) -> Option<Range<usize>> {
        let sel = self.sel.as_ref()?;
        let text = self.text_of(&sel.block)?;
        self.marked.as_ref().map(|m| utf16::range_to(text, m))
    }

    fn unmark_text(&mut self, _: &mut Window, _: &mut Context<Self>) {
        self.marked = None;
    }

    fn replace_text_in_range(
        &mut self,
        range: Option<Range<usize>>,
        text: &str,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.tool != InkTool::Type {
            self.tool = InkTool::Type;
        }
        let range = range.and_then(|r| {
            Some(utf16::range_from(
                self.text_of(&self.sel.as_ref()?.block)?,
                &r,
            ))
        });
        self.insert_text(range, text, cx);
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        range: Option<Range<usize>>,
        text: &str,
        new_sel: Option<Range<usize>>,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(sel) = self.sel.clone() else { return };
        let Some(current) = self.text_of(&sel.block).map(str::to_string) else {
            return;
        };
        let range = range
            .map(|r| utf16::range_from(&current, &r))
            .or(self.marked.clone())
            .unwrap_or(sel.range());
        let chars = utf16::chars(&current, &range);
        self.session.replace(&sel.block, chars, text, None);
        self.marked = (!text.is_empty()).then(|| range.start..range.start + text.len());
        let (a, h) = match new_sel {
            Some(r) => {
                let r = utf16::range_from(text, &r);
                (range.start + r.start, range.start + r.end)
            }
            None => (range.start + text.len(), range.start + text.len()),
        };
        self.sel = Some(Selection {
            block: sel.block,
            anchor: a,
            head: h,
        });
        self.changed(cx);
    }

    fn bounds_for_range(
        &mut self,
        range: Range<usize>,
        _: Bounds<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        let sel = self.sel.as_ref()?;
        let text = self.text_of(&sel.block)?;
        let r = utf16::range_from(text, &range);
        let l = self.layout(&sel.block)?;
        let start = l.position_for_index(r.start)?;
        let end = l.position_for_index(r.end).unwrap_or(start);
        Some(Bounds::from_corners(
            start,
            point(end.x.max(start.x + px(1.)), end.y + l.line_height()),
        ))
    }

    fn character_index_for_point(
        &mut self,
        p: Point<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<usize> {
        let sel = self.sel.as_ref()?;
        let l = self.layout(&sel.block)?;
        let text = self.text_of(&sel.block)?;
        Some(utf16::offset_to(text, closest_index(&l, p)))
    }
}

pub fn key_bindings() -> Vec<gpui::KeyBinding> {
    use gpui::KeyBinding;
    let c = Some(CONTEXT);
    vec![
        KeyBinding::new("backspace", Backspace, c),
        KeyBinding::new("delete", Delete, c),
        KeyBinding::new("enter", Enter, c),
        KeyBinding::new("left", Left, c),
        KeyBinding::new("right", Right, c),
        KeyBinding::new("up", Up, c),
        KeyBinding::new("down", Down, c),
        KeyBinding::new("shift-left", SelectLeft, c),
        KeyBinding::new("shift-right", SelectRight, c),
        KeyBinding::new("shift-up", SelectUp, c),
        KeyBinding::new("shift-down", SelectDown, c),
        KeyBinding::new("ctrl-left", WordLeft, c),
        KeyBinding::new("ctrl-right", WordRight, c),
        KeyBinding::new("ctrl-shift-left", SelectWordLeft, c),
        KeyBinding::new("ctrl-shift-right", SelectWordRight, c),
        KeyBinding::new("home", Home, c),
        KeyBinding::new("end", End, c),
        KeyBinding::new("shift-home", SelectHome, c),
        KeyBinding::new("shift-end", SelectEnd, c),
        KeyBinding::new("ctrl-home", DocStart, c),
        KeyBinding::new("ctrl-end", DocEnd, c),
        KeyBinding::new("ctrl-a", SelectAll, c),
        KeyBinding::new("ctrl-c", Copy, c),
        KeyBinding::new("ctrl-x", Cut, c),
        KeyBinding::new("ctrl-v", Paste, c),
        KeyBinding::new("ctrl-z", Undo, c),
        KeyBinding::new("ctrl-shift-z", Redo, c),
        KeyBinding::new("ctrl-y", Redo, c),
        KeyBinding::new("ctrl-b", Bold, c),
        KeyBinding::new("ctrl-i", Italic, c),
        KeyBinding::new("ctrl-u", Underline, c),
        KeyBinding::new("ctrl-shift-x", Strike, c),
        KeyBinding::new("tab", Indent, c),
        KeyBinding::new("shift-tab", Outdent, c),
        KeyBinding::new("ctrl-1", Heading1, c),
        KeyBinding::new("ctrl-2", Heading2, c),
        KeyBinding::new("ctrl-3", Heading3, c),
        KeyBinding::new("ctrl-0", BodyText, c),
        // Linux reports Shift+digit as the symbol it types on a US layout.
        KeyBinding::new("ctrl-shift-8", BulletList, c),
        KeyBinding::new("ctrl-*", BulletList, c),
        KeyBinding::new("ctrl-shift-7", NumberedList, c),
        KeyBinding::new("ctrl-&", NumberedList, c),
        KeyBinding::new("ctrl-shift-9", Checklist, c),
        KeyBinding::new("ctrl-(", Checklist, c),
        KeyBinding::new("ctrl-enter", ToggleCheck, c),
        KeyBinding::new("escape", Escape, c),
        KeyBinding::new("ctrl-=", ZoomIn, c),
        KeyBinding::new("ctrl-+", ZoomIn, c),
        KeyBinding::new("ctrl--", ZoomOut, c),
        KeyBinding::new("ctrl-0", ZoomReset, c),
    ]
}
