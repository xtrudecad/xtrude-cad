#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
# SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
#
# Tests check_glibc.sh, the release build's glibc floor check, on saved
# `objdump -T` outputs (scripts/release/fixtures/objdump_*.txt), so that
# it runs on every push without a release binary. scripts/check.sh runs it.
set -euo pipefail
HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
FIXTURES="$HERE/fixtures"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

failed=0
fail() {
    echo "FAIL: $*" >&2
    failed=1
}

# Runs the check on a fixture against a floor; the output goes to $TMP/out.
expect_glibc() {
    local want="$1" fixture="$2" max="$3" got=0
    "$HERE/check_glibc.sh" --objdump-output "$FIXTURES/$fixture" "$max" >"$TMP/out" 2>&1 || got=$?
    if [[ "$got" != "$want" ]]; then
        fail "check_glibc.sh on $fixture, floor $max, exited $got, expected $want: $(cat "$TMP/out")"
        return 1
    fi
}
expect_output() {
    local fixture="$1" pattern="$2"
    grep -qE -- "$pattern" "$TMP/out" ||
        fail "check_glibc.sh on $fixture: no '$pattern' in its output: $(cat "$TMP/out")"
}
expect_no_output() {
    local fixture="$1" pattern="$2"
    if grep -qE -- "$pattern" "$TMP/out"; then
        fail "check_glibc.sh on $fixture: unexpected '$pattern' in its output: $(cat "$TMP/out")"
    fi
}

# Only symbols up to GLIBC_2.28 (one printed bare, not in parentheses;
# GLIBC_PRIVATE and GCC_ versions are not glibc releases): passes.
if expect_glibc 0 objdump_glibc_2.28.txt 2.28; then
    expect_output objdump_glibc_2.28.txt 'newest symbol version 2\.28,'
fi
# One GLIBC_2.34 symbol: fails, and names that symbol and nothing else.
if expect_glibc 1 objdump_glibc_2.34.txt 2.28; then
    expect_output objdump_glibc_2.34.txt '^  GLIBC_2\.34  __libc_start_main$'
    expect_no_output objdump_glibc_2.34.txt 'fcntl64|clock_gettime|free'
fi
# The same symbols under a newer floor: pass.
expect_glibc 0 objdump_glibc_2.34.txt 2.34 || true
# GLIBC_2.3 next to GLIBC_2.28: versions compare numerically (2.3 < 2.28),
# not as strings, so this passes and 2.28 is the newest.
if expect_glibc 0 objdump_glibc_2.3_and_2.28.txt 2.28; then
    expect_output objdump_glibc_2.3_and_2.28.txt 'newest symbol version 2\.28,'
fi
# ... and under a 2.27 floor only the 2.28 symbol is too new.
if expect_glibc 1 objdump_glibc_2.3_and_2.28.txt 2.27; then
    expect_output objdump_glibc_2.3_and_2.28.txt '^  GLIBC_2\.28  fcntl64$'
    expect_no_output objdump_glibc_2.3_and_2.28.txt '__ctype_toupper_loc|__snprintf_chk'
fi

# Usage errors exit 2.
expect_glibc 2 missing.txt 2.28 || true
expect_glibc 2 objdump_glibc_2.28.txt not-a-version || true
if "$HERE/check_glibc.sh" --objdump-output >/dev/null 2>&1; then
    fail "check_glibc.sh accepted --objdump-output with no file"
fi

if [[ $failed -ne 0 ]]; then
    echo "glibc check tests failed" >&2
    exit 1
fi
echo "glibc check tests passed"
