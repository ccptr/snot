# Snot

Notes that mix typed text and handwriting on the same page, in the spirit
of Samsung Notes, with native apps for Linux, Windows and Android over one
Rust core.

- **Text** with headings, quotes, code, bulleted, numbered and checklists,
  indenting, bold, italic, underline, strikethrough, highlight and text colour.
- **Handwriting anywhere on the page**, over and around the text: pen,
  marker and highlighter in ten colours and several nib sizes, a stroke
  eraser, and plain, lined, grid or dotted paper. A note is one page of
  fixed width that zooms rather than reflows, so writing stays where it was
  put relative to the words on every screen. On Android a stylus always
  draws and fingers draw in pen mode, with two fingers to scroll.
- **Low-latency ink.** On Android the stroke being written goes straight
  to a front-buffered layer (Android 10 and newer) before the app's next
  frame. On the desktop, finished strokes are kept as tiles of pixels and
  the note's text and the lists beside it are reused from the last frame,
  so a moving pen redraws one stroke however long the note is.
- **Pictures** and dividers between paragraphs.
- **Organising** with folders, favourites, pinning, note colours, sorting by
  date or title, full-text search with highlighted matches, and a trash that
  keeps deleted notes for 30 days.
- **Undo and redo** for every edit, and autosave.
- **Markdown**, so notes are never locked in: export to a folder on the
  desktop, with a note's handwriting as `assets/handwriting.svg`; share as
  text and import files on Android.

Everything stays on the device, in a library you own: a SQLite database
plus an attachments folder.

## Install

Snot is an alpha. Builds for Linux (x86_64), Windows (x86_64 and ARM64) and
Android (arm64 and x86_64) are attached to each
[release](https://github.com/ccptr/snot/releases), along with a pacman
package for Arch Linux (`sudo pacman -U` its link). On Arch it is also in
the AUR as `snot-notes` (built from source), `snot-notes-bin` (the release's
build) and `snot-notes-git` (the latest commit); all install the app as
`snot`, with a desktop entry. The Windows build is not code-signed, so
SmartScreen asks before its first run. On Android it is also in an F-Droid
repository, which keeps it updated: add
[`https://fdroid.ccptr.dev/repo`](https://fdroid.ccptr.dev/) in F-Droid's
**Settings → Repositories** by scanning the QR code there, which pins the
repository's signing key as it is added.

## Layout

| Path | What it is |
|---|---|
| `crates/snot-core` | Document model, editing sessions with undo, ink smoothing and outlines, storage and search, Markdown |
| `crates/snot-ffi` | [UniFFI](https://mozilla.github.io/uniffi-rs/) bindings of the core for Kotlin |
| `crates/snot-desktop` | Linux and Windows app drawn with [GPUI](https://www.gpui.rs/) (binary `snot`) |
| `crates/uniffi-bindgen` | The bindings generator, pinned to the same UniFFI version |
| `android` | Android app in Kotlin and Jetpack Compose |
| `packaging/aur` | Arch Linux packages for the AUR |

## Desktop

Needs a recent stable Rust and, on Debian or Ubuntu,
`libxkbcommon-dev libxkbcommon-x11-dev libwayland-dev libx11-xcb-dev
libfontconfig-dev libvulkan-dev`. On Windows it needs the MSVC build tools
and a Windows SDK, whose `fxc.exe` compiles GPUI's shaders in release
builds; build it on Windows, including for ARM64.

```sh
cargo run --release -p snot-desktop
```

Notes live in `$XDG_DATA_HOME/snot` (usually `~/.local/share/snot`), or
`%APPDATA%\snot` on Windows; set `SNOT_LIBRARY` to use another folder. `SNOT_THEME=light` or `dark`
overrides the system appearance. `Ctrl+=` and `Ctrl+-` zoom the page and
`Ctrl+0` sets it back.

## Android

Needs JDK 21, the Android SDK with NDK 27.2.12479018, and the
Rust Android targets:

```sh
rustup target add aarch64-linux-android x86_64-linux-android
cd android
./gradlew installDebug                          # both ABIs
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
cd android && ./gradlew testDebugUnitTest -PrustAbis=arm64-v8a -PrustProfile=dev
```

CI runs these on every push, on Linux and on Windows x86_64 and ARM64, with
unoptimised builds only; optimised builds are made for releases.

## Releases

The version in the root `Cargo.toml` is the only one: the crates and the
Android app both take it, with Android's `versionCode` =
`MAJOR * 1000000 + MINOR * 1000 + PATCH`. Bump it, then publish a GitHub
release tagged `v` plus that version, and a Linux tarball, a pacman package
built from it, a Windows zip for x86_64 and ARM64, and a signed APK for each
Android ABI are attached to it; a tag that does not match
`Cargo.toml` builds nothing. The APKs are signed with the key in the
`release` environment, which only deploys from `v*` tags:

| Name | Kind | Value |
|---|---|---|
| `ANDROID_KEYSTORE_BASE64` | Secret | The PKCS12 release keystore, base64-encoded |
| `ANDROID_KEYSTORE_PASSWORD` | Secret | Its password, which is also the key's |
| `ANDROID_KEY_ALIAS` | Variable | The key's alias |

To build an existing release again, run the workflow on its tag:
`gh workflow run release.yml --ref v1.2.3`.

A full release (not a prerelease) then asks
[ccptr/fdroid](https://github.com/ccptr/fdroid) to collect its APKs into the
F-Droid repository, which it signs and deploys itself. That needs the
`FDROID_DISPATCH_TOKEN` repository secret: a fine-grained token with
Contents: write on `ccptr/fdroid` only. If the dispatch fails, run
`gh workflow run publish.yml -R ccptr/fdroid` instead.

The AUR packages are kept in `packaging/aur` and pushed to the AUR by hand.
After a full release's workflow has finished, point them at it, which
refreshes their checksums and `.SRCINFO`, commit that, and push each one:

```sh
packaging/aur/update.sh 1.2.3
git commit -am "Package 1.2.3 for the AUR"
for pkg in snot-notes snot-notes-bin; do
  [ -d ~/aur/$pkg ] || git clone ssh://aur@aur.archlinux.org/$pkg.git ~/aur/$pkg
  git -C ~/aur/$pkg pull
  cp packaging/aur/$pkg/{PKGBUILD,.SRCINFO} ~/aur/$pkg/
  git -C ~/aur/$pkg add PKGBUILD .SRCINFO
  git -C ~/aur/$pkg commit -m "Update to 1.2.3" && git -C ~/aur/$pkg push
done
```

`snot-notes-git` builds whatever is on `main`, so it only needs pushing when
its PKGBUILD changes. The pacman package attached to a release is
`snot-notes-bin` built from the release's own tarball.

Each desktop platform builds in its own job, in parallel; to add one, add an
entry to the `desktop` matrix in `.github/workflows/release.yml`. Every
Android ABI builds in one job, into an APK of its own; to add one, add it to
`rustAbis` in `android/app/build.gradle.kts`.

## Not there yet

- Builds for macOS and iOS.
- Pen pressure on the desktop (GPUI does not report it); Android uses it.
- Selecting across several blocks at once.
- Sync between devices, locked notes, PDF annotation and voice recordings.

## Licence

GPL-3.0-or-later.
