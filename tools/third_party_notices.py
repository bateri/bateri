#!/usr/bin/env python3
"""Generates the notices for the third-party software that enters the package.

Usage (from the repo root, after `make bundle` has downloaded Sparkle):

    python3 tools/third_party_notices.py target/sparkle-<version> \
        > assets/bundle/THIRD-PARTY-LICENSES.txt

Why it exists: MIT and Apache-2.0 require the notice to travel with copies, and
`bateri.app` is distributed. A hand-kept list silently went stale whenever a
dependency was added (the debt in `docs/YOL-HARITASI.md`); the yardstick is
therefore `cargo tree` — `bateri`'s **product** graph (normal + build edges,
aarch64-apple-darwin), dev-dependencies (like the wgpu trial) are excluded.

Choices:
- Crates that have no MIT option and whose license is on the allow list
  (`OWN_LICENSES`) are given with their own texts: the full Apache-2.0 text
  (+ NOTICE if any), for Zlib and ISC the crate's own license file (with the
  copyright line in it).
- Every remaining crate carries MIT as an option (MIT, "MIT OR Apache-2.0",
  "Zlib OR Apache-2.0 OR MIT", "Unlicense OR MIT"); the notice is given under
  MIT — a copyright line per crate, the text once. The copyright line comes
  from the crate's own license file, and from `Cargo.toml`'s authors if it is
  not in the file. A crate whose license neither offers MIT nor is on the
  allow list stops the script: that is a dependency decision, it is not
  silently added to the list (never GPL-2.0-only — bateri is GPL-3.0-or-later).
- Sparkle's LICENSE (MIT + external notices such as bsdiff/sais/ed25519) as is.
"""

import json
import os
import subprocess
import sys

ROOT = "bateri"
TARGET = "aarch64-apple-darwin"
# Licenses without an MIT option that are accepted, each a recorded decision
# (all GPL-3 compatible): Apache-2.0 (alacritty_terminal; codespan-reporting
# via wgpu), Zlib (foldhash via wgpu) and ISC (libloading via wgpu) — user
# decision 2026-09-30, .tasks/040-linux-kapisi-ve-wgpu/phase-5.md. Anything
# else without an MIT option stops the script.
OWN_LICENSES = {"Apache-2.0", "Zlib", "ISC"}
# For crates whose license file carries no copyright line, the project's own
# attribution instead of the author list (alacritty's repo and `Credits.html` use it).
COPYRIGHT = {"alacritty_terminal": ["Copyright 2020 The Alacritty Project"]}
MIT_TEXT = """Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE."""
RULE = "=" * 80


def product_crates():
    """The (name, version) pairs in `bateri`'s product graph, excluding our own crates."""
    out = subprocess.run(
        ["cargo", "tree", "-p", ROOT, "-e", "normal,build", "--target", TARGET,
         "--prefix", "none", "-f", "{p}"],
        check=True, capture_output=True, text=True,
    ).stdout
    crates = set()
    for line in out.splitlines():
        parts = line.split()
        if len(parts) < 2 or parts[0] == ROOT or parts[0].startswith("bt-"):
            continue
        crates.add((parts[0], parts[1].lstrip("v")))
    return crates


def packages():
    meta = json.loads(subprocess.run(
        ["cargo", "metadata", "--format-version", "1", "--filter-platform", TARGET],
        check=True, capture_output=True, text=True,
    ).stdout)
    return {(p["name"], p["version"]): p for p in meta["packages"]}


def copyright_lines(pkg):
    """The copyright lines in the crate's license file; if none, one line from the authors."""
    if pkg["name"] in COPYRIGHT:
        return COPYRIGHT[pkg["name"]]
    root = os.path.dirname(pkg["manifest_path"])
    for name in ("LICENSE-MIT", "LICENSE-MIT.md", "LICENSE-MIT.txt", "LICENSE.MIT",
                 "LICENSE", "LICENSE.md", "LICENSE.txt", "COPYING"):
        path = os.path.join(root, name)
        if not os.path.isfile(path):
            continue
        with open(path, encoding="utf-8", errors="replace") as f:
            lines = [l.strip() for l in f if l.strip().lower().startswith("copyright")]
        # A line without an owner's name ("Copyright (c) 2016--2017") and a
        # redirect file (Unlicense/COPYING) do not count as a notice; the next
        # candidate, and the authors last.
        lines = [l for l in lines
                 if any(ch.isalpha() for ch in l[9:].replace("(c)", "").replace("(C)", ""))]
        if lines:
            return lines
    authors = [a.split(" <")[0] for a in pkg.get("authors") or []]
    return [f"Copyright (c) {', '.join(authors) or 'the ' + pkg['name'] + ' developers'}"]


