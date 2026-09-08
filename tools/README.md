# Releasing Snot for Android

Two variants ship from this one source tree:

| | application id | versionCode | published by |
|---|---|---|---|
| **release** | `app.snot.notes` | from `tauri.conf.json` | pushing a `v*` tag, after a review |
| **nightly** | `app.snot.notes.nightly` | commit count on `main` | every push to `main`, unattended |

Different application ids mean both install side by side with separate
libraries, so trying the nightly cannot touch notes in the release build. They
are signed with different keys, so a leaked nightly key costs the nightly app
only.

The variant identifier comes from `crates/snot-app/tauri.nightly.conf.json` via
`tauri --config`, not from a Gradle patch, because `gen/android` is regenerated
by `cargo tauri android init` and is not tracked. Tauri bakes the application id
all the way down into the Kotlin package directories and refuses to change it in
place, so `release-android.sh` deletes and regenerates `gen/android` whenever the
tree on disk belongs to the other variant.

## Permissions in a generated project

`gen/android` and `gen/apple` are produced by `cargo tauri <platform> init` and
are gitignored, so anything that has to live inside them cannot simply be edited
once and committed — a fresh checkout, a CI run, or a variant switch that deletes
the tree all start from the stock template again.

Voice recording needs exactly that. wry's `RustWebChromeClient` already turns the
webview's `AUDIO_CAPTURE` request into a runtime Android permission request, but
Android auto-denies a runtime request for a permission the manifest never
declared, and the generated manifest declares only `INTERNET`. iOS is stricter
still: without `NSMicrophoneUsageDescription` in `Info.plist` the system does not
deny the request, it kills the process.

`tools/patch-permissions.sh` puts those declarations back. It is idempotent, so
it runs on every Android build rather than only after an init — from
`release-android.sh` just after its init step, and as its own step in the
`android` job of `.github/workflows/ci.yml`. Anything else a generated project
needs belongs in that script too, for the same reason.

## Locally

```sh
cp tools/release.env.example tools/release.env   # once, then edit
tools/release-android.sh                          # release variant
tools/release-android.sh --variant nightly
```

`--help` lists the flags that stop it early: `--build-only` (touches no keys),
`--no-publish` (signed APK in `dist/`), `--no-deploy` (local repo only).

## In CI

`.github/workflows/release.yml` runs the same script. A `v*` tag builds and
signs the release variant; a push to `main` does the nightly. The signed APK is
attached to a GitHub Release — and that is where this repo's involvement ends.

Publishing is not done here. The F-Droid repository carries several apps, so its
config, every app's metadata, the index key and the deploy live in **ccptr/fdroid**,
which collects the release assets published here. That keeps the index key out of
each app repo, so a compromise of one app cannot forge the index for the others,
and it puts every deploy in one workflow, so two apps publishing at once cannot
race each other's index.

The handover format is the whole contract: a signed APK named
`<applicationId>_<versionCode>.apk` on a GitHub Release.

### The build/sign split

`build` runs every build script in the dependency tree — each crate's `build.rs`,
every npm lifecycle script — and is given **no secrets at all**. `sign` holds the
key and runs almost nothing: download an artifact, sign it, attach it.

The release APK key sits behind a required review. The nightly key is used
unattended, which buys an attacker nightly builds if they get it, but not an
update to anyone's installed release build — F-Droid pins the release signer.

### Environments

Create two GitHub environments. **`release` must have required reviewers and be
restricted to tag refs** — referencing an environment that does not exist
silently creates one with no protection at all, which would leave the release
key ungated.

Each holds these under the same names, with different values:

| name | kind | |
|---|---|---|
| `APK_KEYSTORE_B64` | secret | `base64 -w0 < keystore.p12` for that variant |
| `APK_KEYSTORE_PASS` | secret | its password |
| `APK_KEY_ALIAS` | variable | `snot` / `snot-nightly` |

Optional, to make the F-Droid repo publish immediately rather than waiting for
its next scheduled run:

| name | kind | |
|---|---|---|
| `FDROID_DISPATCH_TOKEN` | secret | a token that can dispatch workflows in the fdroid repo |
| `FDROID_REPO_SLUG` | variable | `ccptr/fdroid` |

## The keys

- **release APK key** — pinned by every install of `app.snot.notes`
- **nightly APK key** — pinned by every install of `app.snot.notes.nightly`

Neither can be rotated; keep offline backups. The index and deploy keys are not
this repo's concern any more — they live in ccptr/fdroid.

`tools/signer.sha256` and `tools/signer-nightly.sha256` record the expected
certificates, and the script refuses to publish anything signed by a different
key. The same digests go in the fdroid repo's `apps.yml`, where they are checked
again on the way in and enforced by fdroid itself as `AllowedAPKSigningKeys`.

## Publishing by hand

The script can still publish straight into a local F-Droid working tree, which
is what you want if GitHub is down or you are testing the repo itself:

```sh
pipx install fdroidserver
SNOT_FDROID_DIR=~/snot-fdroid tools/release-android.sh --no-deploy
```

That needs a working tree made by `fdroid init`; see the ccptr/fdroid README for
how the repo is laid out and how the server is set up.

## Why a universal APK

`--split-per-abi` is deliberately not used: Tauri's generated Gradle gives every
ABI the same `versionCode`, and F-Droid rejects two APKs of one package that
share one. The universal APK is bigger but correct. For the same reason the APK
is signed out-of-band with `apksigner` rather than by a Gradle `signingConfig` —
a `signingConfig` in `gen/android` would not survive regeneration.
