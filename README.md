# Snot

Notes that mix typed text and handwriting on the same page, in the spirit
of Samsung Notes, with native apps for Linux and Android over one Rust core.

- **Text** with headings, quotes, code, bulleted, numbered and checklists,
  indenting, bold, italic, underline, strikethrough, highlight and text colour.
- **Handwriting** in drawing blocks anywhere in a note: pen, marker and
  highlighter in ten colours and several nib sizes, a stroke eraser, and
  plain, lined, grid or dotted paper. On Android a stylus always draws and
  fingers draw in pen mode.
- **Pictures** and dividers between blocks.
- **Organising** with folders, favourites, pinning, note colours, sorting by
  date or title, full-text search with highlighted matches, and a trash that
  keeps deleted notes for 30 days.
- **Undo and redo** for every edit, and autosave.
- **Markdown**, so notes are never locked in: export to a folder on the
  desktop; share as text and import files on Android.

Everything stays on the device, in a library you own: a SQLite database
plus an attachments folder.

## Layout

| Path | What it is |
|---|---|
| `crates/snot-core` | Document model, editing sessions with undo, ink smoothing and outlines, storage and search, Markdown |
| `crates/snot-ffi` | [UniFFI](https://mozilla.github.io/uniffi-rs/) bindings of the core for Kotlin |
| `crates/snot-desktop` | Linux app drawn with [GPUI](https://www.gpui.rs/) (binary `snot`) |
| `crates/uniffi-bindgen` | The bindings generator, pinned to the same UniFFI version |
| `android` | Android app in Kotlin and Jetpack Compose |

## Linux desktop

Needs a recent stable Rust and, on Debian or Ubuntu,
`libxkbcommon-dev libxkbcommon-x11-dev libwayland-dev libx11-xcb-dev
libfontconfig-dev libvulkan-dev`.

```sh
cargo run --release -p snot-desktop
```

Notes live in `$XDG_DATA_HOME/snot` (usually `~/.local/share/snot`); set
`SNOT_LIBRARY` to use another folder. `SNOT_THEME=light` or `dark`
overrides the system appearance.

## Android

Needs JDK 21, the Android SDK with NDK 27.2.12479018, and the
Rust Android targets:

```sh
rustup target add aarch64-linux-android armv7-linux-androideabi x86_64-linux-android
cd android
./gradlew installDebug                          # all ABIs
./gradlew installDebug -PrustAbis=arm64-v8a     # faster, phones only
```

Gradle compiles `snot-ffi` with the NDK's clang and generates the Kotlin
bindings from the built library. The app needs Android 8.0 or newer. Notes
live in the app's private storage and are included in device backups.

## Checks

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

## Not there yet

- Builds for Windows, macOS and iOS.
- Pen pressure on the desktop (GPUI does not report it); Android uses it.
- Selecting across several blocks at once.
- Sync between devices, locked notes, PDF annotation and voice recordings.

## Licence

GPL-3.0-or-later.
