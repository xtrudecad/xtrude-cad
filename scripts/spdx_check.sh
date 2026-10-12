#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
# SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
# Every Rust/WGSL/C++ source file must start with the two-line SPDX header,
# and every Lua file with the same lines as `--` comments; crates/openskp
# (derived from MIT-licensed OpenSKP) keeps the MIT header.
set -euo pipefail
cd "$(dirname "$0")/.."
# REUSE-IgnoreStart
expected='// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)'
expected_lua='-- SPDX-License-Identifier: GPL-3.0-or-later
-- SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)'
expected_mit='// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 2024-2026 Ahsan Mehmood
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)'
# REUSE-IgnoreEnd
bad=0
while IFS= read -r f; do
    case "$f" in
        crates/openskp/*) ok=$([ "$(head -n3 "$f")" = "$expected_mit" ] && echo y) ;;
        *.lua) ok=$([ "$(head -n2 "$f")" = "$expected_lua" ] && echo y) ;;
        *) ok=$([ "$(head -n2 "$f")" = "$expected" ] && echo y) ;;
    esac
    if [ -z "$ok" ]; then
        echo "missing SPDX header: $f" >&2
        bad=1
    fi
done < <(find crates -name '*.rs' -o -name '*.wgsl' -o -name '*.cpp' -o -name '*.hpp'; find crates resources -name '*.lua')
exit $bad
