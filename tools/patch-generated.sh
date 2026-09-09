#!/usr/bin/env bash
#
# Put back what the generated mobile projects cannot keep for themselves.
#
# Two things, both of which live inside gen/ and neither of which survives an
# init.
#
# 1. The platform permissions a voice recording needs. Recording is asked for
#    by the webview, but granted by the platform:
#
#      Android  wry's RustWebChromeClient already turns the webview's
#               AUDIO_CAPTURE request into a runtime request for RECORD_AUDIO
#               and MODIFY_AUDIO_SETTINGS. Android auto-denies a runtime
#               request for a permission the manifest never declared, and
#               Tauri's generated manifest declares only INTERNET — so without
#               these two lines the grant fails silently and the microphone
#               never opens.
#
#      iOS      a usage string in Info.plist. Without it the system does not
#               deny the request, it kills the process.
#
# 2. Android's back button. wry knows how to answer it out of the webview's
#    history, and Tauri's generated activity switches that off, which leaves
#    the first press closing the app from whatever pane, drawer or dialog was
#    covering the screen. The UI keeps a history entry behind each of those,
#    so the activity is patched to hand the press back to the webview.
#
# None of these files can be edited once and committed: gen/ is produced by
# `cargo tauri <platform> init` and is gitignored, so a fresh checkout — CI, or
# a variant switch that deletes the tree — starts from the stock template every
# time. This runs after init instead, and is idempotent, so running it against
# an already-patched tree is a no-op rather than a duplicated declaration.
#
# Usage: tools/patch-generated.sh
#
# Patches whichever generated projects are present and says what it did.

set -euo pipefail

readonly REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
readonly GEN_DIR="$REPO_ROOT/crates/snot-app/gen"
readonly MANIFEST="$GEN_DIR/android/app/src/main/AndroidManifest.xml"
readonly ANDROID_SRC="$GEN_DIR/android/app/src/main/java"
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

# The activity Tauri generates is `class MainActivity : TauriActivity()`, with
# or without a body depending on the template — both shapes are patched.
patch_android_back() {
    local activity
    activity="$(find "$ANDROID_SRC" -name MainActivity.kt 2>/dev/null | head -n 1)"
    [ -n "$activity" ] || return 0

    if grep -q handleBackNavigation "$activity"; then
        note "${activity#"$REPO_ROOT"/} already hands the back button to the webview"
        return 0
    fi

    awk '
        !done && /class MainActivity.*TauriActivity\(\)/ {
            if ($0 ~ /\{[[:space:]]*$/) {
                print
            } else {
                printf "%s {\n", $0
                opened = 1
            }
            print "  // Android asks the activity what a back press means, and the"
            print "  // generated one says: leave. That closed the app from whatever"
            print "  // pane, drawer or dialog was covering the screen. The UI keeps a"
            print "  // history entry behind each of those, so the press goes to the"
            print "  // webview history instead and only leaves once none are left."
            print "  override val handleBackNavigation: Boolean = true"
            print ""
            done = 1
            next
        }
        { print }
        END { if (opened) print "}" }
    ' "$activity" > "$activity.patched"
    grep -q handleBackNavigation "$activity.patched" || {
        rm -f "$activity.patched"
        printf 'error: %s declares no MainActivity to patch\n' "$activity" >&2
        exit 1
    }
    mv "$activity.patched" "$activity"
    note "handed the back button to the webview in ${activity#"$REPO_ROOT"/}"
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
patch_android_back
patch_ios
