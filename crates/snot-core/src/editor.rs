//! An open note being edited.
//!
//! Every structural edit — Enter, Backspace at the start of a line, list
//! and heading changes, handwriting — goes through here, so the desktop and the
//! phone behave identically and undo works the same on both. Front-ends
//! own carets, selection and drawing; the session owns the document.

use std::collections::HashMap;
use std::ops::Range;
use std::sync::Arc;

use crate::doc::{Block, Content, Doc, ImageBlock, Mark, ParaStyle, TextBlock, new_block_id};
use crate::ink::{InkLayer, Paper, Stroke};
use crate::note::{Note, now_ms};

const UNDO_LIMIT: usize = 200;
/// Typing in one block merges into one undo step until it pauses this long.
const TYPING_PAUSE_MS: i64 = 1200;
pub const MAX_INDENT: u8 = 6;

/// Where the caret should go after an edit.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Caret {
    pub block: String,
    pub offset: u32,
}

/// What can be inserted after a block.
#[derive(Clone, Debug)]
pub enum Insert {
    Divider,
    Image { attachment: String, aspect: f32 },
}

#[derive(Clone, Debug, PartialEq)]
enum Step {
    Typing(String),
    Title,
    Erase,
    Other,
}

#[derive(Clone)]
struct Snapshot {
    title: String,
    doc: Doc,
    caret: Option<Caret>,
}

pub struct EditSession {
    note: Note,
    undo: Vec<Snapshot>,
    redo: Vec<Snapshot>,
    last: Option<(Step, i64)>,
    revisions: HashMap<String, u64>,
    blocks_revision: u64,
    ink_revision: u64,
    clock: u64,
    dirty: bool,
}

impl EditSession {
    pub fn new(mut note: Note) -> Self {
        note.doc.sanitize();
        let mut s = EditSession {
            note,
            undo: Vec::new(),
            redo: Vec::new(),
            last: None,
            revisions: HashMap::new(),
            blocks_revision: 0,
            ink_revision: 0,
            clock: 0,
            dirty: false,
        };
        s.touch_all();
        s
    }

    pub fn note(&self) -> &Note {
        &self.note
    }

    pub fn doc(&self) -> &Doc {
        &self.note.doc
    }

    pub fn title(&self) -> &str {
        &self.note.title
    }

    /// Whether there are changes not yet saved.
    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    pub fn mark_saved(&mut self) {
        self.dirty = false;
    }

    /// A number that changes whenever the block does, so a front-end can
    /// keep laid-out text until it is stale.
    pub fn revision(&self, block: &str) -> u64 {
        self.revisions.get(block).copied().unwrap_or(0)
    }

    /// A number that changes whenever any block does, or blocks come, go
    /// or move: when it holds, so does all the text on the page.
    pub fn blocks_revision(&self) -> u64 {
        self.blocks_revision
    }

