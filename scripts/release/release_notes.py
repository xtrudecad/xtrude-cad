#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-or-later
# SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
#
# Prints the release notes of a draft release, in Markdown: the
# <description> of the newest <release> in the AppStream metainfo (the
# first one; AppStream lists releases newest first). Prints nothing when
# that release has no description; the release workflow then lets GitHub
# generate the notes.
#
# A description holds what AppStream allows: paragraphs (<p>), lists
# (<ul>, <ol> of <li>), and inline <em> and <code>.
#
# Usage: scripts/release/release_notes.py <metainfo.xml>
import re
import sys
import xml.etree.ElementTree as ET


def inline(el):
    """The text of an element, its <em> and <code> children marked up and
    its whitespace collapsed."""
    parts = [el.text or ""]
    for child in el:
        text = inline(child)
        if child.tag == "em":
            text = f"*{text}*"
        elif child.tag == "code":
            text = f"`{text}`"
        parts.append(text)
        parts.append(child.tail or "")
    return re.sub(r"\s+", " ", "".join(parts)).strip()


def markdown(description):
    blocks = []
    for el in description:
        if el.tag == "p":
            blocks.append(inline(el))
        elif el.tag in ("ul", "ol"):
            items = [li for li in el if li.tag == "li"]
            if el.tag == "ul":
                lines = [f"- {inline(li)}" for li in items]
            else:
                lines = [f"{i}. {inline(li)}" for i, li in enumerate(items, 1)]
            blocks.append("\n".join(lines))
    return "\n\n".join(b for b in blocks if b)


def main():
    if len(sys.argv) != 2:
        print(f"usage: {sys.argv[0]} <metainfo.xml>", file=sys.stderr)
        return 2
    try:
        root = ET.parse(sys.argv[1]).getroot()
    except (OSError, ET.ParseError) as e:
        print(f"error: cannot read {sys.argv[1]}: {e}", file=sys.stderr)
        return 1
    release = root.find("releases/release")
    if release is None:
        print(f"error: {sys.argv[1]} has no <release>", file=sys.stderr)
        return 1
    description = release.find("description")
    notes = markdown(description) if description is not None else ""
    if notes:
        print(notes)
    return 0


if __name__ == "__main__":
    sys.exit(main())
