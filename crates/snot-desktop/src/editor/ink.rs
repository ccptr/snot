//! Handwriting on the page: the stroke being written and the eraser. The
//! finished strokes are drawn into tiles, in [`super::tiles`], so a frame
//! draws one stroke as a shape however much is already on the page.

use std::cell::RefCell;
use std::rc::Rc;

use gpui::{Bounds, Hsla, Path, PathBuilder, Pixels, Point, Window, point, px};
use snot_core::editor::EditSession;
use snot_core::ink;
use snot_core::page::PAPER_SPACING;
use snot_core::{Paper, Stroke, Tool, color};

use super::tiles::{Layer, Tiles};
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

    /// Nib sizes offered for the tool, in page units.
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
    pub tool: Tool,
    pub color: String,
    pub width: f32,
    pub points: Vec<ink::Point>,
}

/// The stroke being written, ready to paint: tessellated in pixels from
/// the page's top left.
pub struct Painted {
    path: Option<Path<Pixels>>,
    color: Hsla,
    layer: Layer,
}

pub struct InkState {
    pub pens: Pens,
    pub live: Option<Live>,
    /// An eraser swipe is under way.
    pub erasing: bool,
    /// Where the eraser is, for its outline, in window pixels.
    pub eraser_at: Option<Point<Pixels>>,
    pub tiles: Rc<RefCell<Tiles>>,
}

impl Default for InkState {
    fn default() -> Self {
        InkState {
            pens: Pens {
                color: ["ink".into(), "blue".into(), "yellow".into()],
                width: [4.0, 9.0, 28.0, 16.0],
            },
            live: None,
            erasing: false,
            eraser_at: None,
            tiles: Rc::default(),
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

    pub fn begin(&mut self, tool: InkTool, at: [f32; 2]) {
        if let Some(t) = tool.stroke_tool() {
            self.live = Some(Live {
                tool: t,
                color: self.color(tool).to_string(),
                width: self.width(tool),
                points: vec![ink::Point {
                    x: at[0],
                    y: at[1],
                    pressure: 0.5,
                }],
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
        if std::mem::take(&mut self.erasing) {
            session.end_gesture();
            return false;
        }
        let Some(live) = self.live.take() else {
            return false;
        };
        session.add_stroke(Stroke::new(live.tool, live.color, live.width, &live.points))
    }

    /// The stroke being written, outlined and placed for this frame.
    pub fn live_painted(&self, scale: f32, dark: bool) -> Option<Painted> {
        let live = self.live.as_ref()?;
        Some(painted(
            &ink::outline(&live.points, live.tool, live.width),
            live.tool,
            &live.color,
            scale,
            dark,
        ))
    }
}

fn painted(outline: &[[f32; 2]], tool: Tool, token: &str, scale: f32, dark: bool) -> Painted {
    Painted {
        path: tessellate(outline, scale),
        color: argb(color::ink(token, dark)).opacity(tool.alpha()),
        layer: Layer::of(tool),
    }
}

fn tessellate(outline: &[[f32; 2]], scale: f32) -> Option<Path<Pixels>> {
    if outline.len() < 3 {
        return None;
    }
    let mut builder =
        PathBuilder::fill().with_style(gpui::PathStyle::Fill(gpui::FillOptions::non_zero()));
    for (i, [x, y]) in outline.iter().enumerate() {
        let pt = point(px(x * scale), px(y * scale));
        if i == 0 {
            builder.move_to(pt);
        } else {
            builder.line_to(pt);
        }
    }
    builder.close();
    builder.build().ok()
}

/// Window position to page units, for a page whose top left is `origin`.
pub fn to_page(origin: Point<Pixels>, scale: f32, pos: Point<Pixels>) -> [f32; 2] {
    [
        f32::from(pos.x - origin.x) / scale,
        f32::from(pos.y - origin.y) / scale,
    ]
}

/// Paints the paper over the visible part of the page.
pub fn paint_paper(
    origin: Point<Pixels>,
    width: Pixels,
    scale: f32,
    paper: Paper,
    theme: &Theme,
    window: &mut Window,
) {
    if paper == Paper::Plain {
        return;
    }
    let visible = window.content_mask().bounds;
    let rule = theme.paper_rule;
    let step = PAPER_SPACING * scale;
    // Rows from the first one in view to the last.
    let first = ((f32::from(visible.top() - origin.y) / step).floor()).max(1.0);
    let last = (f32::from(visible.bottom() - origin.y) / step).ceil();
    let row = |i: f32| origin.y + px(i * step);
    let mut i = first;
    match paper {
        Paper::Plain => {}
        Paper::Lined | Paper::Grid => {
            while i <= last {
                window.paint_quad(gpui::fill(
                    Bounds::new(point(origin.x, row(i)), gpui::size(width, px(1.))),
                    rule,
                ));
                i += 1.0;
            }
            if paper == Paper::Grid {
                let top = visible.top().max(origin.y);
                let mut x = step;
                while x < f32::from(width) {
                    window.paint_quad(gpui::fill(
                        Bounds::new(
                            point(origin.x + px(x), top),
                            gpui::size(px(1.), visible.bottom() - top),
                        ),
                        rule,
                    ));
                    x += step;
                }
            }
        }
        Paper::Dotted => {
            let dot = rule.opacity(rule.a * 2.5);
            let r = (1.6 * scale).max(1.2);
            while i <= last {
                let mut x = step;
                while x < f32::from(width) {
                    window.paint_quad(
                        gpui::fill(
                            Bounds::new(
                                point(origin.x + px(x - r), row(i) - px(r)),
                                gpui::size(px(2. * r), px(2. * r)),
                            ),
                            dot,
                        )
                        .corner_radii(px(r)),
                    );
                    x += step;
                }
                i += 1.0;
            }
        }
    }
}

/// Paints the handwriting in view: the highlighter, then the rest, each
/// with the stroke being written among them if it belongs there.
pub fn paint_ink(
    origin: Point<Pixels>,
    tiles: &mut Tiles,
    live: Option<Painted>,
    window: &mut Window,
) {
    let mut live = live;
    for layer in [Layer::Under, Layer::Over] {
        tiles.paint(layer, origin, window);
        if let Some(p) = live.take_if(|p| p.layer == layer)
            && let Some(mut path) = p.path
        {
            path.bounds.origin += origin;
            for v in &mut path.vertices {
                v.xy_position += origin;
            }
            window.paint_path(path, p.color);
        }
    }
}
