//! Drawing tools: the stroke being written, the eraser, and a cache of
//! finished stroke outlines so a page of handwriting is not re-outlined on
//! every frame.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use gpui::{Bounds, Hsla, PathBuilder, Pixels, Point, Window, point, px};
use snot_core::editor::EditSession;
use snot_core::ink::{self, INK_WIDTH, PAPER_SPACING};
use snot_core::{InkBlock, Paper, Stroke, Tool, color};

use crate::theme::{Theme, argb};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InkTool {
    Type,
    Pen,
    Marker,
    Highlighter,
    Eraser,
}

impl InkTool {
    pub fn stroke_tool(self) -> Option<Tool> {
        match self {
            InkTool::Pen => Some(Tool::Pen),
            InkTool::Marker => Some(Tool::Marker),
            InkTool::Highlighter => Some(Tool::Highlighter),
            InkTool::Type | InkTool::Eraser => None,
        }
    }

    pub fn draws(self) -> bool {
        self != InkTool::Type
    }

    /// Nib sizes offered for the tool, in ink units.
    pub fn widths(self) -> &'static [f32] {
        match self {
            InkTool::Pen => &[2.0, 4.0, 7.0, 12.0],
            InkTool::Marker => &[5.0, 9.0, 15.0, 24.0],
            InkTool::Highlighter => &[18.0, 28.0, 42.0],
            InkTool::Eraser => &[8.0, 16.0, 32.0],
            InkTool::Type => &[],
        }
    }
}

/// Colour and nib size remembered per tool. The eraser's width is its
/// radius.
pub struct Pens {
    pub color: [String; 3],
    pub width: [f32; 4],
}

fn slot(tool: InkTool) -> usize {
    match tool {
        InkTool::Pen | InkTool::Type => 0,
        InkTool::Marker => 1,
        InkTool::Highlighter => 2,
        InkTool::Eraser => 3,
    }
}

pub struct Live {
    pub block: String,
    pub tool: Tool,
    pub color: String,
    pub width: f32,
    pub points: Vec<ink::Point>,
}

pub struct Painted {
    pub color: Hsla,
    pub outline: Vec<[f32; 2]>,
}

/// Outlines of a block's strokes, keyed by its revision and the theme.
type Cached = (u64, bool, Rc<Vec<Painted>>);

pub struct InkState {
    pub pens: Pens,
    pub live: Option<Live>,
    /// The block an eraser swipe is working on.
    pub erasing: Option<String>,
    /// Where the eraser is, for its outline, in window pixels.
    pub eraser_at: Option<Point<Pixels>>,
    cache: RefCell<HashMap<String, Cached>>,
}

impl Default for InkState {
    fn default() -> Self {
        InkState {
            pens: Pens {
                color: ["ink".into(), "blue".into(), "yellow".into()],
                width: [4.0, 9.0, 28.0, 16.0],
            },
            live: None,
            erasing: None,
            eraser_at: None,
            cache: RefCell::default(),
        }
    }
}

impl InkState {
    pub fn color(&self, tool: InkTool) -> &str {
        &self.pens.color[slot(tool).min(2)]
    }

    pub fn set_color(&mut self, tool: InkTool, color: &str) {
        self.pens.color[slot(tool).min(2)] = color.to_string();
    }

    pub fn width(&self, tool: InkTool) -> f32 {
        self.pens.width[slot(tool)]
    }

    pub fn set_width(&mut self, tool: InkTool, w: f32) {
        self.pens.width[slot(tool)] = w;
    }

    pub fn begin(&mut self, block: &str, tool: InkTool, at: [f32; 2]) {
        let p = ink::Point {
            x: at[0],
            y: at[1],
            pressure: 0.5,
        };
        if let Some(t) = tool.stroke_tool() {
            self.live = Some(Live {
                block: block.into(),
                tool: t,
                color: self.color(tool).to_string(),
                width: self.width(tool),
                points: vec![p],
            });
        }
    }

    pub fn extend(&mut self, at: [f32; 2]) -> bool {
        let Some(live) = &mut self.live else {
            return false;
        };
        live.points.push(ink::Point {
            x: at[0],
            y: at[1],
            pressure: 0.5,
        });
        true
    }

    /// Ends whatever gesture is under way. True when the note changed.
    pub fn finish(&mut self, session: &mut EditSession) -> bool {
        self.eraser_at = None;
        if self.erasing.take().is_some() {
            session.end_gesture();
            return false;
        }
        let Some(live) = self.live.take() else {
            return false;
        };
        let stroke = Stroke::new(live.tool, live.color, live.width, &live.points);
        session.add_stroke(&live.block, stroke)
    }

