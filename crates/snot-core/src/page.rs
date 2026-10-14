//! The page a note is written on.
//!
//! A note is one page, [`PAGE_WIDTH`] units wide and as long as its
//! contents. Text and pictures flow down it; handwriting lies on top of
//! the whole page, at fixed page coordinates, so a word circled on the
//! phone is still circled on the desktop. For that to hold, both front-ends
//! lay text out in page units with the metrics here and scale the whole
//! page to the width they show it at — the page zooms, it never reflows.
//!
//! Every size in this module is in page units. A phone shows about 0.4
//! density-independent pixels per unit, so body text of 40 units reads at
//! the usual 16dp.

use crate::doc::{Block, Content, ParaStyle};

/// Width of every page.
pub const PAGE_WIDTH: f32 = 1000.0;
/// Space between the page edges and its text.
pub const MARGIN: f32 = 48.0;
/// How far one indent level steps in.
pub const INDENT: f32 = 60.0;
/// Width of the gutter holding a bullet, number or checkbox.
pub const PREFIX: f32 = 70.0;
/// Side of a checklist box.
pub const CHECKBOX: f32 = 48.0;
/// Diameter of a bullet.
pub const BULLET: f32 = 15.0;
/// Padding inside a code block, across and down.
pub const CODE_PAD_X: f32 = 25.0;
pub const CODE_PAD_Y: f32 = 10.0;
/// The bar down the side of a quote, and the gap after it.
pub const QUOTE_BAR: f32 = 8.0;
pub const QUOTE_GAP: f32 = 30.0;
/// Space above and below a picture.
pub const IMAGE_GAP: f32 = 20.0;
pub const CAPTION_SIZE: f32 = 35.0;
/// Height of a divider's row.
pub const DIVIDER_HEIGHT: f32 = 70.0;
/// Blank page kept below the last line and the lowest writing, so there is
/// always room to carry on; writing into it makes the page longer.
pub const ROOM: f32 = 600.0;
/// Ruling interval for lined, grid and dotted paper: one line of body text.
pub const PAPER_SPACING: f32 = BODY_SIZE * LINE_HEIGHT;

const BODY_SIZE: f32 = 40.0;
/// Line height as a multiple of the font size.
pub const LINE_HEIGHT: f32 = 1.45;

/// How a paragraph style sets its text.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TextMetrics {
    /// Font size.
    pub size: f32,
    pub line_height: f32,
    /// Space above and below the paragraph.
    pub top: f32,
    pub bottom: f32,
    pub semibold: bool,
    pub mono: bool,
}

pub fn text_metrics(style: ParaStyle) -> TextMetrics {
    let (size, top, bottom, semibold) = match style {
        ParaStyle::Heading1 => (67.5, 45.0, 15.0, true),
        ParaStyle::Heading2 => (54.0, 35.0, 10.0, true),
        ParaStyle::Heading3 => (45.0, 25.0, 7.5, true),
        ParaStyle::Code => (36.0, 7.5, 7.5, false),
        _ => (BODY_SIZE, 7.5, 7.5, false),
    };
    TextMetrics {
        size,
        line_height: (size * LINE_HEIGHT).round(),
        top,
        bottom,
        semibold,
        mono: style == ParaStyle::Code,
    }
}

/// Roughly how tall a block lays out, for placing things on a page without
/// a front-end to measure it. It counts characters at an average width, so
/// it is close for prose and only a guide for anything else.
pub fn estimated_height(block: &Block) -> f32 {
    let column = PAGE_WIDTH - 2.0 * MARGIN;
    match &block.content {
        Content::Text(t) => {
            let m = text_metrics(t.style);
            let mut width = column - INDENT * t.indent as f32;
            if t.style.is_list() {
                width -= PREFIX;
            }
            let (pad, quote) = match t.style {
                ParaStyle::Code => (2.0 * CODE_PAD_Y, 2.0 * CODE_PAD_X),
                ParaStyle::Quote => (0.0, QUOTE_BAR + QUOTE_GAP),
                _ => (0.0, 0.0),
            };
            let per_line = ((width - quote) / (m.size * 0.5)).max(1.0);
            let lines = (t.len() as f32 / per_line).ceil().max(1.0);
            m.top + m.bottom + pad + lines * m.line_height
        }
        Content::Divider => DIVIDER_HEIGHT,
        Content::Image(img) => {
            let caption = if img.caption.is_empty() {
                0.0
            } else {
                CAPTION_SIZE * LINE_HEIGHT + 15.0
            };
            2.0 * IMAGE_GAP + column * img.aspect + caption
        }
    }
}
