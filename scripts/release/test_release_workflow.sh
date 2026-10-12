#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
# SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
#
# Tests the helpers of the release workflow (.github/workflows/release.yml)
# that run before and after the build: check_tag.sh (the tag names the
# workspace version), prerelease.sh (a pre-release tag makes a
# pre-release draft), release_notes.py (the draft's notes from the
# metainfo) and flatpak_lint_exceptions.py (the Flatpak job's screenshot
# mirroring and lint exceptions; file:// URLs keep it offline). scripts/check.sh runs it, so a mistake shows on every push,
# not at release time.
set -euo pipefail
HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

failed=0
fail() {
    echo "FAIL: $*" >&2
    failed=1
}

# check_tag.sh: exit status for a tag and a version.
expect_tag() {
    local want="$1" tag="$2" version="$3" got=0
    "$HERE/check_tag.sh" "$tag" "$version" >"$TMP/out" 2>&1 || got=$?
    if [[ "$got" != "$want" ]]; then
        fail "check_tag.sh '$tag' '$version' exited $got, expected $want: $(cat "$TMP/out")"
    fi
}
expect_tag 0 v0.1.0 0.1.0
expect_tag 0 v1.22.333 1.22.333
expect_tag 0 v0.2.0-rc.1 0.2.0-rc.1
expect_tag 1 v0.1.1 0.1.0
expect_tag 1 v0.1.0 0.1.0-rc.1
expect_tag 1 v0.1.0-rc.1 0.1.0
expect_tag 1 0.1.0 0.1.0
expect_tag 1 V0.1.0 0.1.0
expect_tag 1 vv0.1.0 0.1.0
expect_tag 1 v0.1 0.1.0
expect_tag 2 v0.1.0 ""
expect_tag 2 v0.1.0 not-a-version
# The message names the version the tag should have been.
"$HERE/check_tag.sh" v9.9.9 0.1.0 >"$TMP/out" 2>&1 || true
grep -q 'expected v0.1.0' "$TMP/out" || fail "check_tag.sh does not name the expected tag"
# Without a version, the workspace version (from cargo metadata).
VERSION="$(sed -nE 's/^version = "([^"]+)"/\1/p' "$HERE/../../Cargo.toml" | head -n1)"
if ! out="$("$HERE/check_tag.sh" "v$VERSION" 2>&1)"; then
    fail "check_tag.sh v$VERSION (the workspace version) failed: $out"
fi

# prerelease.sh: what it prints for a tag, and its exit status.
expect_prerelease() {
    local want_status="$1" want="$2" tag="$3" got=0 out
    out="$("$HERE/prerelease.sh" "$tag" 2>"$TMP/err")" || got=$?
    if [[ "$got" != "$want_status" ]]; then
        fail "prerelease.sh '$tag' exited $got, expected $want_status: $out $(cat "$TMP/err")"
    elif [[ "$out" != "$want" ]]; then
        fail "prerelease.sh '$tag' printed '$out', expected '$want'"
    fi
}
expect_prerelease 0 false v0.1.0
expect_prerelease 0 false v1.22.333
expect_prerelease 0 false v1.2.3+build.5
expect_prerelease 0 true v0.1.0-rc.1
expect_prerelease 0 true v1.2.3-beta.4
expect_prerelease 0 true v0.2.0-alpha
expect_prerelease 0 true v1.2.3-rc.1+build.5
expect_prerelease 2 "" 0.1.0-rc.1
expect_prerelease 2 "" v0.1
expect_prerelease 2 "" v0.1.0-
expect_prerelease 2 "" v0.1.0-rc..1
expect_prerelease 2 "" ""
if "$HERE/prerelease.sh" >/dev/null 2>&1; then
    fail "prerelease.sh accepted no tag"
fi

# release_notes.py: the Markdown printed for a metainfo.
notes() {
    cat >"$TMP/metainfo.xml"
    "$HERE/release_notes.py" "$TMP/metainfo.xml"
}
expect_notes() {
    local name="$1" want="$2" got
    if ! got="$(notes)"; then
        fail "release_notes.py failed on '$name'"
    elif [[ "$got" != "$want" ]]; then
        fail "release_notes.py on '$name': got
$got
expected
$want"
    fi
}
expect_notes "newest release, paragraphs and lists" "First paragraph, on two lines.

- Item *one*
- Item \`two\`

1. Step
2. Next

Last." <<'XML'
<?xml version="1.0" encoding="UTF-8"?>
<component type="desktop-application">
  <releases>
    <release version="0.2.0" date="2026-11-01">
      <description>
        <p>First paragraph,
           on two lines.</p>
        <ul>
          <li>Item <em>one</em></li>
          <li>Item <code>two</code></li>
        </ul>
        <ol><li>Step</li><li>Next</li></ol>
        <p>Last.</p>
      </description>
    </release>
    <release version="0.1.0" date="2026-10-10">
      <description><p>Older notes.</p></description>
    </release>
  </releases>
</component>
XML
expect_notes "newest release without a description" "" <<'XML'
<component>
  <releases>
    <release version="0.2.0" date="2026-11-01"/>
    <release version="0.1.0" date="2026-10-10">
      <description><p>Older notes.</p></description>
    </release>
  </releases>
</component>
XML
if notes >/dev/null 2>&1 <<'XML'; then
<component><releases/></component>
XML
    fail "release_notes.py accepted a metainfo with no <release>"
fi
if ! "$HERE/release_notes.py" "$HERE/../../packaging/linux/org.xtrudecad.XtrudeCAD.metainfo.xml" >/dev/null; then
    fail "release_notes.py failed on the app's metainfo"
fi

# flatpak_lint_exceptions.py: exit status, mirror decision and exceptions
# for screenshots that resolve or not, outside a release, for a
# pre-release and for a final release.
touch "$TMP/shot.png"
cat >"$TMP/exceptions.json" <<'JSON'
{"org.example.App": {"finish-args-home-filesystem-access": "Why."}}
JSON
shots_metainfo() {
    cat >"$TMP/shots.xml" <<XML
<component>
  <screenshots>
    <screenshot type="default"><image>file://$TMP/shot.png</image></screenshot>
    <screenshot><image>$1</image></screenshot>
  </screenshots>
</component>
XML
}
expect_shots() {
    local name="$1" want_status="$2" want_mirror="$3" want_errors="$4" tag="$5" got=0 errors
    rm -f "$TMP/out.json"
    "$HERE/flatpak_lint_exceptions.py" "$TMP/shots.xml" "$TMP/exceptions.json" \
        "$TMP/out.json" "$tag" >"$TMP/out" 2>"$TMP/err" || got=$?
    if [[ "$got" != "$want_status" ]]; then
        fail "flatpak_lint_exceptions.py on '$name' exited $got, expected $want_status: $(cat "$TMP/out" "$TMP/err")"
        return
    fi
    [[ "$want_status" == 0 ]] || return 0
    grep -qx "mirror=$want_mirror" "$TMP/out" ||
        fail "flatpak_lint_exceptions.py on '$name': expected mirror=$want_mirror, got $(cat "$TMP/out")"
    errors="$(python3 -c 'import json, sys; print(" ".join(sorted(json.load(open(sys.argv[1]))["org.example.App"])))' "$TMP/out.json")"
    [[ "$errors" == "$want_errors" ]] ||
        fail "flatpak_lint_exceptions.py on '$name': exceptions '$errors', expected '$want_errors'"
}
both="appstream-missing-screenshots appstream-screenshots-not-mirrored-in-ostree finish-args-home-filesystem-access"
shots_metainfo "file://$TMP/shot.png"
expect_shots "screenshots resolve" 0 true finish-args-home-filesystem-access ""
expect_shots "screenshots resolve, final release" 0 true finish-args-home-filesystem-access v1.2.3
shots_metainfo "file://$TMP/missing.png"
expect_shots "a screenshot missing" 0 false "$both" ""
expect_shots "a screenshot missing, pre-release" 0 false "$both" v1.2.3-rc.1
expect_shots "a screenshot missing, final release" 1 "" "" v1.2.3
"$HERE/flatpak_lint_exceptions.py" "$TMP/shots.xml" "$TMP/exceptions.json" "$TMP/out.json" >"$TMP/out" 2>&1 || true
grep -q '^::warning ' "$TMP/out" || fail "flatpak_lint_exceptions.py does not warn about a missing screenshot"
if "$HERE/flatpak_lint_exceptions.py" "$TMP/shots.xml" >/dev/null 2>&1; then
    fail "flatpak_lint_exceptions.py accepted missing arguments"
fi

if [[ $failed -ne 0 ]]; then
    echo "release workflow helper tests failed" >&2
    exit 1
fi
echo "release workflow helper tests passed"
