//! Icons and fonts compiled into the binary, so the app is one file.

use std::borrow::Cow;

use gpui::{AssetSource, Result, SharedString};

static ICONS: &[(&str, &[u8])] = &[
    (
        "icons/arrow-down.svg",
        include_bytes!("../assets/icons/arrow-down.svg"),
    ),
    (
        "icons/arrow-left.svg",
        include_bytes!("../assets/icons/arrow-left.svg"),
    ),
    (
        "icons/arrow-up.svg",
        include_bytes!("../assets/icons/arrow-up.svg"),
    ),
    ("icons/bold.svg", include_bytes!("../assets/icons/bold.svg")),
    (
        "icons/bullets.svg",
        include_bytes!("../assets/icons/bullets.svg"),
    ),
    (
        "icons/check.svg",
        include_bytes!("../assets/icons/check.svg"),
    ),
    (
        "icons/checklist.svg",
        include_bytes!("../assets/icons/checklist.svg"),
    ),
    (
        "icons/chevron-down.svg",
        include_bytes!("../assets/icons/chevron-down.svg"),
    ),
    (
        "icons/chevron-right.svg",
        include_bytes!("../assets/icons/chevron-right.svg"),
    ),
    (
        "icons/close.svg",
        include_bytes!("../assets/icons/close.svg"),
    ),
    ("icons/code.svg", include_bytes!("../assets/icons/code.svg")),
    ("icons/copy.svg", include_bytes!("../assets/icons/copy.svg")),
    (
        "icons/divider.svg",
        include_bytes!("../assets/icons/divider.svg"),
    ),
    (
        "icons/dotted.svg",
        include_bytes!("../assets/icons/dotted.svg"),
    ),
    (
        "icons/drawing.svg",
        include_bytes!("../assets/icons/drawing.svg"),
    ),
    ("icons/edit.svg", include_bytes!("../assets/icons/edit.svg")),
    (
        "icons/eraser.svg",
        include_bytes!("../assets/icons/eraser.svg"),
    ),
    (
        "icons/expand.svg",
        include_bytes!("../assets/icons/expand.svg"),
    ),
    (
        "icons/export.svg",
        include_bytes!("../assets/icons/export.svg"),
    ),
    (
        "icons/folder-plus.svg",
        include_bytes!("../assets/icons/folder-plus.svg"),
    ),
    (
        "icons/folder.svg",
        include_bytes!("../assets/icons/folder.svg"),
    ),
    ("icons/grid.svg", include_bytes!("../assets/icons/grid.svg")),
    (
        "icons/heading.svg",
        include_bytes!("../assets/icons/heading.svg"),
    ),
    (
        "icons/highlighter.svg",
        include_bytes!("../assets/icons/highlighter.svg"),
    ),
    (
        "icons/image.svg",
        include_bytes!("../assets/icons/image.svg"),
    ),
    (
        "icons/import.svg",
        include_bytes!("../assets/icons/import.svg"),
    ),
    (
        "icons/inbox.svg",
        include_bytes!("../assets/icons/inbox.svg"),
    ),
    (
        "icons/indent.svg",
        include_bytes!("../assets/icons/indent.svg"),
    ),
    (
        "icons/italic.svg",
        include_bytes!("../assets/icons/italic.svg"),
    ),
    (
        "icons/lined.svg",
        include_bytes!("../assets/icons/lined.svg"),
    ),
    (
        "icons/marker.svg",
        include_bytes!("../assets/icons/marker.svg"),
    ),
    ("icons/menu.svg", include_bytes!("../assets/icons/menu.svg")),
    ("icons/more.svg", include_bytes!("../assets/icons/more.svg")),
    ("icons/move.svg", include_bytes!("../assets/icons/move.svg")),
    (
        "icons/notes.svg",
        include_bytes!("../assets/icons/notes.svg"),
    ),
    (
        "icons/numbers.svg",
        include_bytes!("../assets/icons/numbers.svg"),
    ),
    (
        "icons/outdent.svg",
        include_bytes!("../assets/icons/outdent.svg"),
    ),
    (
        "icons/palette.svg",
        include_bytes!("../assets/icons/palette.svg"),
    ),
    ("icons/pen.svg", include_bytes!("../assets/icons/pen.svg")),
    ("icons/pin.svg", include_bytes!("../assets/icons/pin.svg")),
    (
        "icons/plain.svg",
        include_bytes!("../assets/icons/plain.svg"),
    ),
    ("icons/plus.svg", include_bytes!("../assets/icons/plus.svg")),
    (
        "icons/quote.svg",
        include_bytes!("../assets/icons/quote.svg"),
    ),
    ("icons/redo.svg", include_bytes!("../assets/icons/redo.svg")),
    (
        "icons/restore.svg",
        include_bytes!("../assets/icons/restore.svg"),
    ),
    (
        "icons/search.svg",
        include_bytes!("../assets/icons/search.svg"),
    ),
    (
        "icons/share.svg",
        include_bytes!("../assets/icons/share.svg"),
    ),
    (
        "icons/sidebar.svg",
        include_bytes!("../assets/icons/sidebar.svg"),
    ),
    ("icons/sort.svg", include_bytes!("../assets/icons/sort.svg")),
    (
        "icons/star-filled.svg",
        include_bytes!("../assets/icons/star-filled.svg"),
    ),
    ("icons/star.svg", include_bytes!("../assets/icons/star.svg")),
    (
        "icons/strike.svg",
        include_bytes!("../assets/icons/strike.svg"),
    ),
    (
        "icons/trash.svg",
        include_bytes!("../assets/icons/trash.svg"),
    ),
    ("icons/type.svg", include_bytes!("../assets/icons/type.svg")),
    (
        "icons/underline.svg",
        include_bytes!("../assets/icons/underline.svg"),
    ),
    ("icons/undo.svg", include_bytes!("../assets/icons/undo.svg")),
];

pub static FONTS: &[&[u8]] = &[
    include_bytes!("../assets/fonts/IBMPlexSans-Regular.ttf"),
    include_bytes!("../assets/fonts/IBMPlexSans-Italic.ttf"),
    include_bytes!("../assets/fonts/IBMPlexSans-SemiBold.ttf"),
    include_bytes!("../assets/fonts/IBMPlexSans-SemiBoldItalic.ttf"),
    include_bytes!("../assets/fonts/Lilex-Regular.ttf"),
    include_bytes!("../assets/fonts/Lilex-Bold.ttf"),
];

pub const UI_FONT: &str = "IBM Plex Sans";
pub const MONO_FONT: &str = "Lilex";

pub struct Assets;

impl AssetSource for Assets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        Ok(ICONS
            .iter()
            .find(|(p, _)| *p == path)
            .map(|(_, b)| Cow::Borrowed(*b)))
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        Ok(ICONS
            .iter()
            .filter(|(p, _)| p.starts_with(path))
            .map(|(p, _)| SharedString::from(*p))
            .collect())
    }
}