def offers_mit(pkg):
    expr = pkg.get("license") or ""
    # `/` is the old SPDX-less "or" (`Apache-2.0/MIT`, rustc-hash 1.1).
    return "MIT" in expr.replace("(", " ").replace(")", " ").replace("/", " ").split()


def own_license(pkg):
    """The accepted non-MIT license of `pkg`, or None if it offers MIT."""
    if offers_mit(pkg):
        return None
    expr = (pkg.get("license") or "").strip()
    if expr not in OWN_LICENSES:
        sys.exit(f"{pkg['name']} {pkg['version']}: license '{expr}' neither offers "
                 "MIT nor is on the allow list")
    return expr


def read_first(root, names):
    for name in names:
        path = os.path.join(root, name)
        if os.path.isfile(path):
            with open(path, encoding="utf-8", errors="replace") as f:
                return f.read().rstrip()
    return None


def main():
    if len(sys.argv) != 2:
        sys.exit("usage: third_party_notices.py <sparkle directory>")
    sparkle_license = os.path.join(sys.argv[1], "LICENSE")
    pkgs = packages()
    crates = sorted(product_crates())

    out = ["bateri includes, among others, the following third-party software.", ""]

    for name, version in crates:
        pkg = pkgs[(name, version)]
        license = own_license(pkg)
        if license is None:
            continue
        root = os.path.dirname(pkg["manifest_path"])
        if license == "Apache-2.0":
            text = read_first(root, ("LICENSE-APACHE", "LICENSE-APACHE.md", "LICENSE-APACHE.txt",
                                     "LICENSE", "LICENSE.md", "LICENSE.txt"))
            if text is None or "Apache License" not in text:
                sys.exit(f"{name} {version}: Apache-2.0 text not found")
            notice = read_first(root, ("NOTICE", "NOTICE.md", "NOTICE.txt"))
            out += [RULE, name, pkg.get("repository") or "", *copyright_lines(pkg),
                    "Licensed under the Apache License, Version 2.0; full text below.",
                    RULE, "", *([notice, ""] if notice else []), text, "", ""]
        else:
            # Zlib and ISC: the crate's own license file, which carries its
            # copyright line; the license requires the notice verbatim.
            text = read_first(root, ("LICENSE", "LICENSE.md", "LICENSE.txt", "COPYING",
                                     f"LICENSE-{license.upper()}"))
            if text is None:
                sys.exit(f"{name} {version}: {license} license file not found")
            out += [RULE, name, pkg.get("repository") or "",
                    f"Licensed under the {license} License; full text below.",
                    RULE, "", text, "", ""]

    out += [RULE,
            "The following Rust crates are used under the MIT License (each offers",
            "MIT, alone or as one of its license options). The license text follows",
            "the list.",
            RULE, ""]
    for name, version in crates:
        pkg = pkgs[(name, version)]
        if own_license(pkg) is not None:
            continue
        out += [f"{name} {version}", *(f"  {l}" for l in copyright_lines(pkg))]
        if pkg.get("repository"):
            out.append(f"  {pkg['repository']}")
        out.append("")
    out += ["MIT License", "", MIT_TEXT, "", ""]

    with open(sparkle_license, encoding="utf-8") as f:
        sparkle = f.read().rstrip()
    out += [RULE, "Sparkle", "https://sparkle-project.org",
            "Licensed under the MIT License, with the external notices it carries;",
            "full text below.", RULE, "", sparkle, ""]

    sys.stdout.write("\n".join(out))


if __name__ == "__main__":
    main()
