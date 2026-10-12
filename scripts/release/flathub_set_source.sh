#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
# SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
#
# Points the Flathub copy of the manifest at a release: sets the `tag` and
# `commit` of the xtrude-cad module's `type: git` source, in place. Every
# other line stays as it is: the comments, which Flathub's reviewers
# read, and the manifold module's own git sources (Manifold, Clipper2).
# Fails, leaving the file untouched, when the xtrude-cad module has no
# git source or more than one, or when that source lacks a `tag` or a
# `commit` line. flathub-update.yml runs it on every release; it also
# makes the first submission's manifest (RELEASING.md, "Flathub").
#
# Usage: scripts/release/flathub_set_source.sh <manifest> <tag> <commit>
#   commit: the full 40-hex-digit commit the tag points to.
set -euo pipefail

if [[ $# -ne 3 ]]; then
    echo "usage: $0 <manifest> <tag, e.g. v0.1.0> <commit>" >&2
    exit 2
fi
MANIFEST="$1"
TAG="$2"
COMMIT="$3"
if [[ ! -f "$MANIFEST" ]]; then
    echo "error: no manifest $MANIFEST" >&2
    exit 2
fi
if [[ ! "$TAG" =~ ^[A-Za-z0-9._+-]+$ ]]; then
    echo "error: not a tag name: '$TAG'" >&2
    exit 2
fi
if [[ ! "$COMMIT" =~ ^[0-9a-f]{40}$ ]]; then
    echo "error: not a full commit hash: '$COMMIT'" >&2
    exit 2
fi

python3 -I - "$MANIFEST" "$TAG" "$COMMIT" <<'PY'
import os
import re
import sys

path, tag, commit = sys.argv[1:]
with open(path, encoding="utf-8") as f:
    lines = f.read().splitlines(keepends=True)


def indent_of(line):
    return len(line) - len(line.lstrip(" "))


def key_value(text):
    """`key: value` with any trailing comment and quotes dropped."""
    m = re.match(r"([\w-]+):\s*(.*?)\s*(?:#.*)?$", text)
    if not m:
        return None, None
    return m.group(1), m.group(2).strip("'\"")


# The xtrude-cad module's sources, one dict per list entry: its `type`
# and the line numbers of its `tag` and `commit` keys.
module = None
sources_indent = None
item_indent = None
entries = []
for i, line in enumerate(lines):
    stripped = line.strip()
    if not stripped or stripped.startswith("#"):
        continue
    name = re.match(r"  - name: (\S+)\s*$", line)
    if name:
        module = name.group(1)
        sources_indent = None
        continue
    if module != "xtrude-cad":
        continue
    indent = indent_of(line)
    if re.match(r"\s+sources:\s*$", line):
        sources_indent, item_indent = indent, None
        continue
    if sources_indent is None:
        continue
    is_item = stripped.startswith("- ")
    if is_item and indent >= sources_indent and item_indent is None:
        item_indent = indent
    if is_item and indent == item_indent:
        entries.append({"type": None, "tag": [], "commit": []})
        text, key_indent = stripped[2:], indent + 2
    elif item_indent is not None and indent > item_indent:
        if indent != item_indent + 2:
            # A nested value of the entry, not one of its keys.
            continue
        text, key_indent = stripped, indent
    else:
        # Past the end of the sources list.
        sources_indent = None
        continue
    key, value = key_value(text)
    if key == "type":
        entries[-1]["type"] = value
    elif key in ("tag", "commit"):
        entries[-1][key].append((i, key_indent))

git = [e for e in entries if e["type"] == "git"]
if len(git) != 1:
    sys.exit(f"error: {path}: expected one git source in the xtrude-cad "
             f"module, found {len(git)}")
source = git[0]
for key, value in (("tag", tag), ("commit", commit)):
    if len(source[key]) != 1:
        sys.exit(f"error: {path}: the xtrude-cad git source has "
                 f"{len(source[key])} `{key}` lines, expected one")
    i, key_indent = source[key][0]
    lines[i] = f"{' ' * key_indent}{key}: {value}\n"

tmp = f"{path}.tmp"
with open(tmp, "w", encoding="utf-8") as f:
    f.writelines(lines)
os.replace(tmp, path)
PY
