#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
# SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
#
# Tests packaging/linux/install.sh, the release tarball's installer,
# without a release build: it lays out a tarball tree as
# build_tarball.sh does (dummy binaries, the real desktop entry,
# metainfo, MIME type and icons), installs it into a temporary prefix,
# checks the installed files and the desktop entry's rewritten Exec=,
# uninstalls, and checks that the prefix holds nothing the script added.
# It runs twice: with the desktop, MIME and icon cache tools on PATH (the
# ones this machine has) and without them. scripts/check.sh runs it.
set -euo pipefail
HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "$HERE/../.." && pwd)"
APP_ID=org.xtrudecad.XtrudeCAD
MIME_TYPE=application/x-ingetrazo
CACHE_TOOLS=(update-desktop-database update-mime-database gtk-update-icon-cache)
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

failed=0
fail() {
    echo "FAIL: $*" >&2
    failed=1
}

# The tarball tree, as build_tarball.sh stages it.
TREE="$TMP/xtrude-cad-0.0.0-x86_64-linux"
mkdir -p "$TREE/bin"
for bin in xtrude xtrude-step; do
    printf '#!/bin/sh\necho %s\n' "$bin" >"$TREE/bin/$bin"
    chmod 755 "$TREE/bin/$bin"
done
install -Dm644 "$ROOT/packaging/linux/$APP_ID.desktop" \
    "$TREE/share/applications/$APP_ID.desktop"
install -Dm644 "$ROOT/packaging/linux/$APP_ID.metainfo.xml" \
    "$TREE/share/metainfo/$APP_ID.metainfo.xml"
install -Dm644 "$ROOT/packaging/linux/$APP_ID.mime.xml" \
    "$TREE/share/mime/packages/$APP_ID.xml"
