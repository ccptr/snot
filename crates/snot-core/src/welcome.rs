//! The note a brand-new library opens on.

use std::sync::Arc;

use crate::doc::{Block, Content, Doc, Mark, ParaStyle, new_block_id};
use crate::error::Result;
use crate::ink::{InkBlock, Paper, Point, Stroke, Tool};
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
        ("Draw in the box below, or add a new drawing", false),
        ("Search for a word from any note", false),
    ] {
        let mut b = Block::styled(ParaStyle::Checklist, text);
        b.as_text_mut().expect("text block").checked = done;
        blocks.push(b);
    }

    let mut ink = InkBlock {
        height: 260.0,
        paper: Paper::Dotted,
        strokes: Vec::new(),
    };
    // A star and a flourish under it, so the page shows what ink looks like.
    let star: Vec<Point> = (0..=10)
        .map(|i| {
            let a = -std::f32::consts::FRAC_PI_2 + i as f32 * std::f32::consts::PI * 0.8;
            Point {
                x: 150.0 + 80.0 * a.cos(),
                y: 120.0 + 80.0 * a.sin(),
                pressure: 0.6,
            }
        })
        .collect();
    ink.strokes.push(Stroke::new(Tool::Pen, "blue", 5.0, &star));
    let wave: Vec<Point> = (0..=60)
        .map(|i| {
            let x = 290.0 + i as f32 * 9.0;
            let p = 0.3 + 0.6 * (i as f32 / 60.0 * std::f32::consts::PI).sin();
            Point {
                x,
                y: 140.0 + 28.0 * (i as f32 * 0.32).sin(),
                pressure: p,
            }
        })
        .collect();
    ink.strokes.push(Stroke::new(Tool::Pen, "ink", 4.0, &wave));
    let swipe = [
        Point {
            x: 300.0,
            y: 200.0,
            pressure: 0.5,
        },
        Point {
            x: 800.0,
            y: 200.0,
            pressure: 0.5,
        },
    ];
    ink.strokes
        .push(Stroke::new(Tool::Highlighter, "green", 26.0, &swipe));
    blocks.push(Block {
        id: new_block_id(),
        content: Content::Ink(Arc::new(ink)),
    });

    blocks.push(Block::text(
        "Notes save as you go. Deleted notes wait in the trash for 30 days before they are gone.",
    ));
    lib.insert_note(None, "Welcome to Snot".into(), Doc { blocks })?;
    Ok(())
}
