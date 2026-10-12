#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
# SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
#
# Tells whether a release tag names a pre-release: a SemVer version with
# a pre-release part (`v0.1.0-rc.1`, `v0.2.0-beta.2`). The release
# workflow makes its draft a pre-release then, so publishing it never
# reaches Flathub (flathub-update.yml skips pre-releases).
#
# Prints `true` or `false`; exits 2 when the tag is not `v` followed by a
# SemVer version.
#
# Usage: scripts/release/prerelease.sh <tag>
set -euo pipefail

if [[ $# -ne 1 ]]; then
    echo "usage: $0 <tag, e.g. v0.1.0-rc.1>" >&2
    exit 2
fi
TAG="$1"

# vMAJOR.MINOR.PATCH, an optional -pre-release, an optional +build.
ident='[0-9A-Za-z-]+'
if [[ ! "$TAG" =~ ^v[0-9]+\.[0-9]+\.[0-9]+(-$ident(\.$ident)*)?(\+$ident(\.$ident)*)?$ ]]; then
    echo "error: tag '$TAG' is not a version tag (vX.Y.Z or vX.Y.Z-pre)" >&2
    exit 2
fi
if [[ -n "${BASH_REMATCH[1]}" ]]; then
    echo true
else
    echo false
fi
