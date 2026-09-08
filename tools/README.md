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

## Locally

```sh
cp tools/release.env.example tools/release.env   # once, then edit
tools/release-android.sh                          # release variant
tools/release-android.sh --variant nightly
```

`--help` lists the flags that stop it early: `--build-only` (touches no keys),
`--no-publish` (signed APK in `dist/`), `--no-deploy` (local repo only).

## In CI

`.github/workflows/release.yml` runs the same script. A `v*` tag publishes the
release variant; a push to `main` publishes a nightly.

It is split into two jobs on purpose. `build` runs every build script in the
dependency tree — each crate's `build.rs`, every npm lifecycle script — and is
given **no secrets at all**. `publish` holds the keys and runs almost nothing:
download an artifact, sign it, rsync the result.

### What that boundary is worth

The release APK key sits behind a required review, so nobody ships a release
update without a human approving it.

The repo index key and the nightly APK key are used unattended. A compromise
there lets an attacker publish nightly builds and add new packages to the index
— but **not** push an update to anyone's installed release build, because
F-Droid pins the release signer and the attacker does not have that key.

If that boundary is too loose, give nightly its own F-Droid repository with its
own index key, so the main repo's index is only ever signed behind the gate.
Nothing else in the setup changes.

### Environments

Create two GitHub environments. **`release` must have required reviewers and be
restricted to tag refs** — referencing an environment that does not exist
silently creates one with no protection at all, which would leave the release
key ungated.

Each environment holds these under the same names, with different values:

| name | kind | |
|---|---|---|
| `APK_KEYSTORE_B64` | secret | `base64 -w0 < keystore.p12` for that variant |
| `APK_KEYSTORE_PASS` | secret | its password |
| `APK_KEY_ALIAS` | variable | `snot` / `snot-nightly` |

Repository-wide:

| name | kind | |
|---|---|---|
| `FDROID_KEYSTORE_B64` | secret | the repo *index* key, base64 |
| `FDROID_KEYSTORE_PASS` | secret | its password |
| `DEPLOY_SSH_KEY` | secret | private key for `fdroid@<host>` |
| `DEPLOY_KNOWN_HOSTS` | secret | `ssh-keyscan <host>` output, so the host key is pinned |
| `FDROID_KEY_ALIAS` | variable | index key alias |
| `FDROID_REPO_URL` | variable | `https://<host>/fdroid/repo` |
| `FDROID_ARCHIVE_URL` | variable | `https://<host>/fdroid/archive` |
| `FDROID_REPO_NAME` | variable | e.g. `Snot` |
| `FDROID_SERVERWEBROOT` | variable | `fdroid@<host>:/srv/fdroid/fdroid/` |

`fdroid/metadata/*.yml` is tracked here and copied into the working tree each
run; the existing `repo/` and `archive/` are rsynced down from the server first,
because `fdroid update` needs the published versions to write a complete index
and to decide what rolls into the archive.

## The keys

Four keystores exist, and none of them can be rotated:

- **release APK key** — pinned by every install of `app.snot.notes`
- **nightly APK key** — pinned by every install of `app.snot.notes.nightly`
- **repo index key** — pinned by every client that added the repo
- **deploy SSH key** — the one that *can* be rotated freely

Keep offline backups of the first three. `tools/signer.sha256` and
`tools/signer-nightly.sha256` record the expected APK certificates; the script
refuses to publish anything signed by a different key, which matters more in CI
than locally because nobody is watching. Create each file after the first real
signature, using the digest the script prints.

## One-time: the server

Any static web root. On an Arch-family host:

```sh
sudo useradd -r -m -d /srv/fdroid -s /bin/bash fdroid
sudo install -d -o fdroid -g fdroid /srv/fdroid/fdroid
sudo pacman -S nginx-mainline certbot certbot-nginx rsync
```

nginx wants `root /srv/fdroid;` and `autoindex off;`, then `certbot --nginx`.
TLS is not what makes the repo trustworthy — signatures do that — but it keeps
the client's cleartext-traffic policy happy.

Restrict the deploy key in `/srv/fdroid/.ssh/authorized_keys`:

```
command="rrsync /srv/fdroid/fdroid",restrict ssh-ed25519 AAAA... deploy
```

A leaked deploy key then gets rsync into that one directory and no shell. Note
this is read **and** write, not `rrsync -wo`: the publish job pulls the existing
repo down before regenerating the index.

## One-time: the F-Droid repository

Only needed for the local flow; CI assembles its working tree from scratch each
run.

```sh
pipx install fdroidserver
mkdir -p ~/snot-fdroid && cd ~/snot-fdroid
fdroid init --distinguished-name "CN=<host>, OU=F-Droid"
ln -s ~/p/snot/fdroid/metadata metadata     # keep one source of metadata
```

Set `repo_url`, `repo_name`, `archive_older: 3`, `archive_url` and
`serverwebroot` in `config.yml`, then point `SNOT_FDROID_DIR` at the directory.
`fdroid init` prints the fingerprint users need:

```
https://<host>/fdroid/repo?fingerprint=<fingerprint>
```

## Why a universal APK

`--split-per-abi` is deliberately not used: Tauri's generated Gradle gives every
ABI the same `versionCode`, and F-Droid rejects two APKs of one package that
share one. The universal APK is bigger but correct. For the same reason the APK
is signed out-of-band with `apksigner` rather than by a Gradle `signingConfig` —
a `signingConfig` in `gen/android` would not survive regeneration.
