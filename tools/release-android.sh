#!/usr/bin/env bash
#
# Build, sign and publish an Android variant to a self-hosted F-Droid repo.
#
# Two variants ship from one source tree:
#
#   release   app.snot.notes          version and versionCode from tauri.conf.json
#   nightly   app.snot.notes.nightly  versionCode from the commit count on HEAD
#
# They carry different application ids so both install side by side, and they
# are signed with different keys: a leaked nightly key costs the nightly app
# only. The identifier comes from tauri.nightly.conf.json rather than a Gradle
# patch, because gen/android is regenerated and not tracked.
#
# The steps are separable, because they do not all belong on the same machine:
# build and sign need the Android SDK and a key, publish needs the F-Droid repo
# working tree, deploy needs SSH to the server.
#
#   build    cargo tauri android build --apk   -> an unsigned universal APK
#   sign     zipalign + apksigner              -> <applicationId>_<code>.apk
#   publish  copy into the repo, fdroid update -> a signed index
#   deploy   fdroid deploy                     -> rsync to the web root
#
# Configuration comes from tools/release.env (gitignored, see release.env.example)
# or from the environment. Run with --help for the options.

set -euo pipefail

readonly REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
readonly APP_DIR="$REPO_ROOT/crates/snot-app"
readonly ANDROID_DIR="$APP_DIR/gen/android"
readonly UNSIGNED_APK="$ANDROID_DIR/app/build/outputs/apk/universal/release/app-universal-release-unsigned.apk"

if [ -t 1 ]; then bold=$'\033[1m' blue=$'\033[1;34m' red=$'\033[31m' off=$'\033[0m'
else bold="" blue="" red="" off=""; fi

die() { printf '%serror:%s %s\n' "$red" "$off" "$*" >&2; exit 1; }
step() { printf '\n%s==>%s %s%s%s\n' "$blue" "$off" "$bold" "$*" "$off"; }
note() { printf '    %s\n' "$*"; }

variant=release
do_build=1 do_sign=1 do_publish=1 do_deploy=1
apk_override=""

usage() {
    cat <<'USAGE'
Build, sign and publish an Android variant to a self-hosted F-Droid repo.

Usage: tools/release-android.sh [options]

Steps, all run by default:
  build    cargo tauri android build --apk   -> an unsigned universal APK
  sign     zipalign + apksigner              -> <applicationId>_<code>.apk
  publish  copy into the repo, fdroid update -> a signed index
  deploy   fdroid deploy                     -> rsync to the web root

Options:
  --variant V     release (default) or nightly
  --apk PATH      sign this APK instead of building one (implies --skip-build)
  --skip-build    reuse the unsigned APK already in gen/android
  --build-only    build the unsigned APK and stop, touching no keys
  --no-publish    stop after signing; leave the APK in dist/
  --no-deploy     publish into the local repo but do not push to the server
  -h, --help      this message
USAGE
}

while [ $# -gt 0 ]; do
    case "$1" in
        --variant) variant="${2:?--variant needs release or nightly}"; shift 2 ;;
        --apk) apk_override="${2:?--apk needs a path}"; do_build=0; shift 2 ;;
        --skip-build) do_build=0; shift ;;
        --build-only) do_sign=0; do_publish=0; do_deploy=0; shift ;;
        --no-publish) do_publish=0; do_deploy=0; shift ;;
        --no-deploy) do_deploy=0; shift ;;
        -h|--help) usage; exit 0 ;;
        *) die "unknown option: $1 (try --help)" ;;
    esac
done

# ---------------------------------------------------------------- configuration

if [ -f "$REPO_ROOT/tools/release.env" ]; then
    # shellcheck disable=SC1091
    set -a; . "$REPO_ROOT/tools/release.env"; set +a
fi

: "${ANDROID_HOME:=${ANDROID_SDK_ROOT:-$HOME/Android/Sdk}}"
: "${SNOT_DIST_DIR:=$REPO_ROOT/dist}"

# Each variant has its own application id, its own signing key and its own
# recorded certificate, so a mistake in one cannot reach the other.
config_args=()
case "$variant" in
    release)
        package_id="app.snot.notes"
        signer_file="$REPO_ROOT/tools/signer.sha256"
        keystore="${SNOT_KEYSTORE:-}"
        key_alias="${SNOT_KEY_ALIAS:-snot}"
        pass_var=SNOT_KEYSTORE_PASS
        key_pass_var=SNOT_KEY_PASS
        ;;
    nightly)
        package_id="app.snot.notes.nightly"
        signer_file="$REPO_ROOT/tools/signer-nightly.sha256"
        keystore="${SNOT_NIGHTLY_KEYSTORE:-}"
        key_alias="${SNOT_NIGHTLY_KEY_ALIAS:-snot-nightly}"
        pass_var=SNOT_NIGHTLY_KEYSTORE_PASS
        key_pass_var=SNOT_NIGHTLY_KEY_PASS

        # F-Droid only requires that versionCode increase. The commit count is
        # monotonic on a linear main branch and says something useful, unlike a
        # timestamp, which collides when two builds land in the same day.
        commits="$(git -C "$REPO_ROOT" rev-list --count HEAD)"
        short_sha="$(git -C "$REPO_ROOT" rev-parse --short HEAD)"
        base_version="$(jq -r .version "$APP_DIR/tauri.conf.json")"
        nightly_version="$base_version-nightly.$commits+$short_sha"
        config_args=(
            -c "$APP_DIR/tauri.nightly.conf.json"
            -c "{\"version\":\"$nightly_version\",\"bundle\":{\"android\":{\"versionCode\":$commits}}}"
        )
        ;;
    *) die "unknown variant: $variant (expected release or nightly)" ;;
