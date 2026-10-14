//! The document model.
//!
//! A note body is a flat list of blocks. Each text block is one paragraph —
//! a plain string plus a set of style spans over it — so a native editor can
//! give every block its own text field and never has to parse markup. Ink
//! and pictures are blocks of their own that sit in the same flow.
//!
//! Every offset in this module counts Unicode scalar values (`char`s), not
//! bytes and not UTF-16 units. That is the one unit every platform can
//! convert to cheaply; front-ends translate at their edge.

use std::ops::Range;
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::ink::InkBlock;

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Doc {
    pub blocks: Vec<Block>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Block {
    pub id: String,
    #[serde(flatten)]
    pub content: Content,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum Content {
    Text(TextBlock),
    Divider,
    Image(ImageBlock),
    /// Ink is reference-counted so that an undo snapshot of a note full of
    /// handwriting costs a pointer, not a copy of every stroke.
    Ink(Arc<InkBlock>),
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct TextBlock {
    #[serde(default)]
    pub style: ParaStyle,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub indent: u8,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub checked: bool,
    #[serde(default)]
    pub text: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub spans: Vec<Span>,
}

fn is_zero(v: &u8) -> bool {
    *v == 0
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ImageBlock {
    /// Content hash of the attachment holding the picture.
    pub attachment: String,
    /// Height over width, so the block can reserve its space before the
    /// picture has been decoded.
    pub aspect: f32,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub caption: String,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ParaStyle {
    #[default]
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

impl ParaStyle {
    pub fn is_list(self) -> bool {
        matches!(self, Self::Bullet | Self::Numbered | Self::Checklist)
    }

    pub const ALL: [ParaStyle; 9] = [
        Self::Body,
        Self::Heading1,
        Self::Heading2,
        Self::Heading3,
        Self::Bullet,
        Self::Numbered,
        Self::Checklist,
        Self::Quote,
        Self::Code,
    ];
}

/// One inline style applied over `start..end`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Span {
    pub start: u32,
    pub end: u32,
    pub mark: Mark,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Mark {
    Bold,
    Italic,
    Underline,
    Strike,
    /// A palette token (see [`crate::color`]).
    Highlight(String),
    Color(String),
}

impl Mark {
    /// Marks of the same kind exclude each other: a character has one text
    /// colour and one highlight, not several.
    fn same_kind(&self, other: &Mark) -> bool {
        std::mem::discriminant(self) == std::mem::discriminant(other)
    }
}

/// A maximal stretch of text whose style does not change, ready to draw.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Run {
    pub start: u32,
    pub end: u32,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub strike: bool,
    pub highlight: Option<String>,
    pub color: Option<String>,
}

pub fn new_block_id() -> String {
    uuid::Uuid::new_v4().simple().to_string()[..12].to_string()
}

pub fn char_len(s: &str) -> u32 {
    s.chars().count() as u32
}

/// Byte offset of the `ch`-th char, clamped to the end of the string.
pub fn byte_of(s: &str, ch: u32) -> usize {
    s.char_indices()
        .nth(ch as usize)
        .map(|(b, _)| b)
        .unwrap_or(s.len())
}

impl Block {
    pub fn text(text: impl Into<String>) -> Self {
        Self::styled(ParaStyle::Body, text)
    }

    pub fn styled(style: ParaStyle, text: impl Into<String>) -> Self {
        Block {
            id: new_block_id(),
            content: Content::Text(TextBlock {
                style,
                text: text.into(),
                ..Default::default()
            }),
        }
    }

    pub fn as_text(&self) -> Option<&TextBlock> {
        match &self.content {
            Content::Text(t) => Some(t),
            _ => None,
        }
    }

    pub fn as_text_mut(&mut self) -> Option<&mut TextBlock> {
        match &mut self.content {
            Content::Text(t) => Some(t),
            _ => None,
        }
    }
}

impl TextBlock {
    pub fn len(&self) -> u32 {
        char_len(&self.text)
    }

    pub fn is_empty(&self) -> bool {
        self.text.is_empty()
    }

    /// Marks covering the character at `at`, if there is one.
    fn marks_at_char(&self, at: u32) -> Vec<Mark> {
        let mut marks: Vec<Mark> = self
            .spans
            .iter()
            .filter(|s| s.start <= at && at < s.end)
            .map(|s| s.mark.clone())
            .collect();
        marks.sort();
        marks.dedup();
        marks
    }

    /// The marks newly typed text at `at` takes on: those of the character
    /// before it, the way every word processor carries bold on as you type.
    pub fn marks_for_insert(&self, at: u32) -> Vec<Mark> {
        if at == 0 {
            return Vec::new();
        }
        self.marks_at_char(at - 1)
    }

    /// Marks applied to every character of `range`; for an empty range, the
    /// marks typing there would pick up. This is what a toolbar shows as
    /// pressed.
    pub fn marks_in(&self, range: Range<u32>) -> Vec<Mark> {
        if range.is_empty() {
            return self.marks_for_insert(range.start);
        }
        let mut out = self.marks_at_char(range.start);
        out.retain(|m| self.covered(range.clone(), m));
        out
    }

    fn covered(&self, range: Range<u32>, mark: &Mark) -> bool {
        // Spans are normalised, so same-mark spans never overlap or touch;
        // the range is covered only if a single span holds all of it.
        self.spans
            .iter()
            .any(|s| &s.mark == mark && s.start <= range.start && s.end >= range.end)
    }

    /// Replaces `range` with `insert`. The inserted text takes `marks` if
    /// given, and otherwise inherits from the character before it.
    pub fn replace(&mut self, range: Range<u32>, insert: &str, marks: Option<&[Mark]>) {
        let len = self.len();
        let start = range.start.min(len);
        let end = range.end.clamp(start, len);
        let inherit = match marks {
            Some(m) => m.to_vec(),
            // Replacing a selection keeps the style of what was selected.
            None if end > start => self.marks_at_char(start),
            None => self.marks_for_insert(start),
        };

        let (b0, b1) = (byte_of(&self.text, start), byte_of(&self.text, end));
        self.text.replace_range(b0..b1, insert);

        let removed = end - start;
        let added = char_len(insert);
        let collapse = |p: u32| {
            if p <= start {
                p
            } else if p >= end {
                p - removed
            } else {
                start
            }
        };
        let mut spans = Vec::with_capacity(self.spans.len() + inherit.len());
        for span in self.spans.drain(..) {
            let (s, e) = (collapse(span.start), collapse(span.end));
            if s >= e {
                continue;
            }
            // Open a gap at `start` for the inserted text; whatever marks it
            // should carry are added back below, and normalising rejoins the
            // pieces where the mark continues across it.
            if s < start && e > start {
                spans.push(Span {
                    start: s,
                    end: start,
                    mark: span.mark.clone(),
                });
                spans.push(Span {
                    start: start + added,
                    end: e + added,
                    mark: span.mark,
                });
            } else if s >= start {
                spans.push(Span {
                    start: s + added,
                    end: e + added,
                    mark: span.mark,
                });
            } else {
                spans.push(Span {
                    start: s,
                    end: e,
                    mark: span.mark,
                });
            }
        }
        if added > 0 {
            for mark in inherit {
                spans.push(Span {
                    start,
                    end: start + added,
                    mark,
                });
            }
        }
        self.spans = spans;
        self.normalize();
    }

    /// Brings the text to `new` by diffing against the current text. This is
    /// for editors that hand back the whole field after each change, like
    /// Compose: the common prefix and suffix are kept, so their styles stay
    /// put, and only the middle is treated as an edit.
    ///
    /// Returns the edited range in the new text.
    pub fn set_text(&mut self, new: &str, marks: Option<&[Mark]>) -> Range<u32> {
        let old: Vec<char> = self.text.chars().collect();
        let new_chars: Vec<char> = new.chars().collect();
        let prefix = old
            .iter()
            .zip(&new_chars)
            .take_while(|(a, b)| a == b)
            .count();
        let max_suffix = old.len().min(new_chars.len()) - prefix;
        let suffix = old
            .iter()
            .rev()
            .zip(new_chars.iter().rev())
            .take(max_suffix)
            .take_while(|(a, b)| a == b)
            .count();
        let removed = prefix as u32..(old.len() - suffix) as u32;
        let inserted: String = new_chars[prefix..new_chars.len() - suffix].iter().collect();
        self.replace(removed, &inserted, marks);
        prefix as u32..(new_chars.len() - suffix) as u32
    }

    /// Turns `mark` off over `range` if every character there already has
    /// it, and on otherwise. Colours replace whatever colour was there.
    pub fn toggle_mark(&mut self, range: Range<u32>, mark: &Mark) {
        let range = range.start.min(self.len())..range.end.min(self.len());
        if range.is_empty() {
            return;
        }
        let on = self.covered(range.clone(), mark);
        self.clear_kind(range.clone(), mark);
        if !on {
            self.spans.push(Span {
                start: range.start,
                end: range.end,
                mark: mark.clone(),
            });
        }
        self.normalize();
    }

    /// Removes `mark`'s kind from `range`, splitting spans that cross it.
    fn clear_kind(&mut self, range: Range<u32>, mark: &Mark) {
        let mut out = Vec::with_capacity(self.spans.len() + 1);
        for span in self.spans.drain(..) {
            if !span.mark.same_kind(mark) || span.end <= range.start || span.start >= range.end {
                out.push(span);
                continue;
            }
            if span.start < range.start {
                out.push(Span {
                    start: span.start,
                    end: range.start,
                    mark: span.mark.clone(),
                });
            }
            if span.end > range.end {
                out.push(Span {
                    start: range.end,
                    end: span.end,
                    mark: span.mark,
                });
            }
        }
        self.spans = out;
    }

    pub fn clear_marks(&mut self, range: Range<u32>) {
        for mark in [Mark::Bold, Mark::Italic, Mark::Underline, Mark::Strike] {
            self.clear_kind(range.clone(), &mark);
        }
        self.clear_kind(range.clone(), &Mark::Highlight(String::new()));
        self.clear_kind(range, &Mark::Color(String::new()));
        self.normalize();
    }

    /// Clamps, sorts and merges spans so that equal marks never overlap or
    /// abut. Everything else relies on that invariant.
    pub fn normalize(&mut self) {
        let len = self.len();
        self.spans.retain_mut(|s| {
            s.end = s.end.min(len);
            s.start < s.end
        });
        self.spans
            .sort_by(|a, b| a.mark.cmp(&b.mark).then(a.start.cmp(&b.start)));
        let mut merged: Vec<Span> = Vec::with_capacity(self.spans.len());
        for span in self.spans.drain(..) {
            if let Some(last) = merged.last_mut()
                && last.mark == span.mark
                && span.start <= last.end
            {
                last.end = last.end.max(span.end);
                continue;
            }
            merged.push(span);
        }
        // A position has one colour and one highlight; where two coloured
        // spans overlap (only possible in hand-written or imported data),
        // the later one wins.
        self.spans = merged;
        self.spans
            .sort_by(|a, b| a.start.cmp(&b.start).then(a.mark.cmp(&b.mark)));
    }

    /// Splits the text into maximal runs of one style, covering it whole.
    pub fn runs(&self) -> Vec<Run> {
        let len = self.len();
        let mut cuts: Vec<u32> = vec![0, len];
        for s in &self.spans {
            cuts.push(s.start);
            cuts.push(s.end);
        }
        cuts.sort_unstable();
        cuts.dedup();
        let mut runs: Vec<Run> = Vec::new();
        for w in cuts.windows(2) {
            let (start, end) = (w[0], w[1]);
            if start >= end {
                continue;
            }
            let mut run = Run {
                start,
                end,
                ..Default::default()
            };
            for s in self
                .spans
                .iter()
                .filter(|s| s.start <= start && s.end >= end)
            {
                match &s.mark {
                    Mark::Bold => run.bold = true,
                    Mark::Italic => run.italic = true,
                    Mark::Underline => run.underline = true,
                    Mark::Strike => run.strike = true,
                    Mark::Highlight(c) => run.highlight = Some(c.clone()),
                    Mark::Color(c) => run.color = Some(c.clone()),
                }
            }
            match runs.last_mut() {
                Some(prev) if same_style(prev, &run) => prev.end = end,
                _ => runs.push(run),
            }
        }
        if runs.is_empty() {
            runs.push(Run::default());
        }
        runs
    }

    /// Cuts the block at `at`, keeping the head and returning the tail.
    pub fn split_off(&mut self, at: u32) -> TextBlock {
        let at = at.min(self.len());
        let byte = byte_of(&self.text, at);
        let tail_text = self.text.split_off(byte);
        let mut tail_spans = Vec::new();
        for s in &self.spans {
            if s.end > at {
                tail_spans.push(Span {
                    start: s.start.saturating_sub(at),
                    end: s.end - at,
                    mark: s.mark.clone(),
                });
            }
        }
        self.normalize();
        let mut tail = TextBlock {
            style: self.style,
            indent: self.indent,
            checked: false,
            text: tail_text,
            spans: tail_spans,
        };
        tail.normalize();
        tail
    }

    /// Appends `other`'s text and styles to this block.
    pub fn append(&mut self, other: &TextBlock) {
        let offset = self.len();
        self.text.push_str(&other.text);
        self.spans.extend(other.spans.iter().map(|s| Span {
            start: s.start + offset,
            end: s.end + offset,
            mark: s.mark.clone(),
        }));
        self.normalize();
    }
}

fn same_style(a: &Run, b: &Run) -> bool {
    a.bold == b.bold
        && a.italic == b.italic
        && a.underline == b.underline
        && a.strike == b.strike
        && a.highlight == b.highlight
        && a.color == b.color
}

impl Doc {
    pub fn new() -> Self {
        Doc {
            blocks: vec![Block::text("")],
        }
    }

    pub fn index_of(&self, id: &str) -> Option<usize> {
        self.blocks.iter().position(|b| b.id == id)
    }

    pub fn block(&self, id: &str) -> Option<&Block> {
        self.blocks.iter().find(|b| b.id == id)
    }

    pub fn block_mut(&mut self, id: &str) -> Option<&mut Block> {
        self.blocks.iter_mut().find(|b| b.id == id)
    }

    /// The number shown before each numbered-list block, `None` elsewhere.
    /// Consecutive numbered blocks at one indent count up together; a block
    /// at a shallower indent, or any non-list block, restarts the count.
    pub fn list_numbers(&self) -> Vec<Option<u32>> {
        let mut counters: Vec<u32> = Vec::new();
        let mut out = Vec::with_capacity(self.blocks.len());
        for block in &self.blocks {
            match block.as_text() {
                Some(t) if t.style.is_list() => {
                    let depth = t.indent as usize;
                    counters.truncate(depth + 1);
                    counters.resize(depth + 1, 0);
                    if t.style == ParaStyle::Numbered {
                        counters[depth] += 1;
                        out.push(Some(counters[depth]));
                    } else {
                        counters[depth] = 0;
                        out.push(None);
                    }
                }
                _ => {
                    counters.clear();
                    out.push(None);
                }
            }
        }
        out
    }

    /// The plain text of the note, one line per text block, as indexed for
    /// search.
    pub fn plain_text(&self) -> String {
        let mut out = String::new();
        for block in &self.blocks {
            match &block.content {
                Content::Text(t) if !t.text.trim().is_empty() => {
                    out.push_str(t.text.trim_end());
                    out.push('\n');
                }
                Content::Image(img) if !img.caption.is_empty() => {
                    out.push_str(&img.caption);
                    out.push('\n');
                }
                _ => {}
            }
        }
        out.truncate(out.trim_end().len());
        out
    }

    pub fn has_ink(&self) -> bool {
        self.blocks
            .iter()
            .any(|b| matches!(&b.content, Content::Ink(i) if !i.strokes.is_empty()))
    }

    pub fn has_image(&self) -> bool {
        self.blocks
            .iter()
            .any(|b| matches!(b.content, Content::Image(_)))
    }

    /// Every attachment the note refers to.
    pub fn attachments(&self) -> Vec<String> {
        self.blocks
            .iter()
            .filter_map(|b| match &b.content {
                Content::Image(img) => Some(img.attachment.clone()),
                _ => None,
            })
            .collect()
    }

    /// Repairs what a document must always satisfy: at least one block,
    /// unique ids, and a text block to type into at the end, so the caret
    /// can always land after a drawing or a picture.
    pub fn sanitize(&mut self) {
        let mut seen = std::collections::HashSet::new();
        for block in &mut self.blocks {
            if block.id.is_empty() || !seen.insert(block.id.clone()) {
                block.id = new_block_id();
                seen.insert(block.id.clone());
            }
            if let Some(t) = block.as_text_mut() {
                t.normalize();
            }
        }
        if !matches!(self.blocks.last(), Some(b) if b.as_text().is_some()) {
            self.blocks.push(Block::text(""));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tb(text: &str) -> TextBlock {
        TextBlock {
            text: text.into(),
            ..Default::default()
        }
    }

    #[test]
    fn typing_after_bold_stays_bold() {
        let mut t = tb("hello");
        t.toggle_mark(0..5, &Mark::Bold);
        t.replace(5..5, "!", None);
        assert_eq!(
            t.spans,
            vec![Span {
                start: 0,
                end: 6,
                mark: Mark::Bold
            }]
        );
        // At the very start nothing is inherited.
        t.replace(0..0, ">", None);
        assert_eq!(
            t.spans,
            vec![Span {
                start: 1,
                end: 7,
                mark: Mark::Bold
            }]
        );
    }

    #[test]
    fn explicit_marks_cut_a_hole() {
        let mut t = tb("abcd");
        t.toggle_mark(0..4, &Mark::Bold);
        t.replace(2..2, "X", Some(&[]));
        assert_eq!(t.text, "abXcd");
        assert_eq!(
            t.spans,
            vec![
                Span {
                    start: 0,
                    end: 2,
                    mark: Mark::Bold
                },
                Span {
                    start: 3,
                    end: 5,
                    mark: Mark::Bold
                }
            ]
        );
    }

    #[test]
    fn deleting_collapses_spans() {
        let mut t = tb("one two three");
        t.toggle_mark(4..7, &Mark::Italic);
        t.replace(3..8, "", None);
        assert_eq!(t.text, "onethree");
        assert!(t.spans.is_empty());
    }

    #[test]
    fn set_text_diffs_the_middle() {
        let mut t = tb("the cat sat");
        t.toggle_mark(8..11, &Mark::Underline);
        let edited = t.set_text("the big cat sat", None);
        assert_eq!(edited, 4..8);
        assert_eq!(
            t.spans,
            vec![Span {
                start: 12,
                end: 15,
                mark: Mark::Underline
            }]
        );
    }

    #[test]
    fn toggling_twice_is_a_no_op() {
        let mut t = tb("abcdef");
        t.toggle_mark(1..3, &Mark::Bold);
        t.toggle_mark(2..5, &Mark::Bold);
        assert_eq!(
            t.spans,
            vec![Span {
                start: 1,
                end: 5,
                mark: Mark::Bold
            }]
        );
        t.toggle_mark(2..4, &Mark::Bold);
        assert_eq!(
            t.spans,
            vec![
                Span {
                    start: 1,
                    end: 2,
                    mark: Mark::Bold
                },
                Span {
                    start: 4,
                    end: 5,
                    mark: Mark::Bold
                }
            ]
        );
    }

    #[test]
    fn a_new_colour_replaces_the_old() {
        let mut t = tb("abcdef");
        t.toggle_mark(0..6, &Mark::Color("red".into()));
        t.toggle_mark(2..4, &Mark::Color("blue".into()));
        let runs = t.runs();
        assert_eq!(runs.len(), 3);
        assert_eq!(runs[1].color.as_deref(), Some("blue"));
        assert_eq!(runs[2].color.as_deref(), Some("red"));
    }

    #[test]
    fn runs_cover_multibyte_text() {
        let mut t = tb("héllo 👋 wörld");
        t.toggle_mark(6..7, &Mark::Bold);
        let runs = t.runs();
        assert_eq!(
            runs.iter()
                .map(|r| (r.start, r.end, r.bold))
                .collect::<Vec<_>>(),
            vec![(0, 6, false), (6, 7, true), (7, 13, false)]
        );
        t.replace(6..7, "🙂🙂", None);
        assert_eq!(t.text, "héllo 🙂🙂 wörld");
        assert_eq!(
            t.spans,
            vec![Span {
                start: 6,
                end: 8,
                mark: Mark::Bold
            }]
        );
    }

    #[test]
    fn split_and_append_round_trip() {
        let mut t = tb("hello world");
        t.toggle_mark(3..8, &Mark::Bold);
        let original = t.clone();
        let tail = t.split_off(5);
        assert_eq!(t.text, "hello");
        assert_eq!(tail.text, " world");
        assert_eq!(
            tail.spans,
            vec![Span {
                start: 0,
                end: 3,
                mark: Mark::Bold
            }]
        );
        t.append(&tail);
        assert_eq!(t, original);
    }

    #[test]
    fn list_numbers_restart_and_nest() {
        let mut doc = Doc { blocks: vec![] };
        let mut push = |style, indent| {
            let mut b = Block::styled(style, "x");
            b.as_text_mut().unwrap().indent = indent;
            doc.blocks.push(b);
        };
        push(ParaStyle::Numbered, 0);
        push(ParaStyle::Numbered, 0);
        push(ParaStyle::Numbered, 1);
        push(ParaStyle::Numbered, 1);
        push(ParaStyle::Numbered, 0);
        push(ParaStyle::Body, 0);
        push(ParaStyle::Numbered, 0);
        assert_eq!(
            doc.list_numbers(),
            vec![Some(1), Some(2), Some(1), Some(2), Some(3), None, Some(1)]
        );
    }
}
