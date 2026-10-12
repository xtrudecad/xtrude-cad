#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
# SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
# CI-ready checks: formatting, lints, tests, licences, SPDX headers, REUSE.
set -euo pipefail
cd "$(dirname "$0")/.."
# Cargo, and the compilers and linkers it starts, run at low CPU priority:
# the test build links several hundred binaries and would otherwise starve
# the desktop. `nice` goes on cargo itself, not on this script: some
# desktops (ananicy) reset a shell's priority by its process name.
low() { nice -n 19 "$@"; }
low cargo fmt --all -- --check
low cargo clippy --workspace --all-targets -- -D warnings
low cargo test --workspace -q
if command -v cargo-deny >/dev/null; then
    low cargo deny check licenses bans sources
else
    echo "cargo-deny not installed: cargo install cargo-deny" >&2
    exit 1
fi
scripts/spdx_check.sh
# The release workflow's helpers, the glibc floor check and the tarball's
# install.sh, which otherwise run only when a tag is built.
scripts/release/test_release_workflow.sh
scripts/release/test_check_glibc.sh
scripts/release/test_install.sh
# REUSE compliance (fsfe/reuse): every file has licence and copyright info.
if command -v reuse >/dev/null; then
    reuse lint
elif command -v uvx >/dev/null; then
    uvx reuse lint
else
    echo "reuse not installed: pipx install reuse" >&2
    exit 1
fi