esac

# The NDK version floats; take the newest installed unless one is pinned.
if [ -z "${NDK_HOME:-}" ] && [ -d "$ANDROID_HOME/ndk" ]; then
    NDK_HOME="$ANDROID_HOME/ndk/$(ls -1 "$ANDROID_HOME/ndk" | sort -V | tail -1)"
    export NDK_HOME
fi

# apksigner and zipalign live in a versioned build-tools directory that is not
# on PATH in a default SDK install.
build_tools_bin() {
    local tool="$1" dir
    dir="$ANDROID_HOME/build-tools/$(ls -1 "$ANDROID_HOME/build-tools" 2>/dev/null | sort -V | tail -1)"
    [ -x "$dir/$tool" ] || die "$tool not found under $ANDROID_HOME/build-tools — install the Android build-tools, or set ANDROID_HOME"
    printf '%s\n' "$dir/$tool"
}

# ------------------------------------------------------------------------ build

if [ "$do_build" -eq 1 ]; then
    step "Building the $variant APK"
    command -v cargo >/dev/null || die "cargo not found"
    cargo tauri --version >/dev/null 2>&1 || die "tauri-cli not found — cargo install tauri-cli --version '^2' --locked"
    [ -d "$NDK_HOME" ] || die "NDK_HOME is not a directory (${NDK_HOME:-unset}) — sdkmanager --install 'ndk;27.2.12479018'"

    # Gradle cannot configure this project under the newest JDKs, and a rolling
    # distro makes one of those the default; CI pins 21 for the same reason.
    # Respect an explicit JAVA_HOME, otherwise find a version Gradle supports.
    if [ -z "${JAVA_HOME:-}" ]; then
        for v in 21 17; do
            if [ -d "/usr/lib/jvm/java-$v-openjdk" ]; then
                export JAVA_HOME="/usr/lib/jvm/java-$v-openjdk"
                note "using JDK $v at $JAVA_HOME"
                break
            fi
        done
        [ -n "${JAVA_HOME:-}" ] || note "warning: no JDK 21 or 17 found; Gradle will use the default and may fail to configure"
    fi

    [ -d "$REPO_ROOT/ui/node_modules" ] || npm --prefix "$REPO_ROOT/ui" ci

    # The application id is baked into the generated Android project, down to
    # the Kotlin package directories, and Tauri refuses to change it in place.
    # Regenerate when the tree on disk belongs to the other variant.
    existing_id="$(sed -n 's/^ *applicationId = "\(.*\)"$/\1/p' "$ANDROID_DIR/app/build.gradle.kts" 2>/dev/null || true)"
    if [ -n "$existing_id" ] && [ "$existing_id" != "$package_id" ]; then
        note "gen/android holds $existing_id, rebuilding it for $package_id"
        rm -rf "$ANDROID_DIR"
    fi
    if [ ! -d "$ANDROID_DIR" ]; then
        ( cd "$REPO_ROOT" && CI=1 cargo tauri android init --ci "${config_args[@]}" )
    fi

    # A universal APK, deliberately not --split-per-abi: Tauri's generated
    # build.gradle.kts gives every ABI the same versionCode, and F-Droid rejects
    # two APKs of one package sharing a versionCode.
    ( cd "$REPO_ROOT" && cargo tauri android build "${config_args[@]}" --apk )
    [ -f "$UNSIGNED_APK" ] || die "expected an unsigned APK at $UNSIGNED_APK — did a signingConfig get added to gen/android?"
    note "built $UNSIGNED_APK"
fi

input_apk="${apk_override:-$UNSIGNED_APK}"
if [ "$do_sign" -eq 1 ] || [ "$do_publish" -eq 1 ]; then
    [ -f "$input_apk" ] || die "no APK at $input_apk — drop --skip-build to build one"
fi

# The version is whatever Tauri last wrote into the Gradle properties; fall back
# to computing it from tauri.conf.json the same way Tauri does.
props="$ANDROID_DIR/app/tauri.properties"
if [ -f "$props" ]; then
    version_name="$(sed -n 's/^tauri\.android\.versionName=//p' "$props")"
    version_code="$(sed -n 's/^tauri\.android\.versionCode=//p' "$props")"
