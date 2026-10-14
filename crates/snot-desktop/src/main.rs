//! Snot for the desktop: notes with text, lists, pictures and handwriting.

// A release build on Windows is a GUI app and opens no console window; debug
// builds keep one for their output.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod app;
mod assets;
mod editor;
mod text_field;
mod theme;
mod ui;
mod utf16;

use std::borrow::Cow;
use std::path::PathBuf;
use std::sync::Arc;

use gpui::{App, AppContext, Bounds, TitlebarOptions, WindowBounds, WindowOptions, px, size};
use gpui_platform::application;
use snot_core::Library;

/// Where notes live: `$SNOT_LIBRARY`, else the platform data directory.
fn library_dir() -> PathBuf {
    std::env::var_os("SNOT_LIBRARY")
        .map(PathBuf::from)
        .or_else(|| dirs::data_dir().map(|d| d.join("snot")))
        .unwrap_or_else(|| PathBuf::from("snot"))
}

fn main() -> anyhow::Result<()> {
    let dir = library_dir();
    let lib =
        Arc::new(Library::open(&dir).map_err(|e| {
            anyhow::anyhow!("could not open the library at {}: {e}", dir.display())
        })?);

    application()
        .with_assets(assets::Assets)
        .run(move |cx: &mut App| {
            let fonts = assets::FONTS.iter().map(|b| Cow::Borrowed(*b)).collect();
            if let Err(e) = cx.text_system().add_fonts(fonts) {
                eprintln!("snot: could not load fonts: {e}");
            }
            cx.bind_keys(text_field::key_bindings());
            cx.bind_keys(editor::key_bindings());
            cx.bind_keys(app::key_bindings());

            let bounds = Bounds::centered(None, size(px(1320.), px(860.)), cx);
            let options = WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                titlebar: Some(TitlebarOptions {
                    title: Some("Snot".into()),
                    ..Default::default()
                }),
                app_id: Some("snot-app".into()),
                window_min_size: Some(size(px(760.), px(480.))),
                ..Default::default()
            };
            let window = cx
                .open_window(options, |window, cx| {
                    let root = cx.new(|cx| app::SnotApp::new(lib.clone(), window, cx));
                    let saver = root.downgrade();
                    window.on_window_should_close(cx, move |_, cx| {
                        let _ = saver.update(cx, |app, cx| app.save_all(cx));
                        true
                    });
                    root
                })
                .expect("could not open a window");

            let _ = window.update(cx, |_, window, cx| {
                window.activate_window();
                cx.activate(true);
            });
        });
    Ok(())
}
