//! The note a brand-new library opens on.

use crate::doc::{Block, Doc, Mark, ParaStyle};
use crate::error::Result;
use crate::ink::{InkLayer, Paper, Point, Stroke, Tool};
use crate::page;
use crate::store::Library;

pub(crate) fn seed(lib: &Library) -> Result<()> {
    let mut blocks = Vec::new();
    let mut intro = Block::text(
        "Snot keeps typed text and handwriting on the same page. Everything stays on this device, \
         in a library you own.",
    );
    let t = intro.as_text_mut().expect("text block");
    t.toggle_mark(41..54, &Mark::Bold);
    t.toggle_mark(89..109, &Mark::Highlight("yellow".into()));
    blocks.push(intro);
    blocks.push(Block::styled(ParaStyle::Heading2, "Things to try"));
    for (text, done) in [
        ("Tick this box", true),
        ("Make a list: start a line, then pick a list style", false),
        (
            "Pick the pen and write anywhere on the page, even over this line",
            false,
        ),
        ("Search for a word from any note", false),
    ] {
        let mut b = Block::styled(ParaStyle::Checklist, text);
        b.as_text_mut().expect("text block").checked = done;
        blocks.push(b);
    }

    blocks.push(Block::text(
        "Notes save as you go. Deleted notes wait in the trash for 30 days before they are gone.",
    ));

    // A star and a flourish below the text, so the page shows what ink
    // looks like.
    let y: f32 = blocks.iter().map(page::estimated_height).sum::<f32>() + 60.0;
    let mut ink = InkLayer {
        paper: Paper::Plain,
        strokes: Vec::new(),
    };
    let star: Vec<Point> = (0..=10)
        .map(|i| {
            let a = -std::f32::consts::FRAC_PI_2 + i as f32 * std::f32::consts::PI * 0.8;
            Point {
                x: 190.0 + 80.0 * a.cos(),
                y: y + 90.0 + 80.0 * a.sin(),
                pressure: 0.6,
            }
        })
        .collect();
    ink.strokes.push(Stroke::new(Tool::Pen, "blue", 5.0, &star));
    let wave: Vec<Point> = (0..=55)
        .map(|i| {
            let p = 0.3 + 0.6 * (i as f32 / 55.0 * std::f32::consts::PI).sin();
            Point {
                x: 330.0 + i as f32 * 9.0,
                y: y + 110.0 + 28.0 * (i as f32 * 0.32).sin(),
                pressure: p,
            }
        })
        .collect();
    ink.strokes.push(Stroke::new(Tool::Pen, "ink", 4.0, &wave));
    let swipe = [
        Point {
            x: 340.0,
            y: y + 170.0,
            pressure: 0.5,
        },
        Point {
            x: 820.0,
            y: y + 170.0,
            pressure: 0.5,
        },
    ];
    ink.strokes
        .push(Stroke::new(Tool::Highlighter, "green", 26.0, &swipe));

    let doc = Doc {
        blocks,
        ink: ink.into(),
    };
    lib.insert_note(None, "Welcome to Snot".into(), doc)?;
    Ok(())
}
