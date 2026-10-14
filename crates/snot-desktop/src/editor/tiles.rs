//! Finished handwriting, drawn into tiles of pixels once and shown as
//! images from then on. A frame costs a few images however much is written
//! on the page; only the stroke being written is drawn as a shape.
//!
//! Highlighter goes under the other strokes, so it has a layer of tiles of
//! its own, and a highlighter stroke being written can be shown between
//! the two just where it will land.

use std::collections::hash_map::DefaultHasher;
use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::rc::Rc;
use std::sync::Arc;

use gpui::{Bounds, Pixels, Point, RenderImage, Window, point, px, size};
use smallvec::SmallVec;
use snot_core::ink::Rect;
use snot_core::{InkLayer, Stroke, Tool, color};
use tiny_skia::{FillRule, Paint, PathBuilder, Pixmap, Transform};

/// Side of a tile, in device pixels.
const TILE: u32 = 256;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Layer {
    /// Highlighter.
    Under,
    /// Everything else.
    Over,
}

impl Layer {
    pub fn of(tool: Tool) -> Layer {
        if tool == Tool::Highlighter {
            Layer::Under
        } else {
            Layer::Over
        }
    }
}

/// A stroke's outline and colour, worked out once.
struct Prepared {
    key: u64,
    outline: Vec<[f32; 2]>,
    bounds: Rect,
    color: tiny_skia::Color,
    layer: Layer,
}

struct Tile {
    pixmap: Pixmap,
    image: Arc<RenderImage>,
}

type TileId = (Layer, i32, i32);

#[derive(Default)]
pub struct Tiles {
    revision: Option<u64>,
    /// Device pixels per page unit.
    density: f32,
    dark: bool,
    /// Every stroke, in the order written.
    strokes: Vec<Rc<Prepared>>,
    /// Tiles drawn so far; `None` for one with nothing on it.
    tiles: HashMap<TileId, Option<Tile>>,
    /// Images replaced since the last frame, to free.
    dropped: Vec<Arc<RenderImage>>,
}

impl Tiles {
    /// Brings the tiles up to date with the handwriting, redrawing only
    /// those a changed stroke touches.
    pub fn sync(&mut self, ink: &InkLayer, revision: u64, density: f32, dark: bool) {
        if self.density != density || self.dark != dark {
            self.density = density;
            self.dark = dark;
            self.strokes.clear();
            self.clear();
        } else if self.revision == Some(revision) {
            return;
        }
        self.revision = Some(revision);

        let mut known: HashMap<u64, Rc<Prepared>> =
            self.strokes.iter().map(|p| (p.key, p.clone())).collect();
        let old = std::mem::take(&mut self.strokes);
        self.strokes = ink
            .strokes
            .iter()
            .map(|s| {
                let key = key_of(s);
                known
                    .remove(&key)
                    .unwrap_or_else(|| Rc::new(prepare(s, key, dark)))
            })
            .collect();

        // Strokes only added after the ones already there go on top of the
        // tiles as they are; anything else redraws the tiles it touches.
        let kept = old
            .iter()
            .zip(&self.strokes)
            .take_while(|(a, b)| a.key == b.key)
            .count();
        if kept == old.len() {
            for i in kept..self.strokes.len() {
                let p = self.strokes[i].clone();
                self.add_on_top(&p);
            }
        } else {
            // Strokes keep their order when others come and go, so only
            // where one came or went can look different.
            let now: HashSet<u64> = self.strokes.iter().map(|p| p.key).collect();
            let was: HashSet<u64> = old.iter().map(|p| p.key).collect();
            let changed: Vec<Rect> = old
                .iter()
                .filter(|p| !now.contains(&p.key))
                .chain(self.strokes.iter().filter(|p| !was.contains(&p.key)))
                .map(|p| p.bounds)
                .collect();
            for r in changed {
                for id in self.tiles_over(r) {
                    self.forget(id);
                }
            }
        }
    }

    /// Paints the tiles of one layer that are in view, for a page whose top
    /// left is `origin`.
    pub fn paint(&mut self, layer: Layer, origin: Point<Pixels>, window: &mut Window) {
        for image in self.dropped.drain(..) {
            window.drop_image(image).ok();
        }
        let scale = window.scale_factor();
        let visible = window.content_mask().bounds;
        // Tiles sit on whole device pixels, so each shows one to one.
        let base = point(
            (f32::from(origin.x) * scale).round(),
            (f32::from(origin.y) * scale).round(),
        );
        let t = TILE as f32;
        let first_col = ((f32::from(visible.left()) * scale - base.x) / t).floor() as i32;
        let last_col = ((f32::from(visible.right()) * scale - base.x) / t).ceil() as i32;
        let first_row = ((f32::from(visible.top()) * scale - base.y) / t).floor() as i32;
        let last_row = ((f32::from(visible.bottom()) * scale - base.y) / t).ceil() as i32;
        let page_cols = (snot_core::page::PAGE_WIDTH * 1.05 * self.density / t).ceil() as i32;
        for row in first_row.max(0)..last_row {
            for col in first_col.max(0)..last_col.min(page_cols) {
                let id = (layer, col, row);
                if !self.tiles.contains_key(&id) {
                    let tile = self.draw(id);
                    self.tiles.insert(id, tile);
                }
                let Some(Some(tile)) = self.tiles.get(&id) else {
                    continue;
                };
                let at = point(
                    px((base.x + col as f32 * t) / scale),
                    px((base.y + row as f32 * t) / scale),
                );
                let bounds = Bounds::new(at, size(px(t / scale), px(t / scale)));
                window
                    .paint_image(
                        bounds,
                        bounds,
                        Default::default(),
                        tile.image.clone(),
                        0,
                        false,
                    )
                    .ok();
            }
        }
        // Keep a screen's worth either side for scrolling back; let the
        // rest go.
        let keep = (last_row - first_row).max(1);
        let far: Vec<TileId> = self
            .tiles
            .keys()
            .filter(|(l, _, row)| {
                *l == layer && (*row < first_row - keep || *row > last_row + keep)
            })
            .copied()
            .collect();
        for id in far {
            self.forget(id);
        }
    }

