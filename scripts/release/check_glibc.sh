#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
# SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
#
# Checks that a binary runs on glibc <max>: fails when it needs any
# GLIBC_x.y symbol version newer than <max>, and prints those symbols.
#
# The portable tarball is built against an old glibc (2.28, AlmaLinux 8)
# so that it runs on every current distribution; a dependency bump that
# pulls in a newer symbol must fail the release build, not users' machines.
#
# Usage: scripts/release/check_glibc.sh <binary> <max>
#   e.g. scripts/release/check_glibc.sh dist/bin/xtrude 2.28
set -euo pipefail

if [[ $# -ne 2 ]]; then
    echo "usage: $0 <binary> <max glibc version, e.g. 2.28>" >&2
    exit 2
fi
BIN="$1"
MAX="$2"
if [[ ! -f "$BIN" ]]; then
    echo "error: no file at $BIN" >&2
    exit 2
fi
if [[ ! "$MAX" =~ ^[0-9]+(\.[0-9]+)*$ ]]; then
    echo "error: '$MAX' is not a version such as 2.28" >&2
    exit 2
fi
if ! command -v objdump >/dev/null; then
    echo "error: objdump not found (binutils)" >&2
    exit 2
fi

# The dynamic symbols, one "<version> <symbol>" line per GLIBC_ reference.
# objdump prints the version either bare or in parentheses (hidden
# versions), followed by the symbol name in the last column.
if ! dynsyms="$(objdump -T "$BIN" 2>&1)"; then
    echo "error: objdump -T failed on $BIN:" >&2
    echo "$dynsyms" >&2
    exit 2
fi
needed="$(sed -nE 's/.*[ (]GLIBC_([0-9][0-9.]*)\)? +([^ ]+)$/\1 \2/p' <<<"$dynsyms")"

# Versions are compared numerically, part by part (2.3 < 2.28 < 2.28.1),
# never as strings.
offending="$(awk -v max="$MAX" '
    function newer(a, b,    pa, pb, na, nb, i, x, y) {
        na = split(a, pa, ".")
        nb = split(b, pb, ".")
        for (i = 1; i <= (na > nb ? na : nb); i++) {
            x = (i <= na) ? pa[i] + 0 : 0
            y = (i <= nb) ? pb[i] + 0 : 0
            if (x != y) return x > y
        }
        return 0
    }
    newer($1, max) { print "  GLIBC_" $1 "  " $2 }
' <<<"$needed" | sort -V -u)"

newest="$(cut -d' ' -f1 <<<"$needed" | sort -V | tail -n1)"
if [[ -n "$offending" ]]; then
    echo "$BIN needs glibc newer than $MAX:" >&2
    echo "$offending" >&2
    exit 1
fi
echo "$BIN: glibc ok (newest symbol version ${newest:-none}, floor $MAX)"