for icon in "$ROOT"/resources/icons/hicolor/*/apps/"$APP_ID".*; do
    size="$(basename "$(dirname "$(dirname "$icon")")")"
    install -Dm644 "$icon" "$TREE/share/icons/hicolor/$size/apps/$(basename "$icon")"
done
install -m755 "$ROOT/packaging/linux/install.sh" "$TREE/install.sh"
TREE_FILES="$(cd "$TREE" && find bin share -type f | LC_ALL=C sort)"

# A PATH without the cache tools: links to every other command on PATH.
NO_TOOLS="$TMP/no-cache-tools"
mkdir -p "$NO_TOOLS"
IFS=: read -ra path_dirs <<<"$PATH"
for d in "${path_dirs[@]}"; do
    [[ -d "$d" ]] || continue
    find "$d" -maxdepth 1 -mindepth 1 ! -type d \
        ! -name update-desktop-database ! -name update-mime-database \
        ! -name gtk-update-icon-cache \
        -exec ln -s -t "$NO_TOOLS" {} + 2>/dev/null || true
done

# The files (type f) and folders (type d) under a prefix, relative to it.
listing() {
    (cd "$1" && find . -mindepth 1 -type "$2" | sed 's|^\./||' | LC_ALL=C sort)
}

# Installs into a fresh prefix and uninstalls, with the given PATH.
# The prefix's name has a space and a % so that the Exec= quoting shows.
round_trip() {
    local name="$1" path="$2" tools="$3" prefix f mode want exec_line left
    prefix="$TMP/$name/my apps 100%"
    mkdir -p "$prefix/bin" "$prefix/share/applications"
    # Files of other programs, which the script must leave alone.
    echo other >"$prefix/bin/other"
    echo other >"$prefix/share/applications/other.desktop"

    if ! PATH="$path" PREFIX="$prefix" HOME="$TMP/home" \
        "$TREE/install.sh" >"$TMP/out" 2>&1; then
        fail "$name: install.sh failed: $(cat "$TMP/out")"
        return
    fi

    # Every file of the tree, installed with its mode, the same bytes
    # except the desktop entry.
    while IFS= read -r f; do
        if [[ ! -f "$prefix/$f" ]]; then
            fail "$name: $f not installed"
            continue
        fi
        case "$f" in
            bin/*) want=755 ;;
            *) want=644 ;;
        esac
        mode="$(stat -c %a "$prefix/$f")"
        [[ "$mode" == "$want" ]] || fail "$name: $f installed with mode $mode, expected $want"
        if [[ "$f" != "share/applications/$APP_ID.desktop" ]]; then
            cmp -s "$TREE/$f" "$prefix/$f" || fail "$name: $f differs from the tarball's"
        fi
    done <<<"$TREE_FILES"
    "$prefix/bin/xtrude" >/dev/null || fail "$name: the installed bin/xtrude does not run"

    # The desktop entry runs the installed binary by its absolute path,
    # quoted, with % doubled; every other line is unchanged.
    exec_line="$(grep '^Exec=' "$prefix/share/applications/$APP_ID.desktop")"
    want="Exec=\"${prefix//%/%%}/bin/xtrude\" %f"
    [[ "$exec_line" == "$want" ]] ||
        fail "$name: desktop entry has '$exec_line', expected '$want'"
    diff <(grep -v '^Exec=' "$TREE/share/applications/$APP_ID.desktop") \
        <(grep -v '^Exec=' "$prefix/share/applications/$APP_ID.desktop") >/dev/null ||
        fail "$name: the installed desktop entry changed more than its Exec= line"

    # With the tools, the caches name the app and its type.
    if [[ "$tools" == yes ]]; then
        if PATH="$path" command -v update-desktop-database >/dev/null; then
            grep -q "$APP_ID.desktop" "$prefix/share/applications/mimeinfo.cache" 2>/dev/null ||
                fail "$name: mimeinfo.cache does not name $APP_ID.desktop after install"
        fi
        if PATH="$path" command -v update-mime-database >/dev/null; then
            grep -qx "$MIME_TYPE" "$prefix/share/mime/types" 2>/dev/null ||
                fail "$name: share/mime/types has no $MIME_TYPE after install"
        fi
        if PATH="$path" command -v gtk-update-icon-cache >/dev/null; then
            [[ -f "$prefix/share/icons/hicolor/icon-theme.cache" ]] ||
                fail "$name: no icon-theme.cache after install"
        fi
    fi

    if ! PATH="$path" PREFIX="$prefix" HOME="$TMP/home" \
        "$TREE/install.sh" --uninstall >"$TMP/out" 2>&1; then
        fail "$name: install.sh --uninstall failed: $(cat "$TMP/out")"
        return
    fi

    # What is left: the other programs' files, the prefix's bin/ and
    # share/, and what uninstall leaves on purpose: the caches the tools
    # write and the empty share/mime/packages update-mime-database needs.
    left="$(listing "$prefix" f | grep -vxE \
        'bin/other|share/applications/other\.desktop|share/applications/mimeinfo\.cache|share/icons/hicolor/icon-theme\.cache|share/mime/[^/]+|share/mime/[^/]+/[^/]+' |
        grep -v '^share/mime/packages/' || true)"
    [[ -z "$left" ]] || fail "$name: files left after uninstall: $left"
    if [[ -n "$(listing "$prefix/share/mime/packages" f 2>/dev/null)" ]]; then
        fail "$name: share/mime/packages not empty after uninstall"
    fi
    left="$(listing "$prefix" d | grep -vxE \
        'bin|share|share/applications|share/icons|share/icons/hicolor|share/mime(/.*)?' || true)"
    [[ -z "$left" ]] || fail "$name: folders left after uninstall: $left"
    if grep -rqs "$MIME_TYPE" "$prefix/share/mime" --exclude-dir=packages; then
        fail "$name: the MIME cache still names $MIME_TYPE after uninstall"
    fi
    if grep -qs "$APP_ID" "$prefix/share/applications/mimeinfo.cache"; then
        fail "$name: mimeinfo.cache still names $APP_ID after uninstall"
    fi
    if [[ "$tools" == no ]]; then
        # No tool ran, so no cache was written either.
        left="$(listing "$prefix" f | grep -vxE 'bin/other|share/applications/other\.desktop' || true)"
        [[ -z "$left" ]] || fail "$name: files left after uninstall without the tools: $left"
    fi
}

missing=()
for tool in "${CACHE_TOOLS[@]}"; do
    command -v "$tool" >/dev/null || missing+=("$tool")
done
if [[ ${#missing[@]} -gt 0 ]]; then
    echo "note: not on PATH, so not tested: ${missing[*]}" >&2
fi
round_trip with-tools "$PATH" yes
round_trip without-tools "$NO_TOOLS" no

# Usage errors exit 2; a folder without bin/xtrude is refused.
got=0
"$TREE/install.sh" --bogus >/dev/null 2>&1 || got=$?
[[ "$got" == 2 ]] || fail "install.sh --bogus exited $got, expected 2"
mkdir -p "$TMP/not-a-tarball"
cp "$TREE/install.sh" "$TMP/not-a-tarball/"
if PREFIX="$TMP/unused" "$TMP/not-a-tarball/install.sh" >/dev/null 2>&1; then
    fail "install.sh ran from a folder that is not a tarball"
fi
[[ ! -e "$TMP/unused" ]] || fail "install.sh wrote into the prefix from a folder that is not a tarball"

if [[ $failed -ne 0 ]]; then
    echo "install.sh tests failed" >&2
    exit 1
fi
echo "install.sh tests passed"
