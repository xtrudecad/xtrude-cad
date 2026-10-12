#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
# SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
#
# Builds the portable x86_64 Linux tarball,
# xtrude-cad-X.Y.Z-x86_64-linux.tar.gz:
#
#   xtrude-cad-X.Y.Z-x86_64-linux/
#     bin/xtrude
#     bin/xtrude-step        the STEP helper, built with OpenCascade
#     share/applications/org.xtrudecad.XtrudeCAD.desktop
#     share/metainfo/org.xtrudecad.XtrudeCAD.metainfo.xml
#     share/mime/packages/org.xtrudecad.XtrudeCAD.xml
#     share/icons/hicolor/<size>/apps/org.xtrudecad.XtrudeCAD.{svg,png}
#     install.sh   LICENSE   README.md   AUTHORS
#
# The binaries link only libc, libm, libgcc_s and libstdc++ (winit, wgpu
# and the file dialogs load the rest at run time), so what limits where
# they run is the glibc they were built against. Release builds run in an
# AlmaLinux 8 container (glibc 2.28); check each binary afterwards with
# scripts/release/check_glibc.sh <binary> 2.28.
#
# The version is the workspace version, read with `cargo metadata`, not
# the tag. The app finds the STEP helper next to its own binary.
#
# Usage: scripts/release/build_tarball.sh [output dir]
#   default output dir: target/release-dist/
# Cargo's target directory is $CARGO_TARGET_DIR when set, else target/.
#
# The release build, reproduced locally in the same container (the
# separate target directory keeps the container's objects apart from the
# host's; keep it on disk, not on a tmpfs /tmp; the first build of
# OpenCascade takes about 20 minutes). AlmaLinux 8's own GCC 8 lacks the
# C++17 parallel algorithms that Manifold (the solid booleans) uses, so the
# build takes the newer GCC of gcc-toolset-14: it targets the same glibc,
# and links the newer parts of libstdc++ into the binary.
#
#   podman run --rm -v "$PWD":/src:z -w /src \
#       -e CARGO_TARGET_DIR=/src/target/almalinux8 \
#       -e CARGO_HOME=/src/target/almalinux8-cargo \
#       -e RUSTUP_HOME=/src/target/almalinux8-rustup \
#       almalinux:8 bash -c '
#         dnf install -y -q gcc-toolset-14-gcc gcc-toolset-14-gcc-c++ \
#             make cmake perl git binutils tar gzip &&
#         source /opt/rh/gcc-toolset-14/enable &&
#         curl -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal \
#             --default-toolchain none --no-modify-path &&
#         export PATH="$CARGO_HOME/bin:$PATH" &&
#         scripts/release/build_tarball.sh &&
#         scripts/release/check_glibc.sh target/almalinux8/release/xtrude 2.28 &&
#         scripts/release/check_glibc.sh target/almalinux8/release/xtrude-step 2.28'
#
# rustup installs the toolchain that rust-toolchain.toml pins.
set -euo pipefail

APP_ID=org.xtrudecad.XtrudeCAD
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$ROOT"
OUT="${1:-target/release-dist}"
TARGET_DIR="${CARGO_TARGET_DIR:-target}"

# `cargo metadata` prints each package as {"name":…,"version":…,…}.
VERSION="$(cargo metadata --no-deps --format-version 1 --locked |
    sed -nE 's/.*"name":"xtrude-app","version":"([^"]+)".*/\1/p')"
if [[ ! "$VERSION" =~ ^[0-9]+\.[0-9]+\.[0-9]+ ]]; then
    echo "error: could not read the workspace version from cargo metadata" >&2
    exit 1
fi
NAME="xtrude-cad-$VERSION-x86_64-linux"

cargo build --release --locked -p xtrude-app
# OpenCascade's CMake files predate CMake 4, which refuses them without
# a policy minimum.
CMAKE_POLICY_VERSION_MINIMUM=3.5 \
    cargo build --release --locked -p xtrude-step --features occt

STAGE="$TARGET_DIR/release-dist-stage"
DIR="$STAGE/$NAME"
rm -rf "$STAGE"
mkdir -p "$DIR/bin"

for bin in xtrude xtrude-step; do
    install -m755 "$TARGET_DIR/release/$bin" "$DIR/bin/$bin"
    strip "$DIR/bin/$bin"
done

install -Dm644 "packaging/linux/$APP_ID.desktop" \
    "$DIR/share/applications/$APP_ID.desktop"
install -Dm644 "packaging/linux/$APP_ID.metainfo.xml" \
    "$DIR/share/metainfo/$APP_ID.metainfo.xml"
install -Dm644 "packaging/linux/$APP_ID.mime.xml" \
    "$DIR/share/mime/packages/$APP_ID.xml"
for icon in resources/icons/hicolor/*/apps/"$APP_ID".*; do
    size="$(basename "$(dirname "$(dirname "$icon")")")"
    install -Dm644 "$icon" "$DIR/share/icons/hicolor/$size/apps/$(basename "$icon")"
done

for doc in LICENSE README.md AUTHORS; do
    install -m644 "$doc" "$DIR/$doc"
done
install -m755 packaging/linux/install.sh "$DIR/install.sh"

# Same tree, same archive: files sorted, owned by root, dated by the
# last commit.
MTIME="$(git log -1 --format=%ct 2>/dev/null || date +%s)"
mkdir -p "$OUT"
TARBALL="$OUT/$NAME.tar.gz"
tar --sort=name --owner=0 --group=0 --numeric-owner --mtime="@$MTIME" \
    -C "$STAGE" -cf - "$NAME" | gzip -n -9 >"$TARBALL"
rm -rf "$STAGE"

echo "$TARBALL"