    /// Outlines of a block's finished strokes, highlighter first, in the
    /// order they are painted.
    pub fn painted(
        &self,
        id: &str,
        revision: u64,
        block: &InkBlock,
        dark: bool,
    ) -> Rc<Vec<Painted>> {
        let mut cache = self.cache.borrow_mut();
        if let Some((rev, d, p)) = cache.get(id)
            && *rev == revision
            && *d == dark
        {
            return p.clone();
        }
        let ordered = block
            .strokes
            .iter()
            .filter(|s| s.tool == Tool::Highlighter)
            .chain(block.strokes.iter().filter(|s| s.tool != Tool::Highlighter));
        let painted: Vec<Painted> = ordered
            .map(|s| paint_of(s.tool, &s.color, s.outline(), dark))
            .collect();
        let painted = Rc::new(painted);
        cache.insert(id.to_string(), (revision, dark, painted.clone()));
        painted
    }

    pub fn live_painted(&self, id: &str, dark: bool) -> Option<Painted> {
        let live = self.live.as_ref().filter(|l| l.block == id)?;
        Some(paint_of(
            live.tool,
            &live.color,
            ink::outline(&live.points, live.tool, live.width),
            dark,
        ))
    }
}

fn paint_of(tool: Tool, token: &str, outline: Vec<[f32; 2]>, dark: bool) -> Painted {
    let color = argb(color::ink(token, dark)).opacity(tool.alpha());
    Painted { color, outline }
}

/// Window position to ink units inside a block shown at `bounds`.
pub fn to_ink(bounds: Bounds<Pixels>, pos: Point<Pixels>) -> [f32; 2] {
    let scale = INK_WIDTH / f32::from(bounds.size.width).max(1.0);
    [
        f32::from(pos.x - bounds.left()) * scale,
        f32::from(pos.y - bounds.top()) * scale,
    ]
}

pub fn scale(bounds: Bounds<Pixels>) -> f32 {
    f32::from(bounds.size.width) / INK_WIDTH
}

/// Paints a block: its paper, then its strokes, then the live stroke.
pub fn paint(
    bounds: Bounds<Pixels>,
    block: &InkBlock,
    painted: &[Painted],
    live: Option<&Painted>,
    theme: &Theme,
    window: &mut Window,
) {
    let s = scale(bounds);
    let origin = bounds.origin;
    let rule = theme.paper_rule;
    let step = PAPER_SPACING * s;
    match block.paper {
        Paper::Plain => {}
        Paper::Lined => {
            let mut y = step * 2.0;
            while y < f32::from(bounds.size.height) {
                window.paint_quad(gpui::fill(
                    Bounds::new(
                        point(origin.x, origin.y + px(y)),
                        gpui::size(bounds.size.width, px(1.)),
                    ),
                    rule,
                ));
                y += step;
            }
        }
        Paper::Grid => {
            let mut y = step;
            while y < f32::from(bounds.size.height) {
                window.paint_quad(gpui::fill(
                    Bounds::new(
                        point(origin.x, origin.y + px(y)),
                        gpui::size(bounds.size.width, px(1.)),
                    ),
                    rule,
                ));
                y += step;
            }
            let mut x = step;
            while x < f32::from(bounds.size.width) {
                window.paint_quad(gpui::fill(
                    Bounds::new(
                        point(origin.x + px(x), origin.y),
                        gpui::size(px(1.), bounds.size.height),
                    ),
                    rule,
                ));
                x += step;
            }
        }
        Paper::Dotted => {
            let dot = rule.opacity(rule.a * 2.5);
            let r = (1.6 * s).max(1.2);
            let mut y = step;
            while y < f32::from(bounds.size.height) {
                let mut x = step;
                while x < f32::from(bounds.size.width) {
                    window.paint_quad(
                        gpui::fill(
                            Bounds::new(
                                point(origin.x + px(x - r), origin.y + px(y - r)),
                                gpui::size(px(2. * r), px(2. * r)),
                            ),
                            dot,
                        )
                        .corner_radii(px(r)),
                    );
                    x += step;
                }
                y += step;
            }
        }
    }
    for p in painted.iter().chain(live) {
        fill_outline(origin, s, p, window);
    }
}

fn fill_outline(origin: Point<Pixels>, s: f32, p: &Painted, window: &mut Window) {
    if p.outline.len() < 3 {
        return;
    }
    let mut builder =
        PathBuilder::fill().with_style(gpui::PathStyle::Fill(gpui::FillOptions::non_zero()));
    for (i, [x, y]) in p.outline.iter().enumerate() {
        let pt = point(origin.x + px(x * s), origin.y + px(y * s));
        if i == 0 {
            builder.move_to(pt);
        } else {
            builder.line_to(pt);
        }
    }
    builder.close();
    if let Ok(path) = builder.build() {
        window.paint_path(path, p.color);
    }
}
