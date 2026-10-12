#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
# SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
#
# Regenerates packaging/flatpak/cargo-sources.json from Cargo.lock. The
# Flatpak builds offline: flatpak-builder downloads every crate this file
# lists into the build's CARGO_HOME, and cargo then builds with
# --offline. Run it after every change to Cargo.lock.
#
# It uses flatpak-cargo-generator.py from flatpak-builder-tools
# (https://github.com/flatpak/flatpak-builder-tools, MIT), at the commit
# and checksum pinned below, downloaded into a temporary folder
# (mktemp -d, removed on exit) with its Python modules in a virtualenv
# there. Nothing is installed elsewhere. It needs python3 with venv,
# curl, and network access to GitHub, PyPI and crates.io.
#
# Usage: scripts/release/update_cargo_sources.sh
set -euo pipefail

GENERATOR_COMMIT=74697c75b630d7330e77250fc13cb5ea688d9479
GENERATOR_SHA256=0a2db6be87d75910facef28ab46d4d6460802e8419ab850d0caa6a364d26b380
GENERATOR_URL="https://raw.githubusercontent.com/flatpak/flatpak-builder-tools/$GENERATOR_COMMIT/cargo/flatpak-cargo-generator.py"
# The generator's own requirements (its PEP 723 header), pinned.
PYTHON_DEPS=(aiohttp==3.14.5 tomlkit==0.15.1)

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
OUT="$ROOT/packaging/flatpak/cargo-sources.json"

TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

curl -sSfL -o "$TMP/flatpak-cargo-generator.py" "$GENERATOR_URL"
echo "$GENERATOR_SHA256  $TMP/flatpak-cargo-generator.py" | sha256sum --quiet -c -

python3 -m venv "$TMP/venv"
"$TMP/venv/bin/pip" install --quiet --disable-pip-version-check "${PYTHON_DEPS[@]}"

"$TMP/venv/bin/python" "$TMP/flatpak-cargo-generator.py" \
    "$ROOT/Cargo.lock" -o "$OUT"
echo "$OUT"
