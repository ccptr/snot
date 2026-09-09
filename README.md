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
| **Ink** | **Draw anywhere on the page** — over, around and between the text, no drawing box. Pressure-sensitive vector strokes, pen and marker, stroke eraser, and **palm rejection** once a stylus is seen |
| **PDFs** | Import a PDF as a note and write on it. Pages render lazily, so a long document opens at once and costs only what you look at |
| **Organise** | Nested folders, tags, favourites, pinning, drag a note onto a folder to file it |
| **Find** | Full-text search over every title and body, prefix-matching as you type, with the matched phrase highlighted in the result |
| **Voice** | Record straight into a note. The clip is stored as an attachment and plays back in the page; a note that is nothing but a recording still gets a title and turns up in search |
| **Safety** | Deletes go to a trash you can restore from; nothing is destroyed until you empty it |
| **Get in** | Import a folder of Markdown — front matter, tags, dates, nested folders and linked pictures all come across — or a Samsung Notes export. Nothing is ever dropped: a file that cannot be read still arrives as a note carrying the original |
| **Get out** | Every note exports to Markdown. Attachments are ordinary files on disk |
| **Fits the screen** | Three panes on a desktop, one pane with a drawer on a phone, light and dark following the system. On a phone the ribbon sits at the foot of the note and rides above the soft keyboard, with the text formatting behind one button so it stays a single row. Android's back button steps back through the note, the drawer and any dialog before it leaves the app |

Ink is stored as vectors, not a bitmap: a drawing made on a phone reopens on a
desktop at the same proportions and stays sharp at any zoom. The default pen is
stored as a token rather than a colour, so a note written in dark mode is not
invisible when it is reopened in light mode.

The Samsung Notes import is deliberately honest about being partial. A folder
of PDF exports — the format every version of Samsung Notes can produce — comes
across whole, one annotatable page per file. A `.sdocx` is not the XML the
file-extension sites claim: it is a ZIP of little-endian binary whose records
are laid out by property bitmasks and has no published specification, so Snot
takes the pictures and PDFs it carries and sweeps its note body for typed
text. Handwriting, shapes and layout do not survive that, and the sweep can
turn up the odd stray internal string. The older `.snb` is real XML and reads
back cleanly. Anything that yields nothing at all still becomes a note with the
original file attached, and the import says how many that was, so a Samsung
library can be brought over and picked through rather than left behind.

Both folder imports need a folder picker, which is a desktop affordance; on a
phone they say so rather than half-working, and single files still import.

Recording depends on the webview each platform provides. Android asks for the
microphone the first time and plays the clip back in the page. Where a webview
has no recorder, or will not answer a request for the microphone — WebKitGTK on
the Linux desktop is the one to watch — the toolbar button is disabled or says
so plainly rather than failing quietly.

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

A note is three independent things: a rich-text document, an array of ink
strokes, and an optional background document. Handwriting sits beside the text
rather than inside it, because a stroke can cross any number of paragraphs or
sit on an empty part of the page — a shape the text flow cannot hold.

Set `SNOT_LIBRARY` to point the app at a different directory.

A first launch lays down a single welcome note. For testing or screenshots,
`SNOT_DEMO=1` starts a new library with a full worked example instead — folders,
tags, handwriting, favourites and a populated trash. It is opt-in and never
reaches a real user; mobile test builds can enable the `demo-library` feature
for the same thing.

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
- [ ] Selecting, moving and reflowing ink
- [x] Voice recordings attached to a note
- [ ] Note lock (the `locked` flag exists; the encryption does not)
- [x] Import from a Samsung Notes export, and from Markdown directories
- [ ] Handwriting search

## Licence

[GNU GPL v3 or later](LICENSE). Fork it, ship it, build on it — but anything
you distribute stays open.
