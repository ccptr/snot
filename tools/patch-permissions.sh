#!/usr/bin/env bash
#
# Declare the platform permissions a voice recording needs, in the generated
# mobile projects.
#
# Recording is asked for by the webview, but granted by the platform:
#
#   Android  wry's RustWebChromeClient already turns the webview's AUDIO_CAPTURE
#            request into a runtime request for RECORD_AUDIO and
#            MODIFY_AUDIO_SETTINGS. Android auto-denies a runtime request for a
#            permission the manifest never declared, and Tauri's generated
#            manifest declares only INTERNET — so without these two lines the
#            grant fails silently and the microphone never opens.
#
#   iOS      a usage string in Info.plist. Without it the system does not deny
#            the request, it kills the process.
#
# Neither file can be edited once and committed: gen/ is produced by
# `cargo tauri <platform> init` and is gitignored, so a fresh checkout — CI, or
# a variant switch that deletes the tree — starts from the stock template every
# time. This runs after init instead, and is idempotent, so running it against
# an already-patched tree is a no-op rather than a duplicated declaration.
#
# Usage: tools/patch-permissions.sh
#
# Patches whichever generated projects are present and says what it did.

set -euo pipefail

readonly REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
readonly GEN_DIR="$REPO_ROOT/crates/snot-app/gen"
readonly MANIFEST="$GEN_DIR/android/app/src/main/AndroidManifest.xml"
readonly MIC_REASON="Snot records voice notes you attach to a note. The recording stays on this device."

note() { printf '    %s\n' "$*"; }

# ---------------------------------------------------------------------- android

patch_android() {
    [ -f "$MANIFEST" ] || return 0

    local missing="" perm
    for perm in android.permission.RECORD_AUDIO android.permission.MODIFY_AUDIO_SETTINGS; do
        grep -q "android:name=\"$perm\"" "$MANIFEST" || missing="$missing $perm"
    done
    if [ -z "$missing" ]; then
        note "the android manifest already declares the audio permissions"
        return 0
    fi

    # Beside INTERNET, which the stock template already declares, so the
    # patched file still reads like the template rather than a patch.
    awk -v perms="$missing" '
        { print }
        !done && /uses-permission android:name="android.permission.INTERNET"/ {
            match($0, /^[[:space:]]*/)
            indent = substr($0, 1, RLENGTH)
            n = split(perms, list, " ")
            for (i = 1; i <= n; i++) {
                printf "%s<uses-permission android:name=\"%s\" />\n", indent, list[i]
            }
            done = 1
        }
    ' "$MANIFEST" > "$MANIFEST.patched"
    grep -q RECORD_AUDIO "$MANIFEST.patched" || {
        rm -f "$MANIFEST.patched"
        printf 'error: %s declares no INTERNET permission to patch beside\n' "$MANIFEST" >&2
        exit 1
    }
    mv "$MANIFEST.patched" "$MANIFEST"
    note "declared${missing// / } in ${MANIFEST#"$REPO_ROOT"/}"
}

# -------------------------------------------------------------------------- ios

patch_ios() {
    [ -d "$GEN_DIR/apple" ] || return 0

    local plist found=0
    while IFS= read -r plist; do
        found=1
        if grep -q "NSMicrophoneUsageDescription" "$plist"; then
            note "${plist#"$REPO_ROOT"/} already explains the microphone"
            continue
        fi
        if [ -x /usr/libexec/PlistBuddy ]; then
            /usr/libexec/PlistBuddy -c \
                "Add :NSMicrophoneUsageDescription string \"$MIC_REASON\"" "$plist"
        else
            # No PlistBuddy off a Mac. The root <dict> is the first one in the
            # file, so inserting straight after it cannot land inside a nested
            # dictionary.
            awk -v reason="$MIC_REASON" '
                { print }
                !done && /<dict>/ {
                    print "\t<key>NSMicrophoneUsageDescription</key>"
                    printf "\t<string>%s</string>\n", reason
                    done = 1
                }
            ' "$plist" > "$plist.patched"
            mv "$plist.patched" "$plist"
        fi
        note "explained the microphone in ${plist#"$REPO_ROOT"/}"
    done < <(find "$GEN_DIR/apple" -maxdepth 2 -name Info.plist)

    [ "$found" -eq 1 ] || note "no Info.plist under gen/apple yet — nothing to patch"
}

patch_android
patch_ios