    fn clear(&mut self) {
        for (_, tile) in self.tiles.drain() {
            if let Some(tile) = tile {
                self.dropped.push(tile.image);
            }
        }
    }

    fn forget(&mut self, id: TileId) {
        if let Some(Some(tile)) = self.tiles.remove(&id) {
            self.dropped.push(tile.image);
        }
    }

    /// The tiles, drawn or not, that a rectangle of the page lies across.
    fn tiles_over(&self, r: Rect) -> Vec<TileId> {
        let t = TILE as f32 / self.density;
        let (c0, c1) = ((r.min_x / t).floor() as i32, (r.max_x / t).floor() as i32);
        let (r0, r1) = ((r.min_y / t).floor() as i32, (r.max_y / t).floor() as i32);
        let mut ids = Vec::new();
        for layer in [Layer::Under, Layer::Over] {
            for row in r0..=r1 {
                for col in c0..=c1 {
                    ids.push((layer, col, row));
                }
            }
        }
        ids
    }

    /// Draws a new stroke over the tiles already drawn; tiles not drawn yet
    /// will have it when they are.
    fn add_on_top(&mut self, p: &Prepared) {
        for id in self.tiles_over(p.bounds) {
            if id.0 != p.layer {
                continue;
            }
            match self.tiles.get_mut(&id) {
                Some(Some(tile)) => {
                    fill(&mut tile.pixmap, p, self.density, id);
                    let image = image_of(&tile.pixmap);
                    self.dropped.push(std::mem::replace(&mut tile.image, image));
                }
                // Was blank: draw it afresh when next in view.
                Some(None) => {
                    self.tiles.remove(&id);
                }
                None => {}
            }
        }
    }

    fn draw(&self, id: TileId) -> Option<Tile> {
        let (layer, col, row) = id;
        let t = TILE as f32 / self.density;
        let area = Rect {
            min_x: col as f32 * t,
            min_y: row as f32 * t,
            max_x: (col + 1) as f32 * t,
            max_y: (row + 1) as f32 * t,
        };
        let mut pixmap: Option<Pixmap> = None;
        for p in self
            .strokes
            .iter()
            .filter(|p| p.layer == layer && overlaps(&p.bounds, &area))
        {
            let pm = pixmap.get_or_insert_with(|| Pixmap::new(TILE, TILE).expect("tile size"));
            fill(pm, p, self.density, id);
        }
        let pixmap = pixmap?;
        let image = image_of(&pixmap);
        Some(Tile { pixmap, image })
    }
}

fn overlaps(a: &Rect, b: &Rect) -> bool {
    a.min_x < b.max_x && b.min_x < a.max_x && a.min_y < b.max_y && b.min_y < a.max_y
}

fn fill(pixmap: &mut Pixmap, p: &Prepared, density: f32, (_, col, row): TileId) {
    let Some(path) = path_of(&p.outline) else {
        return;
    };
    let mut paint = Paint::default();
    paint.set_color(p.color);
    paint.anti_alias = true;
    let t = TILE as f32;
    let to_tile = Transform::from_row(
        density,
        0.,
        0.,
        density,
        -(col as f32) * t,
        -(row as f32) * t,
    );
    pixmap.fill_path(&path, &paint, FillRule::Winding, to_tile, None);
}

fn path_of(outline: &[[f32; 2]]) -> Option<tiny_skia::Path> {
    if outline.len() < 3 {
        return None;
    }
    let mut b = PathBuilder::with_capacity(outline.len() + 1, outline.len());
    b.move_to(outline[0][0], outline[0][1]);
    for [x, y] in &outline[1..] {
        b.line_to(*x, *y);
    }
    b.close();
    b.finish()
}

/// A tile's pixels as an image GPUI can show: straight alpha, BGRA.
fn image_of(pixmap: &Pixmap) -> Arc<RenderImage> {
    let mut data = pixmap.data().to_vec();
    for px in data.as_chunks_mut::<4>().0 {
        gpui::swap_rgba_pa_to_bgra(px);
    }
    let buffer = image::RgbaImage::from_raw(TILE, TILE, data).expect("tile buffer");
    Arc::new(RenderImage::new(SmallVec::from_elem(
        image::Frame::new(buffer),
        1,
    )))
}

fn key_of(s: &Stroke) -> u64 {
    let mut h = DefaultHasher::new();
    (s.tool as u8).hash(&mut h);
    s.color.hash(&mut h);
    s.width.to_bits().hash(&mut h);
    for p in &s.points {
        (p.x.to_bits(), p.y.to_bits(), p.pressure.to_bits()).hash(&mut h);
    }
    h.finish()
}

fn prepare(s: &Stroke, key: u64, dark: bool) -> Prepared {
    let argb = color::ink(&s.color, dark);
    let alpha = ((argb >> 24) & 0xff) as f32 / 255. * s.tool.alpha();
    Prepared {
        key,
        outline: s.outline(),
        bounds: s.bounds(),
        color: tiny_skia::Color::from_rgba8(
            ((argb >> 16) & 0xff) as u8,
            ((argb >> 8) & 0xff) as u8,
            (argb & 0xff) as u8,
            (alpha * 255.).round() as u8,
        ),
        layer: Layer::of(s.tool),
    }
}
