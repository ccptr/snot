//! Handwriting: vector strokes, their storage format, and the geometry that
//! turns them into filled outlines.
//!
//! A note's handwriting is one layer over its whole page, in page units
//! (see [`crate::page`]). Front-ends scale those units to whatever width
//! they show the page at, so a page written on a phone reopens on a
//! desktop with every stroke in the same place relative to the text.
//!
//! The outline of a stroke — the variable-width shape actually painted — is
//! computed here and not by each platform, so a note looks the same
//! everywhere and the pen feel only has to be tuned once.

use serde::{Deserialize, Serialize};

use crate::page::PAGE_WIDTH;

/// The handwriting on a page and the paper under it.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct InkLayer {
    #[serde(default)]
    pub paper: Paper,
    #[serde(default)]
    pub strokes: Vec<Stroke>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Paper {
    #[default]
    Plain,
    Lined,
    Grid,
    Dotted,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Tool {
    /// Width follows pressure — or speed, where there is no pressure.
    #[default]
    Pen,
    /// Even width whatever the input.
    Marker,
    /// Wide and translucent, painted under the other strokes.
    Highlighter,
}

impl Tool {
    /// Opacity the stroke is painted at.
    pub fn alpha(self) -> f32 {
        match self {
            Tool::Highlighter => 0.35,
            _ => 1.0,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Point {
    pub x: f32,
    pub y: f32,
    /// 0.0..=1.0. Input with no pressure sensor should pass a constant;
    /// the outline then simulates it from writing speed.
    pub pressure: f32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Stroke {
    pub tool: Tool,
    /// A palette token or `#rrggbb`; see [`crate::color`].
    pub color: String,
    /// Nominal width in ink units.
    pub width: f32,
    #[serde(with = "codec")]
    pub points: Vec<Point>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Rect {
    pub min_x: f32,
    pub min_y: f32,
    pub max_x: f32,
    pub max_y: f32,
}

impl Rect {
    pub fn contains(&self, x: f32, y: f32) -> bool {
        x >= self.min_x && x <= self.max_x && y >= self.min_y && y <= self.max_y
    }
}

impl Stroke {
    /// Builds a stroke from raw input, dropping points too close together
    /// to matter. Touch and pen digitisers report far more often than
    /// anyone can see, and every point is stored and outlined forever.
    pub fn new(tool: Tool, color: impl Into<String>, width: f32, raw: &[Point]) -> Self {
        let mut points: Vec<Point> = Vec::with_capacity(raw.len());
        let min_step = (width * 0.15).clamp(0.4, 2.0);
        for (i, p) in raw.iter().enumerate() {
            let p = Point {
                x: p.x.clamp(-50.0, PAGE_WIDTH + 50.0),
                y: p.y.max(-50.0),
                pressure: p.pressure.clamp(0.0, 1.0),
            };
            match points.last() {
                Some(last) if i + 1 < raw.len() && dist(last, &p) < min_step => {}
                _ => points.push(p),
            }
        }
        Stroke {
            tool,
            color: color.into(),
            width: width.max(0.5),
            points,
        }
    }

    pub fn bounds(&self) -> Rect {
        let pad = self.width;
        let mut r = Rect {
            min_x: f32::INFINITY,
            min_y: f32::INFINITY,
            max_x: f32::NEG_INFINITY,
            max_y: f32::NEG_INFINITY,
        };
        for p in &self.points {
            r.min_x = r.min_x.min(p.x - pad);
            r.min_y = r.min_y.min(p.y - pad);
            r.max_x = r.max_x.max(p.x + pad);
            r.max_y = r.max_y.max(p.y + pad);
        }
        r
    }

    /// Whether a circle at `(x, y)` of `radius` touches the stroke.
    pub fn hit(&self, x: f32, y: f32, radius: f32) -> bool {
        let reach = radius + self.width * 0.5;
        let b = self.bounds();
        if x < b.min_x - radius
            || x > b.max_x + radius
            || y < b.min_y - radius
            || y > b.max_y + radius
        {
            return false;
        }
        let c = Point {
            x,
            y,
            pressure: 0.0,
        };
        match self.points.as_slice() {
            [] => false,
            [only] => dist(only, &c) <= reach,
            pts => pts.windows(2).any(|w| seg_dist(&c, &w[0], &w[1]) <= reach),
        }
    }

    /// The filled shape this stroke paints, as a closed polygon in page
    /// units. Fill it with the non-zero rule: a stroke that crosses itself
    /// overlaps its own outline.
    pub fn outline(&self) -> Vec<[f32; 2]> {
        outline(&self.points, self.tool, self.width)
    }
}

impl InkLayer {
    pub fn is_empty(&self) -> bool {
        self.strokes.is_empty() && self.paper == Paper::Plain
    }

    pub fn add_stroke(&mut self, stroke: Stroke) {
        if !stroke.points.is_empty() {
            self.strokes.push(stroke);
        }
    }

    /// Removes every stroke the eraser circle touches. Returns where they
    /// were, in ascending order, so a front-end can drop exactly those
    /// from what it has on screen.
    pub fn erase_at(&mut self, x: f32, y: f32, radius: f32) -> Vec<usize> {
        let mut index = 0;
        let mut removed = Vec::new();
        self.strokes.retain(|s| {
            let hit = s.hit(x, y, radius);
            if hit {
                removed.push(index);
            }
            index += 1;
            !hit
        });
        removed
    }

    /// Lowest point any stroke reaches; the page is at least this long.
    pub fn content_bottom(&self) -> f32 {
        self.strokes
            .iter()
            .map(|s| s.bounds().max_y)
            .fold(0.0, f32::max)
    }

    /// Strokes in the order they are painted: highlighter under the rest.
    pub fn painting_order(&self) -> impl Iterator<Item = &Stroke> {
        self.strokes
            .iter()
            .filter(|s| s.tool == Tool::Highlighter)
            .chain(self.strokes.iter().filter(|s| s.tool != Tool::Highlighter))
    }

    /// The handwriting alone as an SVG the width of the page, for export.
    pub fn to_svg(&self, dark: bool) -> String {
        use std::fmt::Write;
        let h = (self.content_bottom() + 20.0).ceil();
        let mut out = format!(
            "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 {PAGE_WIDTH} {h}\" width=\"{PAGE_WIDTH}\" height=\"{h}\">"
        );
        for stroke in self.painting_order() {
            let poly = stroke.outline();
            if poly.is_empty() {
                continue;
            }
            let rgb = crate::color::ink(&stroke.color, dark) & 0xff_ffff;
            let _ = write!(out, "<path fill=\"#{rgb:06x}\"");
            if stroke.tool.alpha() < 1.0 {
                let _ = write!(out, " fill-opacity=\"{}\"", stroke.tool.alpha());
            }
            out.push_str(" d=\"");
            for (i, [x, y]) in poly.iter().enumerate() {
                let _ = write!(out, "{}{:.1} {:.1}", if i == 0 { "M" } else { "L" }, x, y);
            }
            out.push_str("Z\"/>");
        }
        out.push_str("</svg>");
        out
    }
}

fn dist(a: &Point, b: &Point) -> f32 {
    ((a.x - b.x).powi(2) + (a.y - b.y).powi(2)).sqrt()
}

fn seg_dist(p: &Point, a: &Point, b: &Point) -> f32 {
    let (dx, dy) = (b.x - a.x, b.y - a.y);
    let len2 = dx * dx + dy * dy;
    if len2 <= f32::EPSILON {
        return dist(p, a);
    }
    let t = (((p.x - a.x) * dx + (p.y - a.y) * dy) / len2).clamp(0.0, 1.0);
    let proj = Point {
        x: a.x + t * dx,
        y: a.y + t * dy,
        pressure: 0.0,
    };
    dist(p, &proj)
}

/// Outline of a stroke through `points`. Exposed on its own so a front-end
/// can draw the stroke still being written exactly as it will be stored.
pub fn outline(points: &[Point], tool: Tool, width: f32) -> Vec<[f32; 2]> {
    if points.is_empty() {
        return Vec::new();
    }
    let pts = smooth(points, width);
    let radii = radii(&pts, tool, width);
    if pts.len() == 1 || path_length(&pts) < width * 0.25 {
        let r = radii.iter().cloned().fold(0.0, f32::max);
        return circle(pts[0].x, pts[0].y, r, 16);
    }

    let n = pts.len();
    let mut left = Vec::with_capacity(n);
    let mut right = Vec::with_capacity(n);
    for i in 0..n {
        let a = &pts[i.saturating_sub(1)];
        let b = &pts[(i + 1).min(n - 1)];
        let (mut dx, mut dy) = (b.x - a.x, b.y - a.y);
        let len = (dx * dx + dy * dy).sqrt();
        if len > f32::EPSILON {
            dx /= len;
            dy /= len;
        } else {
            (dx, dy) = (1.0, 0.0);
        }
        let (nx, ny) = (-dy, dx);
        let r = radii[i];
        left.push([pts[i].x + nx * r, pts[i].y + ny * r]);
        right.push([pts[i].x - nx * r, pts[i].y - ny * r]);
    }

    let mut poly = Vec::with_capacity(2 * n + 24);
    poly.extend_from_slice(&left);
    // Round cap at the end: from the left edge round to the right.
    cap(&mut poly, &pts[n - 1], &pts[n - 2], radii[n - 1]);
    poly.extend(right.iter().rev());
    cap(&mut poly, &pts[0], &pts[1], radii[0]);
    poly
}

/// One pass of a [¼ ½ ¼] filter, which takes the jitter out of digitiser
/// input without pulling the stroke's ends.
///
/// Only densely sampled points are filtered. Where samples lie far apart —
/// a fast flick, or a shape built from a few vertices — the filter would
/// drag each corner towards its neighbours and shrink the shape, so those
/// points stay where they are.
fn smooth(points: &[Point], width: f32) -> Vec<Point> {
    if points.len() < 3 {
        return points.to_vec();
    }
    let reach = (width * 2.0).max(6.0);
    let mut out = Vec::with_capacity(points.len());
    out.push(points[0]);
    for w in points.windows(3) {
        if dist(&w[0], &w[1]) > reach || dist(&w[1], &w[2]) > reach {
            out.push(w[1]);
            continue;
        }
        out.push(Point {
            x: 0.25 * w[0].x + 0.5 * w[1].x + 0.25 * w[2].x,
            y: 0.25 * w[0].y + 0.5 * w[1].y + 0.25 * w[2].y,
            pressure: w[1].pressure,
        });
    }
    out.push(points[points.len() - 1]);
    out
}

fn path_length(pts: &[Point]) -> f32 {
    pts.windows(2).map(|w| dist(&w[0], &w[1])).sum()
}

fn radii(pts: &[Point], tool: Tool, width: f32) -> Vec<f32> {
    let half = width * 0.5;
    match tool {
        Tool::Marker | Tool::Highlighter => vec![half; pts.len()],
        Tool::Pen => {
            let first = pts[0].pressure;
            let constant = pts.iter().all(|p| (p.pressure - first).abs() < 0.01);
            let n = pts.len();
            let mut out = Vec::with_capacity(n);
            let mut sim = 0.6f32;
            for (i, p) in pts.iter().enumerate() {
                let pressure = if constant {
                    // No sensor: a slow line is a heavy line. The speed is
                    // smoothed so one jumpy sample does not pinch the ink.
                    let step = if i == 0 { 0.0 } else { dist(&pts[i - 1], p) };
                    let target = 1.0 - (step / (width * 2.5)).min(1.0) * 0.6;
                    sim += (target - sim) * 0.35;
                    sim
                } else {
                    p.pressure
                };
                let mut r = half * (0.35 + 0.85 * pressure);
                // Taper both ends a little, as a nib lifts and lands.
                let ends = (i.min(n - 1 - i) as f32 + 1.0) / 4.0;
                if constant && ends < 1.0 {
                    r *= 0.55 + 0.45 * ends;
                }
                out.push(r.max(0.3));
            }
            out
        }
    }
}

fn cap(poly: &mut Vec<[f32; 2]>, at: &Point, from: &Point, r: f32) {
    let base = (at.y - from.y).atan2(at.x - from.x);
    // Sweep from the left normal (+90°) through the tip to the right (−90°).
    const STEPS: usize = 8;
    for k in 1..STEPS {
        let a = base + std::f32::consts::FRAC_PI_2 - std::f32::consts::PI * k as f32 / STEPS as f32;
        poly.push([at.x + r * a.cos(), at.y + r * a.sin()]);
    }
}

fn circle(x: f32, y: f32, r: f32, steps: usize) -> Vec<[f32; 2]> {
    (0..steps)
        .map(|k| {
            let a = std::f32::consts::TAU * k as f32 / steps as f32;
            [x + r * a.cos(), y + r * a.sin()]
        })
        .collect()
}

/// Points are stored as one base64 string of zig-zag varint deltas: x and y
/// in eighths of an ink unit, pressure as a byte. A page of handwriting is
/// tens of thousands of points, and as JSON numbers it would be ten times
/// the size.
mod codec {
    use super::Point;
    use base64::Engine;
    use base64::engine::general_purpose::STANDARD_NO_PAD as B64;
    use serde::{Deserialize, Deserializer, Serializer};

    const SCALE: f32 = 8.0;

    pub fn serialize<S: Serializer>(points: &[Point], s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&encode(points))
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<Point>, D::Error> {
        let text = String::deserialize(d)?;
        decode(&text).map_err(serde::de::Error::custom)
    }

    pub fn encode(points: &[Point]) -> String {
        let mut bytes = Vec::with_capacity(points.len() * 4);
        let (mut px, mut py) = (0i64, 0i64);
        for p in points {
            let x = (p.x * SCALE).round() as i64;
            let y = (p.y * SCALE).round() as i64;
            varint(&mut bytes, zigzag(x - px));
            varint(&mut bytes, zigzag(y - py));
            bytes.push((p.pressure.clamp(0.0, 1.0) * 255.0).round() as u8);
            (px, py) = (x, y);
        }
        B64.encode(bytes)
    }

    pub fn decode(text: &str) -> Result<Vec<Point>, String> {
        let bytes = B64
            .decode(text.trim_end_matches('='))
            .map_err(|e| e.to_string())?;
        let mut out = Vec::new();
        let mut it = bytes.into_iter();
        let (mut x, mut y) = (0i64, 0i64);
        while let Some(dx) = read_varint(&mut it)? {
            let dy = read_varint(&mut it)?.ok_or("truncated point")?;
            let p = it.next().ok_or("truncated point")?;
            x += unzigzag(dx);
            y += unzigzag(dy);
            out.push(Point {
                x: x as f32 / SCALE,
                y: y as f32 / SCALE,
                pressure: p as f32 / 255.0,
            });
        }
        Ok(out)
    }

    fn zigzag(v: i64) -> u64 {
        ((v << 1) ^ (v >> 63)) as u64
    }

    fn unzigzag(v: u64) -> i64 {
        ((v >> 1) as i64) ^ -((v & 1) as i64)
    }

    fn varint(out: &mut Vec<u8>, mut v: u64) {
        while v >= 0x80 {
            out.push((v as u8) | 0x80);
            v >>= 7;
        }
        out.push(v as u8);
    }

    fn read_varint(it: &mut impl Iterator<Item = u8>) -> Result<Option<u64>, String> {
        let mut v = 0u64;
        for shift in (0..64).step_by(7) {
            let Some(b) = it.next() else {
                return if shift == 0 {
                    Ok(None)
                } else {
                    Err("truncated varint".into())
                };
            };
            v |= ((b & 0x7f) as u64) << shift;
            if b & 0x80 == 0 {
                return Ok(Some(v));
            }
        }
        Err("varint too long".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(n: usize, pressure: f32) -> Vec<Point> {
        (0..n)
            .map(|i| Point {
                x: 100.0 + i as f32 * 5.0,
                y: 200.0 + (i as f32 * 0.3).sin() * 20.0,
                pressure,
            })
            .collect()
    }

    #[test]
    fn points_survive_the_codec() {
        let pts = line(50, 0.42);
        let stroke = Stroke {
            tool: Tool::Pen,
            color: "ink".into(),
            width: 3.0,
            points: pts.clone(),
        };
        let json = serde_json::to_string(&stroke).unwrap();
        let back: Stroke = serde_json::from_str(&json).unwrap();
        for (a, b) in pts.iter().zip(&back.points) {
            assert!((a.x - b.x).abs() <= 1.0 / 16.0);
            assert!((a.y - b.y).abs() <= 1.0 / 16.0);
            assert!((a.pressure - b.pressure).abs() <= 1.0 / 255.0);
        }
        // Far smaller than the same points as JSON numbers.
        assert!(
            json.len()
                < serde_json::to_string(
                    &pts.iter()
                        .map(|p| [p.x, p.y, p.pressure])
                        .collect::<Vec<_>>()
                )
                .unwrap()
                .len()
                    / 4
        );
    }

    #[test]
    fn negative_coordinates_round_trip() {
        let pts = vec![
            Point {
                x: -10.0,
                y: -3.5,
                pressure: 1.0,
            },
            Point {
                x: 5.0,
                y: -20.0,
                pressure: 0.0,
            },
        ];
        let back = codec::decode(&codec::encode(&pts)).unwrap();
        assert_eq!(back, pts);
    }

    #[test]
    fn outline_is_closed_and_wraps_the_points() {
        let stroke = Stroke::new(Tool::Pen, "ink", 4.0, &line(40, 0.5));
        let poly = stroke.outline();
        assert!(poly.len() > stroke.points.len() * 2);
        let b = stroke.bounds();
        for [x, y] in &poly {
            assert!(b.contains(*x, *y), "({x},{y}) outside {b:?}");
        }
    }

    #[test]
    fn a_dot_is_a_circle() {
        let stroke = Stroke::new(
            Tool::Marker,
            "ink",
            6.0,
            &[Point {
                x: 10.0,
                y: 10.0,
                pressure: 0.5,
            }],
        );
        let poly = stroke.outline();
        assert_eq!(poly.len(), 16);
        for [x, y] in poly {
            assert!((((x - 10.0).powi(2) + (y - 10.0).powi(2)).sqrt() - 3.0).abs() < 1e-3);
        }
    }

    #[test]
    fn eraser_takes_only_what_it_touches() {
        let mut ink = InkLayer::default();
        ink.add_stroke(Stroke::new(Tool::Pen, "ink", 3.0, &line(20, 0.5)));
        ink.add_stroke(Stroke::new(
            Tool::Pen,
            "red",
            3.0,
            &[
                Point {
                    x: 500.0,
                    y: 500.0,
                    pressure: 0.5,
                },
                Point {
                    x: 600.0,
                    y: 500.0,
                    pressure: 0.5,
                },
            ],
        ));
        assert!(ink.erase_at(300.0, 600.0, 10.0).is_empty());
        assert_eq!(ink.erase_at(550.0, 505.0, 6.0), [1]);
        assert_eq!(ink.strokes.len(), 1);
        assert_eq!(ink.strokes[0].color, "ink");
    }

    #[test]
    fn dense_input_is_thinned() {
        let raw: Vec<Point> = (0..1000)
            .map(|i| Point {
                x: i as f32 * 0.01,
                y: 0.0,
                pressure: 0.5,
            })
            .collect();
        let stroke = Stroke::new(Tool::Pen, "ink", 3.0, &raw);
        assert!(stroke.points.len() < 30);
        assert_eq!(stroke.points.last().unwrap().x, raw.last().unwrap().x);
    }

    #[test]
    fn sparse_corners_keep_their_place() {
        // A five-point star from its vertices alone: smoothing must not
        // pull the points in towards the middle.
        let star: Vec<Point> = (0..=5)
            .map(|i| {
                let a = i as f32 * std::f32::consts::PI * 0.8;
                Point {
                    x: 500.0 + 100.0 * a.cos(),
                    y: 300.0 + 100.0 * a.sin(),
                    pressure: 0.5,
                }
            })
            .collect();
        let poly = outline(&star, Tool::Marker, 4.0);
        let reach = poly
            .iter()
            .map(|[x, y]| ((x - 500.0).powi(2) + (y - 300.0).powi(2)).sqrt())
            .fold(0.0, f32::max);
        assert!(reach > 95.0, "outline reaches only {reach}");
    }
}
