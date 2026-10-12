#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
# SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
#
# Installs Xtrude CAD from the unpacked release tarball, for the current
# user by default. Run it from the tarball's folder (it sits at its root,
# next to bin/ and share/):
#
#   ./install.sh                 install into $PREFIX (~/.local by default)
#   ./install.sh --uninstall     remove the files it installed
#   ./install.sh --help
#
# It copies bin/xtrude, bin/xtrude-step (the STEP helper, which the app
# looks for next to its own binary) and the share/ tree (desktop entry,
# metainfo, MIME type, icons) into the prefix. The installed desktop
# entry runs the installed binary by its absolute path, since the prefix's
# bin/ need not be on PATH. The desktop, MIME and icon caches of the
# prefix are refreshed when their tools exist.
#
# The files installed and removed are the files under bin/ and share/ in
# the tarball, so --uninstall must run from the same release's folder.
# It writes nothing outside the prefix and never asks for root; for a
# system-wide install, run it as root with PREFIX=/usr/local.
set -euo pipefail

APP_ID=org.xtrudecad.XtrudeCAD
SRC="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

usage() {
    cat <<EOF
Usage: $0 [--uninstall | --help]

Installs Xtrude CAD into \$PREFIX (now: ${PREFIX:-$HOME/.local}):
  \$PREFIX/bin/xtrude, \$PREFIX/bin/xtrude-step and the desktop
  integration files under \$PREFIX/share.

  --uninstall   remove the files this release installs, then refresh
                the desktop, MIME and icon caches
  --help        show this help
EOF
}

MODE=install
case "${1:-}" in
    "") ;;
    --uninstall) MODE=uninstall ;;
    -h | --help)
        usage
        exit 0
        ;;
    *)
        usage >&2
        exit 2
        ;;
esac
if [[ $# -gt 1 ]]; then
    usage >&2
    exit 2
fi

if [[ ! -x "$SRC/bin/xtrude" || ! -d "$SRC/share" ]]; then
    echo "error: $SRC is not an unpacked Xtrude CAD tarball (no bin/xtrude or share/)" >&2
    exit 1
fi

PREFIX="${PREFIX:-$HOME/.local}"
if [[ -z "$PREFIX" ]]; then
    echo "error: PREFIX is empty" >&2
    exit 1
fi
# The desktop entry needs an absolute path to the binary.
if [[ "$PREFIX" != /* ]]; then
    PREFIX="$PWD/$PREFIX"
fi
PREFIX="${PREFIX%/}"

# The files the tarball carries, relative to its root: one per line,
# sorted. Install and uninstall go through this same list.
FILES=()
while IFS= read -r f; do
    FILES+=("$f")
done < <(cd "$SRC" && find bin share -type f | LC_ALL=C sort)

DESKTOP="share/applications/$APP_ID.desktop"

# Rebuilds the caches of the prefix that the desktop reads; each tool is
# optional, and each runs only on a folder that exists.
refresh_caches() {
    local dir
    dir="$PREFIX/share/applications"
    if [[ -d "$dir" ]] && command -v update-desktop-database >/dev/null; then
        update-desktop-database -q "$dir" || true
    fi
    dir="$PREFIX/share/mime"
    if [[ -d "$dir/packages" ]] && command -v update-mime-database >/dev/null; then
        update-mime-database "$dir" >/dev/null || true
    fi
    dir="$PREFIX/share/icons/hicolor"
    if [[ -d "$dir" ]] && command -v gtk-update-icon-cache >/dev/null; then
        gtk-update-icon-cache -q -t -f "$dir" || true
    fi
}

# Quotes a path as one argument of a desktop entry's Exec key: inside
# double quotes, `"`, `` ` ``, `$` and `\` take a backslash; then, as in
# every string value, `\` itself is doubled, and `%` (a field code) is
# doubled too.
exec_quote() {
    local s="$1"
    s="${s//\\/\\\\}"
    s="${s//\"/\\\"}"
    s="${s//\`/\\\`}"
    s="${s//\$/\\\$}"
    s="${s//\\/\\\\}"
    s="${s//%/%%}"
    printf '"%s"' "$s"
}

if [[ "$MODE" == uninstall ]]; then
    for f in "${FILES[@]}"; do
        rm -f "$PREFIX/$f"
    done
    # The folders left empty, deepest first, up to (not including) the
    # prefix's own bin/ and share/. share/mime/packages stays:
    # update-mime-database refuses to run without it, and the MIME cache
    # beside it still names the removed type until it runs.
    while IFS= read -r d; do
        [[ "$d" == share/mime/packages ]] && continue
        rmdir "$PREFIX/$d" 2>/dev/null || true
    done < <(for f in "${FILES[@]}"; do
        d="$(dirname "$f")"
        while [[ "$d" == */* ]]; do
            echo "$d"
            d="$(dirname "$d")"
        done
    done | LC_ALL=C sort -u -r)
    refresh_caches
    echo "Removed Xtrude CAD from $PREFIX"
    exit 0
fi

for f in "${FILES[@]}"; do
    case "$f" in
        bin/*) mode=755 ;;
        *) mode=644 ;;
    esac
    install -Dm"$mode" "$SRC/$f" "$PREFIX/$f"
done

# The desktop entry runs the installed binary.
BIN="$PREFIX/bin/xtrude"
EXEC="$(exec_quote "$BIN")"
while IFS= read -r line || [[ -n "$line" ]]; do
    if [[ "$line" == "Exec=xtrude "* ]]; then
        line="Exec=$EXEC ${line#Exec=xtrude }"
    elif [[ "$line" == "Exec=xtrude" ]]; then
        line="Exec=$EXEC"
    fi
    printf '%s\n' "$line"
done <"$SRC/$DESKTOP" >"$PREFIX/$DESKTOP"

refresh_caches
echo "Installed Xtrude CAD in $PREFIX (run: $BIN)"
case ":$PATH:" in
    *":$PREFIX/bin:"*) ;;
    *) echo "Note: $PREFIX/bin is not on PATH; the desktop menu entry works without it." ;;
esac