elif [ "$variant" = nightly ]; then
    version_name="$nightly_version"
    version_code="$commits"
else
    version_name="$(jq -r .version "$APP_DIR/tauri.conf.json")"
    version_code="$(printf '%s' "$version_name" | awk -F. '{print $1*1000000 + $2*1000 + $3}')"
fi
[ -n "$version_name" ] && [ -n "$version_code" ] || die "could not determine the app version"

# ------------------------------------------------------------------------- sign

signed_apk="$SNOT_DIST_DIR/${package_id}_${version_code}.apk"

if [ "$do_sign" -eq 1 ]; then
    step "Signing $package_id $version_name (versionCode $version_code)"
    [ -n "$keystore" ] || die "no keystore for the $variant variant — set ${pass_var%_PASS} in tools/release.env"
    [ -f "$keystore" ] || die "keystore not found: $keystore"

    zipalign="$(build_tools_bin zipalign)"
    apksigner="$(build_tools_bin apksigner)"
    mkdir -p "$SNOT_DIST_DIR"

    # Sign to a staging name; an APK only reaches its published filename once
    # the certificate below has been checked.
    aligned="$SNOT_DIST_DIR/.aligned-$version_code.apk"
    staged="$SNOT_DIST_DIR/.unverified-$version_code.apk"
    trap 'rm -f "$aligned" "$staged"' EXIT

    "$zipalign" -p -f 4 "$input_apk" "$aligned"

    # Keep the passwords off the command line: apksigner reads them from the
    # environment, or prompts on a terminal when neither variable is set.
    pass_args=()
    if [ -n "${!pass_var:-}" ]; then
        pass_args+=(--ks-pass "env:$pass_var")
        if [ -n "${!key_pass_var:-}" ]; then
            pass_args+=(--key-pass "env:$key_pass_var")
        else
            # A PKCS12 keystore uses one password for the store and the key.
            pass_args+=(--key-pass "env:$pass_var")
        fi
    fi
    "$apksigner" sign --ks "$keystore" --ks-key-alias "$key_alias" \
        --v4-signing-enabled false "${pass_args[@]}" --out "$staged" "$aligned"

    # F-Droid pins the signing certificate on first install: an APK signed with
    # the wrong key is not an update, it is an app nobody can upgrade to. Check
    # against the recorded fingerprint before it can reach the repo.
    digest="$("$apksigner" verify --print-certs "$staged" \
        | sed -n 's/^Signer #1 certificate SHA-256 digest: //p')"
    [ -n "$digest" ] || die "could not read the signer certificate back from the signed APK"

    if [ -f "$signer_file" ]; then
        expected="$(tr -d '[:space:]' < "$signer_file")"
        [ "$digest" = "$expected" ] || die "signed with the wrong key!
    expected $expected
    got      $digest
  Publishing this would strand every existing install of $package_id."
        note "signer matches ${signer_file#"$REPO_ROOT"/}"
    else
        note "signer SHA-256: $digest"
        note "no ${signer_file#"$REPO_ROOT"/} yet — record it once you are sure this is the right key:"
        note "    echo $digest > ${signer_file#"$REPO_ROOT"/}"
    fi
    mv "$staged" "$signed_apk"
    note "wrote $signed_apk"
fi

# ---------------------------------------------------------------------- publish

if [ "$do_publish" -eq 1 ]; then
    step "Publishing into the F-Droid repo"
    [ -n "${SNOT_FDROID_DIR:-}" ] || die "SNOT_FDROID_DIR is not set — see tools/release.env.example"
    [ -f "$SNOT_FDROID_DIR/config.yml" ] || die "$SNOT_FDROID_DIR is not an F-Droid repo (no config.yml) — run 'fdroid init' there first"
    command -v fdroid >/dev/null || die "fdroid not found — pipx install fdroidserver"

    mkdir -p "$SNOT_FDROID_DIR/repo"
    cp "$signed_apk" "$SNOT_FDROID_DIR/repo/"

    # The first run has no metadata to update; -c stubs it out from the APK.
    update_args=()
    [ -f "$SNOT_FDROID_DIR/metadata/$package_id.yml" ] || update_args+=(-c)

    ( cd "$SNOT_FDROID_DIR" && fdroid update "${update_args[@]}" )

    if [ "${#update_args[@]}" -gt 0 ]; then
        note "created metadata/$package_id.yml — fill in Summary, Description and"
        note "License: GPL-3.0-or-later, then re-run to regenerate the index."
    fi
fi

# ----------------------------------------------------------------------- deploy

if [ "$do_deploy" -eq 1 ]; then
    step "Deploying to the server"
    ( cd "$SNOT_FDROID_DIR" && fdroid deploy )
fi

step "Done: $package_id $version_name (versionCode $version_code)"
