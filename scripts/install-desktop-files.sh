#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
# SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
#
# Installs the desktop integration files for the current user, so that a
# development build shows the Xtrude CAD icon on Wayland.
#
# Why this is needed: on Wayland, a window cannot hand the compositor its
# own icon (winit, under eframe, ignores the icon set with `with_icon`
# there; the built-in `resources/xtrude_256.png` is used only on X11 and
# Windows). The compositor finds the icon from the window's app id,
# `org.xtrudecad.XtrudeCAD`: it looks up the desktop entry of that name,
# whose `Icon=` names an icon in the icon theme. Without an installed
# desktop entry, KDE Plasma shows its generic Wayland "W" icon in the
# title bar and the task bar, and other desktops show their own fallback.
# Packages install these files; a plain `cargo build` does not.
#
# What it installs, under $XDG_DATA_HOME (~/.local/share by default):
#   applications/org.xtrudecad.XtrudeCAD.desktop  the desktop entry, with
#       `Exec=` pointing at this checkout's binary, so the menu entry and
#       "Open with" start the build (`xtrude` need not be on PATH)
#   icons/hicolor/<size>/apps/org.xtrudecad.XtrudeCAD.{png,svg}
#   mime/packages/org.xtrudecad.XtrudeCAD.xml  the `.igz` MIME type
# and refreshes the desktop, icon and MIME caches when their tools exist.
#
# Usage:
#   scripts/install-desktop-files.sh [path/to/xtrude]   install (default:
#       target/release/xtrude, else target/debug/xtrude)
#   scripts/install-desktop-files.sh --uninstall        remove them again
#
# A window already open keeps its fallback icon; restart the app.
set -euo pipefail

APP_ID=org.xtrudecad.XtrudeCAD
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
DATA="${XDG_DATA_HOME:-$HOME/.local/share}"
DESKTOP="$DATA/applications/$APP_ID.desktop"
MIME="$DATA/mime/packages/$APP_ID.xml"

# Rebuilds the caches the desktop reads; each tool is optional.
refresh_caches() {
    if command -v update-desktop-database >/dev/null; then
        update-desktop-database -q "$DATA/applications" || true
    fi
    if command -v update-mime-database >/dev/null; then
        update-mime-database "$DATA/mime" >/dev/null || true
    fi
    if command -v gtk-update-icon-cache >/dev/null; then
        gtk-update-icon-cache -q -t "$DATA/icons/hicolor" || true
    fi
    # KDE Plasma reads its own service cache.
    for tool in kbuildsycoca6 kbuildsycoca5; do
        if command -v "$tool" >/dev/null; then
            "$tool" >/dev/null 2>&1 || true
            break
        fi
    done
}

if [[ "${1:-}" == "--uninstall" ]]; then
    rm -f "$DESKTOP" "$MIME"
    for icon in "$ROOT"/resources/icons/hicolor/*/apps/"$APP_ID".*; do
        size="$(basename "$(dirname "$(dirname "$icon")")")"
        rm -f "$DATA/icons/hicolor/$size/apps/$(basename "$icon")"
    done
    refresh_caches
    echo "Removed the $APP_ID desktop files from $DATA"
    exit 0
fi

if [[ $# -gt 0 ]]; then
    BIN="$1"
elif [[ -x "$ROOT/target/release/xtrude" ]]; then
    BIN="$ROOT/target/release/xtrude"
else
    BIN="$ROOT/target/debug/xtrude"
fi
if [[ ! -x "$BIN" ]]; then
    echo "error: no xtrude binary at $BIN (run cargo build first, or pass its path)" >&2
    exit 1
fi
BIN="$(cd "$(dirname "$BIN")" && pwd)/$(basename "$BIN")"

# The desktop entry, with Exec pointing at this build. The path is quoted
# as the Desktop Entry spec asks, in case it contains spaces.
mkdir -p "$(dirname "$DESKTOP")"
sed "s|^Exec=xtrude |Exec=\"$BIN\" |" \
    "$ROOT/packaging/linux/$APP_ID.desktop" >"$DESKTOP"

for icon in "$ROOT"/resources/icons/hicolor/*/apps/"$APP_ID".*; do
    size="$(basename "$(dirname "$(dirname "$icon")")")"
    install -Dm644 "$icon" "$DATA/icons/hicolor/$size/apps/$(basename "$icon")"
done

install -Dm644 "$ROOT/packaging/linux/$APP_ID.mime.xml" "$MIME"

refresh_caches
echo "Installed the $APP_ID desktop files in $DATA (Exec: $BIN)"