    /// The same for the handwriting, so outlines are only redone when it
    /// changes.
    pub fn ink_revision(&self) -> u64 {
        self.ink_revision
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    /// Ends the current gesture, so the next edit starts a new undo step
    /// even if it is of the same kind (two separate eraser swipes, say).
    pub fn end_gesture(&mut self) {
        self.last = None;
    }

    fn touch(&mut self, block: &str) {
        self.clock += 1;
        self.revisions.insert(block.to_string(), self.clock);
        self.blocks_revision = self.clock;
    }

    fn touch_all(&mut self) {
        self.clock += 1;
        let clock = self.clock;
        self.revisions = self
            .note
            .doc
            .blocks
            .iter()
            .map(|b| (b.id.clone(), clock))
            .collect();
        self.blocks_revision = clock;
        self.ink_revision = clock;
    }

    fn touch_ink(&mut self) {
        self.clock += 1;
        self.ink_revision = self.clock;
    }

    /// Records the state before an edit, merging into the previous step
    /// when it continues it.
    fn checkpoint(&mut self, step: Step, caret: Option<Caret>) {
        let now = now_ms();
        let continues = match (&self.last, &step) {
            (Some((last, at)), Step::Typing(_) | Step::Title) => {
                last == &step && now - at < TYPING_PAUSE_MS
            }
            (Some((last, _)), Step::Erase) => last == &step,
            _ => false,
        };
        if !continues {
            self.undo.push(Snapshot {
                title: self.note.title.clone(),
                doc: self.note.doc.clone(),
                caret,
            });
            if self.undo.len() > UNDO_LIMIT {
                self.undo.remove(0);
            }
        }
        self.last = Some((step, now));
        self.redo.clear();
        self.dirty = true;
    }

    pub fn undo(&mut self) -> Option<Caret> {
        let snap = self.undo.pop()?;
        let current = Snapshot {
            title: self.note.title.clone(),
            doc: self.note.doc.clone(),
            caret: snap.caret.clone(),
        };
        self.redo.push(current);
        self.restore(snap)
    }

    pub fn redo(&mut self) -> Option<Caret> {
        let snap = self.redo.pop()?;
        let current = Snapshot {
            title: self.note.title.clone(),
            doc: self.note.doc.clone(),
            caret: snap.caret.clone(),
        };
        self.undo.push(current);
        self.restore(snap)
    }

    fn restore(&mut self, snap: Snapshot) -> Option<Caret> {
        self.note.title = snap.title;
        self.note.doc = snap.doc;
        self.last = None;
        self.dirty = true;
        self.touch_all();
        snap.caret
            .filter(|c| self.note.doc.index_of(&c.block).is_some())
    }

    fn text_mut(&mut self, block: &str) -> Option<&mut TextBlock> {
        self.note.doc.block_mut(block)?.as_text_mut()
    }

    pub fn text(&self, block: &str) -> Option<&TextBlock> {
        self.note.doc.block(block)?.as_text()
    }

    // ---- title -----------------------------------------------------------

    pub fn set_title(&mut self, title: &str) {
        let title = title.replace(['\n', '\r'], " ");
        if title == self.note.title {
            return;
        }
        self.checkpoint(Step::Title, None);
        self.note.title = title;
    }

    // ---- text ------------------------------------------------------------

    /// Replaces `range` of a text block. `marks` overrides the style the
    /// new text would otherwise inherit (a bold button pressed with nothing
    /// selected, say).
    pub fn replace(
        &mut self,
        block: &str,
        range: Range<u32>,
        text: &str,
        marks: Option<&[Mark]>,
    ) -> bool {
        if self.text(block).is_none() {
            return false;
        }
        let caret = Caret {
            block: block.into(),
            offset: range.start,
        };
        self.checkpoint(Step::Typing(block.into()), Some(caret));
        self.text_mut(block).expect("checked above").replace(
            range,
            &text.replace(['\n', '\r'], " "),
            marks,
        );
        self.touch(block);
        true
    }

    /// Sets a text block's whole text, diffing it against what was there.
    /// For editors that report the field's full contents after each change.
    pub fn set_text(&mut self, block: &str, text: &str, marks: Option<&[Mark]>) -> bool {
        let text = text.replace(['\n', '\r'], " ");
        match self.text(block) {
            Some(t) if t.text == text => return false,
            None => return false,
            Some(_) => {}
        }
        let caret = Caret {
            block: block.into(),
            offset: 0,
        };
        self.checkpoint(Step::Typing(block.into()), Some(caret));
        self.text_mut(block)
            .expect("checked above")
            .set_text(&text, marks);
        self.touch(block);
        true
    }

    /// Inserts text that may span several lines, as a paste does: the first
    /// line goes into this block and each further line becomes a block of
    /// the same style. One undo step.
    pub fn paste(&mut self, block: &str, range: Range<u32>, text: &str) -> Option<Caret> {
        let idx = self.note.doc.index_of(block)?;
        self.text(block)?;
        let text = text.replace("\r\n", "\n").replace('\r', "\n");
        let mut lines = text.split('\n');
        let first = lines.next().unwrap_or_default();
        self.checkpoint(
            Step::Other,
            Some(Caret {
                block: block.into(),
                offset: range.start,
            }),
        );
        let t = self.text_mut(block).expect("checked above");
        let start = range.start.min(t.len());
        t.replace(range, first, None);
        let rest: Vec<&str> = lines.collect();
        self.touch(block);
        if rest.is_empty() {
            return Some(Caret {
                block: block.into(),
                offset: start + crate::doc::char_len(first),
            });
        }
        let t = self.text_mut(block).expect("checked above");
        let tail = t.split_off(start + crate::doc::char_len(first));
        let mut caret = Caret {
            block: block.into(),
            offset: 0,
        };
        for (i, line) in rest.iter().enumerate() {
            let mut b = TextBlock {
                style: tail.style,
                indent: tail.indent,
                ..Default::default()
            };
            b.replace(0..0, line, Some(&[]));
            let offset = b.len();
            if i + 1 == rest.len() {
                b.append(&tail);
            }
            let id = new_block_id();
            self.note.doc.blocks.insert(
                idx + 1 + i,
                Block {
                    id: id.clone(),
                    content: Content::Text(b),
                },
            );
            self.touch(&id);
            caret = Caret { block: id, offset };
        }
        Some(caret)
    }

    /// Enter: splits a text block at `at`. On an empty list item it leaves
    /// the list instead, the way every list editor does.
    pub fn split(&mut self, block: &str, at: u32) -> Option<Caret> {
        let idx = self.note.doc.index_of(block)?;
        let t = self.text(block)?;
        if t.is_empty() && t.style.is_list() {
            self.set_style(block, ParaStyle::Body);
            return Some(Caret {
                block: block.into(),
                offset: 0,
            });
        }
        self.checkpoint(
            Step::Other,
            Some(Caret {
                block: block.into(),
                offset: at,
            }),
        );
        let t = self.text_mut(block).expect("checked above");
        let mut tail = t.split_off(at);
        // A heading is one line; what follows it is body text.
        if matches!(
            tail.style,
            ParaStyle::Heading1 | ParaStyle::Heading2 | ParaStyle::Heading3
        ) {
            tail.style = ParaStyle::Body;
        }
        let id = new_block_id();
        self.note.doc.blocks.insert(
            idx + 1,
            Block {
                id: id.clone(),
                content: Content::Text(tail),
            },
        );
        self.touch(block);
        self.touch(&id);
        Some(Caret {
            block: id,
            offset: 0,
        })
    }

    /// Backspace at the very start of a block. Peels formatting off first —
    /// a list item becomes plain text, an indent steps out — and only then
    /// joins the line to the one above.
    pub fn backspace_at_start(&mut self, block: &str) -> Option<Caret> {
        let idx = self.note.doc.index_of(block)?;
        let t = self.text(block)?;
        if t.style != ParaStyle::Body {
            self.set_style(block, ParaStyle::Body);
            return Some(Caret {
                block: block.into(),
                offset: 0,
            });
        }
        if t.indent > 0 {
            self.indent(block, -1);
            return Some(Caret {
                block: block.into(),
                offset: 0,
            });
        }
        let prev_idx = idx.checked_sub(1)?;
        match &self.note.doc.blocks[prev_idx].content {
            Content::Text(prev) => {
                let prev_id = self.note.doc.blocks[prev_idx].id.clone();
                let offset = prev.len();
                self.checkpoint(
                    Step::Other,
                    Some(Caret {
                        block: block.into(),
                        offset: 0,
                    }),
                );
                let Content::Text(this) = self.note.doc.blocks.remove(idx).content else {
                    unreachable!()
                };
                self.text_mut(&prev_id)
                    .expect("checked above")
                    .append(&this);
                self.touch(&prev_id);
                Some(Caret {
                    block: prev_id,
                    offset,
                })
            }
            // A divider is cheap to recreate, so it goes; a picture is
            // not, so backspace stops at it.
            Content::Divider => {
                self.checkpoint(
                    Step::Other,
                    Some(Caret {
                        block: block.into(),
                        offset: 0,
                    }),
                );
                self.note.doc.blocks.remove(prev_idx);
                Some(Caret {
                    block: block.into(),
                    offset: 0,
                })
            }
            _ => None,
        }
    }

    /// Delete at the very end of a block: pulls the next text block up.
    pub fn delete_at_end(&mut self, block: &str) -> Option<Caret> {
        let idx = self.note.doc.index_of(block)?;
        let offset = self.text(block)?.len();
        let next = self.note.doc.blocks.get(idx + 1)?;
        let Content::Text(_) = &next.content else {
            return None;
        };
        let next_id = next.id.clone();
        self.backspace_at_start(&next_id);
        // Formatting may have been peeled instead; run it until it merges.
        while self.note.doc.index_of(&next_id).is_some() {
            self.backspace_at_start(&next_id)?;
        }
        Some(Caret {
            block: block.into(),
            offset,
        })
    }

    /// Changes a block's paragraph style. Picking the style it already has
    /// turns it back into body text, so toolbar buttons toggle.
    pub fn set_style(&mut self, block: &str, style: ParaStyle) {
        let Some(current) = self.text(block).map(|t| t.style) else {
            return;
        };
        let style = if current == style {
            ParaStyle::Body
        } else {
            style
        };
        self.checkpoint(Step::Other, None);
        let t = self.text_mut(block).expect("checked above");
        t.style = style;
        if !style.is_list() {
            t.checked = false;
        }
        self.touch(block);
    }

    pub fn toggle_checked(&mut self, block: &str) {
        if self
            .text(block)
            .is_none_or(|t| t.style != ParaStyle::Checklist)
        {
            return;
        }
        self.checkpoint(Step::Other, None);
        let t = self.text_mut(block).expect("checked above");
        t.checked = !t.checked;
        self.touch(block);
    }

    pub fn indent(&mut self, block: &str, delta: i32) {
        let Some(t) = self.text(block) else { return };
        let indent = (t.indent as i32 + delta).clamp(0, MAX_INDENT as i32) as u8;
        if indent == t.indent {
            return;
        }
        self.checkpoint(Step::Other, None);
        self.text_mut(block).expect("checked above").indent = indent;
        self.touch(block);
    }

    pub fn toggle_mark(&mut self, block: &str, range: Range<u32>, mark: &Mark) {
        if range.is_empty() || self.text(block).is_none() {
            return;
        }
        self.checkpoint(Step::Other, None);
        self.text_mut(block)
            .expect("checked above")
            .toggle_mark(range, mark);
        self.touch(block);
    }

    pub fn clear_marks(&mut self, block: &str, range: Range<u32>) {
        if range.is_empty() || self.text(block).is_none_or(|t| t.spans.is_empty()) {
            return;
        }
        self.checkpoint(Step::Other, None);
        self.text_mut(block)
            .expect("checked above")
            .clear_marks(range);
        self.touch(block);
    }

    /// The marks a toolbar should show as active for this range.
    pub fn marks_in(&self, block: &str, range: Range<u32>) -> Vec<Mark> {
        self.text(block)
            .map(|t| t.marks_in(range))
            .unwrap_or_default()
    }

    // ---- blocks ----------------------------------------------------------

    /// Inserts a block after `after` (or at the end) and returns its id.
    /// When `after` is an empty line, the new block takes its place instead
    /// of leaving a blank line behind it.
    pub fn insert(&mut self, after: Option<&str>, what: Insert) -> String {
        self.checkpoint(Step::Other, None);
        let content = match what {
            Insert::Divider => Content::Divider,
            Insert::Image { attachment, aspect } => Content::Image(ImageBlock {
                attachment,
                aspect: if aspect.is_finite() && aspect > 0.0 {
                    aspect
                } else {
                    0.75
                },
                caption: String::new(),
            }),
        };
        let id = new_block_id();
        let block = Block {
            id: id.clone(),
            content,
        };
        let blocks = &mut self.note.doc.blocks;
        match after.and_then(|a| blocks.iter().position(|b| b.id == a)) {
            Some(i)
                if blocks[i]
                    .as_text()
                    .is_some_and(|t| t.is_empty() && t.style == ParaStyle::Body) =>
            {
                blocks[i] = block;
            }
            Some(i) => blocks.insert(i + 1, block),
            None => blocks.push(block),
        }
        self.note.doc.sanitize();
        self.touch_all();
        id
    }

    /// Removes a block. Returns where the caret can go next.
    pub fn delete_block(&mut self, block: &str) -> Option<Caret> {
        let idx = self.note.doc.index_of(block)?;
        self.checkpoint(Step::Other, None);
        self.note.doc.blocks.remove(idx);
        self.note.doc.sanitize();
        self.touch_all();
        let blocks = &self.note.doc.blocks;
        let near = blocks[..idx.min(blocks.len())]
            .iter()
            .rev()
            .chain(blocks[idx.min(blocks.len())..].iter())
            .find(|b| b.as_text().is_some())?;
        Some(Caret {
            block: near.id.clone(),
            offset: near.as_text().map(|t| t.len()).unwrap_or(0),
        })
    }

    /// Moves a block up (`-1`) or down (`+1`).
    pub fn move_block(&mut self, block: &str, delta: i32) {
        let Some(idx) = self.note.doc.index_of(block) else {
            return;
        };
        let to = idx as i64 + delta as i64;
        if to < 0 || to >= self.note.doc.blocks.len() as i64 {
            return;
        }
        self.checkpoint(Step::Other, None);
        let b = self.note.doc.blocks.remove(idx);
        self.note.doc.blocks.insert(to as usize, b);
        self.note.doc.sanitize();
        self.touch_all();
    }

    pub fn set_caption(&mut self, block: &str, caption: &str) {
        let Some(Block {
            content: Content::Image(_),
            ..
        }) = self.note.doc.block(block)
        else {
            return;
        };
        self.checkpoint(Step::Typing(block.into()), None);
        if let Some(Block {
            content: Content::Image(img),
            ..
        }) = self.note.doc.block_mut(block)
        {
            img.caption = caption.replace(['\n', '\r'], " ");
        }
        self.touch(block);
    }

    // ---- ink -------------------------------------------------------------

    pub fn ink(&self) -> &InkLayer {
        &self.note.doc.ink
    }

    fn ink_mut(&mut self) -> &mut InkLayer {
        Arc::make_mut(&mut self.note.doc.ink)
    }

    pub fn add_stroke(&mut self, stroke: Stroke) -> bool {
        if stroke.points.is_empty() {
            return false;
        }
        self.checkpoint(Step::Other, None);
        self.ink_mut().add_stroke(stroke);
        self.touch_ink();
        true
    }

    /// Erases the strokes under the eraser and returns where they were. A
    /// whole swipe is one undo step, up to the next
    /// [`end_gesture`](Self::end_gesture).
    pub fn erase(&mut self, x: f32, y: f32, radius: f32) -> Vec<usize> {
        if !self.ink().strokes.iter().any(|s| s.hit(x, y, radius)) {
            return Vec::new();
        }
        self.checkpoint(Step::Erase, None);
        let removed = self.ink_mut().erase_at(x, y, radius);
        self.touch_ink();
        removed
    }

    pub fn set_paper(&mut self, paper: Paper) {
        if self.ink().paper == paper {
            return;
        }
        self.checkpoint(Step::Other, None);
        self.ink_mut().paper = paper;
        self.touch_ink();
    }

    pub fn clear_ink(&mut self) {
        if self.ink().strokes.is_empty() {
            return;
        }
        self.checkpoint(Step::Other, None);
        self.ink_mut().strokes.clear();
        self.touch_ink();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ink::{Point, Tool};

    fn session(lines: &[(&str, ParaStyle)]) -> EditSession {
        let blocks = lines.iter().map(|(t, s)| Block::styled(*s, *t)).collect();
        EditSession::new(Note {
            id: "n".into(),
            folder_id: None,
            title: String::new(),
            doc: Doc::of(blocks),
            color: None,
            pinned: false,
            favorite: false,
            created_at: 0,
            updated_at: 0,
            trashed_at: None,
        })
    }

    fn ids(s: &EditSession) -> Vec<String> {
        s.doc().blocks.iter().map(|b| b.id.clone()).collect()
    }

    fn texts(s: &EditSession) -> Vec<String> {
        s.doc()
            .blocks
            .iter()
            .map(|b| b.as_text().map(|t| t.text.clone()).unwrap_or("#".into()))
            .collect()
    }

    #[test]
    fn enter_splits_and_backspace_joins() {
        let mut s = session(&[("hello world", ParaStyle::Body)]);
        let first = ids(&s)[0].clone();
        let caret = s.split(&first, 5).unwrap();
        assert_eq!(texts(&s), ["hello", " world"]);
        assert_eq!(caret.offset, 0);
        let back = s.backspace_at_start(&caret.block).unwrap();
        assert_eq!(texts(&s), ["hello world"]);
        assert_eq!(
            back,
            Caret {
                block: first,
                offset: 5
            }
        );
    }

    #[test]
    fn enter_on_an_empty_list_item_leaves_the_list() {
        let mut s = session(&[("one", ParaStyle::Bullet)]);
        let first = ids(&s)[0].clone();
        let next = s.split(&first, 3).unwrap();
        assert_eq!(s.text(&next.block).unwrap().style, ParaStyle::Bullet);
        s.split(&next.block, 0).unwrap();
        assert_eq!(s.text(&next.block).unwrap().style, ParaStyle::Body);
        assert_eq!(s.doc().blocks.len(), 2);
    }

    #[test]
    fn backspace_peels_style_before_joining() {
        let mut s = session(&[("a", ParaStyle::Body), ("b", ParaStyle::Checklist)]);
        let b = ids(&s)[1].clone();
        s.indent(&b, 1);
        s.backspace_at_start(&b);
        assert_eq!(s.text(&b).unwrap().style, ParaStyle::Body);
        s.backspace_at_start(&b);
        assert_eq!(s.text(&b).unwrap().indent, 0);
        s.backspace_at_start(&b);
        assert_eq!(texts(&s), ["ab"]);
    }

    #[test]
    fn typing_coalesces_and_undo_restores() {
        let mut s = session(&[("", ParaStyle::Body)]);
        let b = ids(&s)[0].clone();
        for (i, c) in "hello".chars().enumerate() {
            s.replace(&b, i as u32..i as u32, &c.to_string(), None);
        }
        s.split(&b, 5);
        assert_eq!(s.doc().blocks.len(), 2);
        s.undo();
        assert_eq!(texts(&s), ["hello"]);
        s.undo();
        assert_eq!(texts(&s), [""]);
        assert!(!s.can_undo());
        s.redo();
        s.redo();
        assert_eq!(texts(&s), ["hello", ""]);
    }

    #[test]
    fn heading_is_followed_by_body() {
        let mut s = session(&[("Title", ParaStyle::Heading1)]);
        let h = ids(&s)[0].clone();
        let next = s.split(&h, 5).unwrap();
        assert_eq!(s.text(&next.block).unwrap().style, ParaStyle::Body);
    }

    #[test]
    fn inserting_on_an_empty_line_replaces_it() {
        let mut s = session(&[("text", ParaStyle::Body), ("", ParaStyle::Body)]);
        let empty = ids(&s)[1].clone();
        let divider = s.insert(Some(&empty), Insert::Divider);
        assert_eq!(texts(&s), ["text", "#", ""]);
        assert_eq!(ids(&s)[1], divider);
        // The trailing line is there to type after the divider.
        assert!(s.doc().blocks.last().unwrap().as_text().is_some());
    }

    #[test]
    fn a_swipe_of_the_eraser_is_one_undo_step() {
        let mut s = session(&[("", ParaStyle::Body)]);
        let text_rev = s.blocks_revision();
        for i in 0..3 {
            let y = 100.0 + i as f32 * 50.0;
            s.add_stroke(Stroke::new(
                Tool::Pen,
                "ink",
                3.0,
                &[
                    Point {
                        x: 0.0,
                        y,
                        pressure: 0.5,
                    },
                    Point {
                        x: 100.0,
                        y,
                        pressure: 0.5,
                    },
                ],
            ));
        }
        s.end_gesture();
        let rev = s.ink_revision();
        for i in 0..3 {
            s.erase(50.0, 100.0 + i as f32 * 50.0, 5.0);
        }
        assert_eq!(s.ink().strokes.len(), 0);
        assert_ne!(s.ink_revision(), rev);
        // Writing and erasing leave the text to be shown as it was.
        assert_eq!(s.blocks_revision(), text_rev);
        s.undo();
        assert_eq!(s.ink().strokes.len(), 3);
        // Text is untouched by it all, and stays where it was.
        assert_eq!(texts(&s), [""]);
    }

    #[test]
    fn revisions_move_only_for_touched_blocks() {
        let mut s = session(&[("a", ParaStyle::Body), ("b", ParaStyle::Body)]);
        let [a, b] = [ids(&s)[0].clone(), ids(&s)[1].clone()];
        let (ra, rb, all) = (s.revision(&a), s.revision(&b), s.blocks_revision());
        s.replace(&a, 1..1, "x", None);
        assert_ne!(s.revision(&a), ra);
        assert_eq!(s.revision(&b), rb);
        assert_ne!(s.blocks_revision(), all);
    }

    #[test]
    fn pasting_lines_makes_blocks() {
        let mut s = session(&[("ab", ParaStyle::Bullet)]);
        let b = ids(&s)[0].clone();
        let caret = s.paste(&b, 1..1, "1\r\n2\n3").unwrap();
        assert_eq!(texts(&s), ["a1", "2", "3b"]);
        assert_eq!(caret.offset, 1);
        assert!(
            s.doc()
                .blocks
                .iter()
                .all(|b| b.as_text().unwrap().style == ParaStyle::Bullet)
        );
        s.undo();
        assert_eq!(texts(&s), ["ab"]);
    }

    #[test]
    fn newlines_never_enter_a_block() {
        let mut s = session(&[("", ParaStyle::Body)]);
        let b = ids(&s)[0].clone();
        s.set_text(&b, "a\nb", None);
        assert_eq!(texts(&s), ["a b"]);
    }
}
