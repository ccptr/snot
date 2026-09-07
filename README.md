<h1 align="center">Snot</h1>
<p align="center"><em>Open source notes. A drop-in replacement for Samsung Notes that runs everywhere.</em></p>

---

Samsung Notes is good software locked to one vendor's phones and one vendor's
cloud. Snot keeps the parts that matter — folders, rich text, checklists, and
real stylus ink on the same page as your writing — and drops the lock-in. Your
library is a plain SQLite file and a folder of attachments on your own disk.

**Status:** early. The core is solid and tested; sync and PDF annotation are
not built yet. See [Roadmap](#roadmap).

## What it does today

| | |
|---|---|
| **Rich text** | Headings, bold/italic/underline/strike, highlight in five colours, bulleted, numbered and **checklists**, quotes, code blocks, dividers, alignment |
| **Ink** | Pressure-sensitive vector strokes, pen and marker, stroke eraser, ruled guides, resizable canvas, **palm rejection** once a stylus is seen |
| **Organise** | Nested folders, tags, favourites, pinning, drag a note onto a folder to file it |
| **Find** | Full-text search over every title and body, prefix-matching as you type, with the matched phrase highlighted in the result |
| **Safety** | Deletes go to a trash you can restore from; nothing is destroyed until you empty it |
| **Get out** | Every note exports to Markdown. Attachments are ordinary files on disk |
| **Fits the screen** | Three panes on a desktop, one pane with a drawer on a phone, light and dark following the system |

Ink is stored as vectors, not a bitmap: a drawing made on a phone reopens on a
desktop at the same proportions and stays sharp at any zoom.

## Platforms

One Rust core and one web UI, shipped through [Tauri 2](https://tauri.app) as a
native app on **Linux, Windows, macOS, Android and iOS**. There is no Electron
and no bundled browser — the app uses the webview the OS already ships.

## Architecture

```
crates/snot-core   storage, search, document model, Markdown export — no UI, no platform
crates/snot-app    Tauri shell: exposes snot-core to the webview as typed commands
ui/                the interface (TypeScript, TipTap, no framework runtime)
```

`snot-core` deliberately knows nothing about Tauri or the web. It is the piece a
CLI, a sync daemon or a different UI would link against, and it is where the
tests live.

The library on disk:

```
<app data>/library/
  snot.db                SQLite: notes, folders, tags, attachments, FTS5 index
  attachments/ab/ab12…   content-addressed blobs — the same image pasted twice is stored once
```

Set `SNOT_LIBRARY` to point the app at a different directory.

## Build it

Prerequisites: [Rust](https://rustup.rs), Node 20+, and the
[Tauri system dependencies](https://tauri.app/start/prerequisites/) for your OS
(on Linux that is `webkit2gtk-4.1`, `libayatana-appindicator3` and `librsvg`).

```sh
npm --prefix ui install
cargo install tauri-cli --version "^2"   # if you don't have it

cargo tauri dev      # run it
cargo tauri build    # produce an installer for the current OS
```

Tests:

```sh
cargo test            # the core: storage, search, trash, export
npm --prefix ui run build   # typecheck and bundle the UI
```

### Android and iOS

```sh
cargo tauri android init && cargo tauri android dev
cargo tauri ios init     && cargo tauri ios dev
```

Android additionally needs the Android SDK, the NDK, and
`ANDROID_HOME`/`NDK_HOME` set. iOS needs Xcode and a macOS host.

## Roadmap

- [ ] Sync — end-to-end encrypted, over any file-sync service or a self-hosted server
- [ ] PDF import and annotation
- [ ] Voice recordings attached to a note
- [ ] Note lock (the `locked` flag exists; the encryption does not)
- [ ] Import from a Samsung Notes export, and from Markdown directories
- [ ] Handwriting search

## Licence

[GNU GPL v3 or later](LICENSE). Fork it, ship it, build on it — but anything
you distribute stays open.
