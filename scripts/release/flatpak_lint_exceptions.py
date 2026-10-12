#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-or-later
# SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
#
# Decides how the Flatpak job (.github/workflows/flatpak.yml) treats the
# metainfo's screenshots, and writes the lint exceptions it uses.
#
# The screenshot URLs link the public repository at the release tag,
# which exists only once that release is tagged there. When every URL
# resolves, the build mirrors the screenshots, as Flathub's builder does,
# and the lint checks them: the exceptions are lint-exceptions.json as
# is. When one does not (a run before the tag, a pre-release, a test tag
# in another repository), the exceptions also accept the two errors that
# follow from it, and the job warns; for a final release (a tag without a
# pre-release part, `v1.2.3`) it fails instead.
#
# Prints `mirror=true` or `mirror=false` (a GitHub Actions step output).
# Exits 1 when a final release's screenshots do not resolve.
#
# Usage: scripts/release/flatpak_lint_exceptions.py <metainfo.xml>
#            <lint-exceptions.json> <output.json> [release tag]
import json
import sys
import urllib.request
import xml.etree.ElementTree as ET

SCREENSHOT_ERRORS = {
    "appstream-missing-screenshots": "The screenshots' release tag is not public yet.",
    "appstream-screenshots-not-mirrored-in-ostree": "Not mirrored while their URLs do not resolve.",
}


def unreachable(urls):
    """The URLs that do not resolve, each with the reason."""
    missing = []
    for url in urls:
        try:
            request = urllib.request.Request(url, method="HEAD")
            with urllib.request.urlopen(request, timeout=30):
                pass
        except (OSError, ValueError) as error:
            missing.append(f"{url} ({error})")
    return missing


def main(argv):
    if len(argv) not in (4, 5):
        print(
            f"usage: {argv[0]} <metainfo.xml> <lint-exceptions.json> <output.json> [release tag]",
            file=sys.stderr,
        )
        return 2
    metainfo, exceptions_in, exceptions_out = argv[1:4]
    tag = argv[4] if len(argv) == 5 else ""

    urls = [el.text.strip() for el in ET.parse(metainfo).iterfind(".//screenshot/image") if el.text]
    with open(exceptions_in, encoding="utf-8") as f:
        exceptions = json.load(f)

    missing = unreachable(urls)
    if missing:
        for line in missing:
            print(f"not reachable: {line}", file=sys.stderr)
        if tag and "-" not in tag:
            print(f"::error file={metainfo}::the screenshot URLs of release {tag} do not resolve")
            return 1
        print(
            f"::warning file={metainfo}::the screenshot URLs do not resolve (yet): "
            "not mirrored, and their lint errors are accepted"
        )
        for reasons in exceptions.values():
            reasons.update(SCREENSHOT_ERRORS)

    with open(exceptions_out, "w", encoding="utf-8") as f:
        json.dump(exceptions, f, indent=4)
        f.write("\n")
    print(f"mirror={'false' if missing else 'true'}")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
