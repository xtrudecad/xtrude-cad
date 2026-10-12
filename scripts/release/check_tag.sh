#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
# SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
#
# Checks that a release tag names the workspace version: the tag without
# its leading `v` must equal the version exactly (`v0.1.0` for 0.1.0).
# The release workflow runs it first, so a tag pushed before the version
# bump (or with a typo) fails before anything is built.
#
# Usage: scripts/release/check_tag.sh <tag> [version]
#   version: the version to compare with; by default the workspace
#   version, read with `cargo metadata` (as build_tarball.sh does).
set -euo pipefail

if [[ $# -lt 1 || $# -gt 2 ]]; then
    echo "usage: $0 <tag, e.g. v0.1.0> [version]" >&2
    exit 2
fi
TAG="$1"

if [[ $# -eq 2 ]]; then
    VERSION="$2"
else
    cd "$(dirname "${BASH_SOURCE[0]}")/../.."
    # `cargo metadata` prints each package as {"name":…,"version":…,…}.
    VERSION="$(cargo metadata --no-deps --format-version 1 --locked |
        sed -nE 's/.*"name":"xtrude-app","version":"([^"]+)".*/\1/p')"
fi
if [[ ! "$VERSION" =~ ^[0-9]+\.[0-9]+\.[0-9]+ ]]; then
    echo "error: could not read the workspace version (got '$VERSION')" >&2
    exit 2
fi

if [[ "$TAG" != v* ]]; then
    echo "error: tag '$TAG' does not start with 'v' (expected v$VERSION)" >&2
    exit 1
fi
if [[ "${TAG#v}" != "$VERSION" ]]; then
    echo "error: tag '$TAG' does not match the workspace version $VERSION" >&2
    echo "       (expected v$VERSION; bump the version in Cargo.toml and the" >&2
    echo "       metainfo's newest <release> first, then tag that commit)" >&2
    exit 1
fi
echo "tag $TAG matches the workspace version $VERSION"
