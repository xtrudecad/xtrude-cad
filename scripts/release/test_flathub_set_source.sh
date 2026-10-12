#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
# SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
#
# Tests flathub_set_source.sh, the edit flathub-update.yml makes to the
# Flathub copy of the manifest, on a git-source copy of the in-tree
# manifest (scripts/release/fixtures/flathub_manifest.yml) and the
# expected result, so that a mistake shows here rather than in a broken
# Flathub pull request. scripts/check.sh runs it.
set -euo pipefail
HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "$HERE/../.." && pwd)"
FIXTURES="$HERE/fixtures"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

TAG=v0.1.0
COMMIT=fedcba9876543210fedcba9876543210fedcba98

failed=0
fail() {
    echo "FAIL: $*" >&2
    failed=1
}

# Runs the script on a copy of a manifest; expects its exit status, and
# on failure a manifest left as it was.
expect_set() {
    local want="$1" name="$2" manifest="$3" tag="${4:-$TAG}" commit="${5:-$COMMIT}" got=0
    cp "$manifest" "$TMP/$name.yml"
    "$HERE/flathub_set_source.sh" "$TMP/$name.yml" "$tag" "$commit" >"$TMP/out" 2>&1 || got=$?
    if [[ "$got" != "$want" ]]; then
        fail "$name: exited $got, expected $want: $(cat "$TMP/out")"
        return 1
    fi
    if [[ "$want" != 0 ]] && ! cmp -s "$manifest" "$TMP/$name.yml"; then
        fail "$name: the manifest changed although the script failed"
    fi
}

# The release's tag and commit on the xtrude-cad source; every other line
# (comments, the manifold module's git sources) unchanged.
if expect_set 0 fixture "$FIXTURES/flathub_manifest.yml"; then
    diff -u "$FIXTURES/flathub_manifest.expected.yml" "$TMP/fixture.yml" >"$TMP/diff" ||
        fail "fixture: not the expected manifest: $(cat "$TMP/diff")"
fi

# The in-tree manifest: a `dir` source, no git source in the module.
if expect_set 1 in-tree "$ROOT/packaging/flatpak/org.xtrudecad.XtrudeCAD.yml"; then
    grep -q 'expected one git source in the xtrude-cad module, found 0' "$TMP/out" ||
        fail "in-tree: unexpected message: $(cat "$TMP/out")"
fi

# Two git sources in the module: which to set is ambiguous.
sed '$a\      - type: git\n        url: https://example.org/other.git\n        tag: v1\n        commit: 0000000000000000000000000000000000000000' \
    "$FIXTURES/flathub_manifest.yml" >"$TMP/two-sources.in"
if expect_set 1 two-sources "$TMP/two-sources.in"; then
    grep -q 'found 2' "$TMP/out" || fail "two-sources: unexpected message: $(cat "$TMP/out")"
fi

# A git source without its commit line.
grep -v '^        commit: 0123456789abcdef' "$FIXTURES/flathub_manifest.yml" >"$TMP/no-commit.in"
if expect_set 1 no-commit "$TMP/no-commit.in"; then
    grep -q 'has 0 `commit` lines' "$TMP/out" || fail "no-commit: unexpected message: $(cat "$TMP/out")"
fi

# Usage errors exit 2: a short commit hash, a tag with a space, no manifest.
expect_set 2 short-commit "$FIXTURES/flathub_manifest.yml" "$TAG" fedcba98 || true
expect_set 2 bad-tag "$FIXTURES/flathub_manifest.yml" "v0.1.0 x" || true
if "$HERE/flathub_set_source.sh" "$TMP/missing.yml" "$TAG" "$COMMIT" >/dev/null 2>&1; then
    fail "flathub_set_source.sh accepted a missing manifest"
fi

if [[ $failed -ne 0 ]]; then
    echo "Flathub source tests failed" >&2
    exit 1
fi
echo "Flathub source tests passed"
