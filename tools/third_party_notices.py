#!/usr/bin/env python3
"""Generates the notices for the third-party software that enters the package.

Usage (from the repo root, after `make bundle` has downloaded Sparkle):

    python3 tools/third_party_notices.py target/sparkle-<version> \
        > assets/bundle/THIRD-PARTY-LICENSES.txt

Why it exists: MIT and Apache-2.0 require the notice to travel with copies, and
`bateri.app` is distributed. A hand-kept list silently went stale whenever a
dependency was added; the yardstick is
therefore `cargo tree` — `bateri`'s **product** graph (normal + build edges,
aarch64-apple-darwin), dev-dependencies (like the wgpu trial) are excluded.

Choices:
- Crates that have no MIT option and whose license is on the allow list
  (`OWN_LICENSES`) are given with their own texts: the full Apache-2.0 text
  (+ NOTICE if any), for Zlib and ISC the crate's own license file (with the
  copyright line in it).
- The crates under "Apache-2.0 WITH LLVM-exception" (the WebAssembly runtime and its
  code generator: a few dozen, one license file among them) are listed once, each with
  its copyright line, under the text of that license given once — the Apache
  License 2.0 with the LLVM exceptions that follow it in the crates' own license file.
- Every remaining crate carries MIT as an option (MIT, "MIT OR Apache-2.0",
  "Zlib OR Apache-2.0 OR MIT", "Unlicense OR MIT"); the notice is given under
  MIT — a copyright line per crate, the text once. The copyright line comes
  from the crate's own license file, and from `Cargo.toml`'s authors if it is
  not in the file. A crate whose license neither offers MIT nor is on the
  allow list stops the script: that is a dependency decision, it is not
  silently added to the list (never GPL-2.0-only — bateri is GPL-3.0-or-later).
- A crate that carries bateri's own license and comes from our own organisation's
  repository (`kablo-proto`, from kablo — the relay bateri speaks to — under
  GPL-3.0-or-later like bateri) is covered by bateri's `LICENSE`, which ships in
  the package: it gets no entry here. The check is both halves, so a GPL crate
  from anywhere else still stops the script.
- Sparkle's LICENSE (MIT + external notices such as bsdiff/sais/ed25519) as is.
- The scripting language's interpreter (`luau.wasm`, built in pencere from its `kit/luau`
  and carried by the `pencere-sandbox` crate) is not a crate of its own: it is Luau, and
  statically the C and C++ runtimes of the toolchain that built it (wasi-libc with its musl,
  libc++, libc++abi, libunwind and compiler-rt of LLVM). Their texts are kept beside the
  sources it is built from and travel in the crate (`pencere-sandbox`'s `licenses/`), and
  are given as they are.
- The fonts that ship with the app (`assets/fonts/{Family}/`, one directory per family):
  the copyright line and the license of each, read from the license file that lies beside
  the font, and the SIL Open Font License text once. The Symbols Nerd Font combines icon
  sets that keep their own licenses; they are listed as its README gives them, and the
  attribution the Creative Commons ones ask for is written out.
- What the PDF reader (`hayro`, which `pencere-sandbox` brings) carries besides code: the
  fallback fonts of PDFium (Foxit's, BSD-3-Clause) and the predefined CMaps of Adobe
  (BSD-3-Clause), each with the license file that lies beside the data in its crate, given
  as it is. A binary that ships the data asks for the notice, as for any BSD license.
- A crate under both BSD-3-Clause and MIT (`BSD-3-Clause AND MIT`, brotli) gives the text of
  each: that `AND` is not the option the MIT list below is made of. Other `AND` expressions
  (`encoding_rs`, `unicode-ident`) are still listed under MIT alone, as they were before the
  PDF reader. A crate under BSD-3-Clause or Apache-2.0 is given under BSD-3-Clause, the text of
  the crate's own `LICENSE.md`.
- The color schemes bateri adapts (`tools/import_themes.py`, the one list a
  theme is added to): the collection the values are read from and each
  project the schemes come from, with its copyright line and license — MIT
  under the MIT text given once, Apache-2.0 under the Apache text the file
  already carries (alacritty_terminal's).
"""

import json
import os
import re
import subprocess
import sys
import textwrap

import import_themes

ROOT = "bateri"
TARGET = "aarch64-apple-darwin"
# Licenses without an MIT option that are accepted, each a recorded decision
# (all GPL-3 compatible): Apache-2.0 (alacritty_terminal; codespan-reporting
# via wgpu), Zlib (foldhash via wgpu) and ISC (libloading via wgpu) — user
# decision 2026-09-30 — and BSD-2-Clause and BSD-3-Clause (the pictures' decoders pencere's
# sandbox brings: tiny-skia under the SVG renderer, arrayref), which the Free Software
# Foundation lists as GPL-compatible and which ask only for the notice, given as the crate
# has it. Anything else without an MIT option stops the script.
OWN_LICENSES = {
    "Apache-2.0", "Zlib", "ISC", "BSD-2-Clause", "BSD-3-Clause",
    "Apache-2.0 WITH LLVM-exception",
    # The PDF reader's stack (hayro and what it brings, 2026-11-06, with the PDF tab): the
    # color library and the scaler offer BSD-3-Clause beside Apache-2.0 and are given under
    # BSD-3-Clause; the brotli crate is under both BSD-3-Clause and MIT (each text is given);
    # the ICU4X crates `yoke` and `zerofrom` are under the Unicode license v3 (permissive,
    # GPL-3 compatible), whose text and copyright line are the crate's own.
    "BSD-3-Clause OR Apache-2.0", "BSD-3-Clause AND MIT", "Unicode-3.0",
}
# Crates of a repository that publishes one license file for several of them, and puts it
# in one of them only: the crate whose package carries none is given the sibling's text,
# of the same repository (the `repository` of both says so, and the check below).
SIBLING_LICENSE = {"alloc-stdlib": "alloc-no-stdlib"}
# The license of the WebAssembly runtime's crates (user decision 2026-10-23, with the runtime):
# Apache-2.0 plus exceptions that only relax it (the object code of a compiled work need not
# carry the notices), so GPL-3 compatible. Its crates share one text.
LLVM_LICENSE = "Apache-2.0 WITH LLVM-exception"
# For crates whose license file carries no copyright line, the project's own
# attribution instead of the author list (alacritty's repo and `Credits.html` use it).
# bateri's own license (`Cargo.toml`, `workspace.package.license`) and the place its
# sister crates are taken from.
OWN_PROJECT_LICENSE = "GPL-3.0-or-later"
OWN_PROJECT_SOURCE = "git+https://github.com/bateri/"
# pencere's LICENSE is the Apache text as it is published and names no holder (the
# lines in it that begin with "copyright" are the license's own sentences); its
# NOTICE does, and this is that line. Six crates of its repository are in the build: the
# host's rules, the wire they share with the program, the page engine, the sandbox, the
# drive that tests a pane and the editor the page engine takes.
COPYRIGHT = {
    "alacritty_terminal": ["Copyright 2020 The Alacritty Project"],
    **{name: ["Copyright 2026 The pencere Authors"]
       for name in ("pencere-host", "pencere-wire", "pencere-view", "pencere-sandbox",
                    "pencere-drive", "pencere-editor")},
}
# Crates whose license file is the Apache text alone, with no copyright line of the holder's (the
# sentences of the license that begin with "copyright" are not one): the line comes from the
# authors in the crate's `Cargo.toml`.
COPYRIGHT_FROM_AUTHORS = {"ab_glyph_rasterizer"}
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
    for name in () if pkg["name"] in COPYRIGHT_FROM_AUTHORS else (
            "LICENSE-MIT", "LICENSE-MIT.md", "LICENSE-MIT.txt", "LICENSE.MIT",
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


def is_ours(pkg):
    """Whether `pkg` is under bateri's own license, from bateri's own organisation."""
    return (pkg.get("license") == OWN_PROJECT_LICENSE
            and (pkg.get("source") or "").startswith(OWN_PROJECT_SOURCE))


def offers_mit(pkg):
    expr = pkg.get("license") or ""
    if expr == "BSD-3-Clause AND MIT":
        # Both apply (brotli): MIT is no option to choose, its text is one of those to give.
        return False
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


def sibling_license(pkgs, pkg, sibling):
    """The license file of `sibling`, a crate of the same repository as `pkg` (which carries
    none in its package); stops if the repositories are not the same."""
    found = [p for (name, _), p in pkgs.items() if name == sibling]
    if len(found) != 1 or found[0].get("repository") != pkg.get("repository"):
        sys.exit(f"{pkg['name']}: no license file, and {sibling} is not one crate of the "
                 "same repository to take it from")
    return read_first(os.path.dirname(found[0]["manifest_path"]),
                      ("LICENSE", "LICENSE.md", "LICENSE.txt"))


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
    crates = sorted(c for c in product_crates() if not is_ours(pkgs[c]))

    out = ["bateri includes, among others, the following third-party software.", ""]

    with_exceptions = []
    for name, version in crates:
        pkg = pkgs[(name, version)]
        license = own_license(pkg)
        if license is None:
            continue
        root = os.path.dirname(pkg["manifest_path"])
        if license == LLVM_LICENSE:
            with_exceptions.append((name, version))
            continue
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
            # Zlib, ISC and BSD: the crate's own license file, which carries its
            # copyright line; the license requires the notice verbatim.
            if " AND " in license:
                # Under both: the file of each (`LICENSE.BSD-3-Clause`, `LICENSE.MIT`).
                parts = license.split(" AND ")
                texts = [read_first(root, (f"LICENSE.{part}", f"LICENSE-{part}")) for part in parts]
                if None in texts:
                    sys.exit(f"{name} {version}: a license file of {license} not found")
                text = "\n\n".join(texts)
                words = f"the {' and the '.join(parts)} Licenses"
            else:
                if license == "BSD-3-Clause OR Apache-2.0":
                    words, license = "the BSD-3-Clause License (it offers Apache-2.0 too)", "BSD-3-Clause"
                else:
                    words = f"the {license} License"
                text = read_first(root, ("LICENSE", "LICENSE.md", "LICENSE.txt", "COPYING",
                                         f"LICENSE-{license.upper()}"))
                if text is None and name in SIBLING_LICENSE:
                    text = sibling_license(pkgs, pkg, SIBLING_LICENSE[name])
                if text is not None and license == "BSD-3-Clause" and "Redistribution" not in text:
                    sys.exit(f"{name} {version}: the license file is not the BSD-3-Clause text")
            if text is None:
                sys.exit(f"{name} {version}: {license} license file not found")
            out += [RULE, name, pkg.get("repository") or "",
                    f"Licensed under {words}; full text below.",
                    RULE, "", text, "", ""]

    if with_exceptions:
        out += llvm_exception_notices(pkgs, with_exceptions)

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

    out += module_notices(pkgs)
    out += pdf_data_notices(pkgs)

    out += theme_notices("Version 2.0" in "\n".join(out))
    out += font_notices()
    sys.stdout.write("\n".join(out))


def licenses_of(pkgs):
    """The folder of license texts the sandbox crate carries beside the interpreter it ships:
    Luau's and the toolchain's, given as they are. The crate is the one in the build, so the
    texts are the ones of the very interpreter that is."""
    found = [p for (name, _), p in pkgs.items() if name == "pencere-sandbox"]
    if len(found) != 1:
        sys.exit(f"pencere-sandbox: expected one version in the build, found {len(found)}")
    folder = os.path.join(os.path.dirname(found[0]["manifest_path"]), "licenses")
    if not os.path.isdir(folder):
        sys.exit(f"pencere-sandbox carries no licenses/ folder at {folder}")
    return folder


def module_notices(pkgs):
    """The notices of what the interpreter module is made of besides our own code: Luau
    (MIT, with the Lua notice it keeps), and the runtimes of the toolchain that are linked
    into it, each with the text it comes with."""
    folder = licenses_of(pkgs)

    def licensed(name):
        with open(os.path.join(folder, name), encoding="utf-8") as f:
            return f.read().rstrip()

    return [
        RULE, "Luau", "https://luau.org", "https://github.com/luau-lang/luau",
        "Licensed under the MIT License, with the notice of Lua it derives from; full text",
        "below. It is the interpreter that runs a view written as a script, built into a",
        "WebAssembly module (`luau.wasm`) together with the libraries named after it.",
        RULE, "", licensed("luau.txt"), "", licensed("lua.txt"), "",
        RULE, "wasi-libc (with musl, dlmalloc and cloudlibc)",
        "https://github.com/WebAssembly/wasi-libc",
        "The C library linked into the interpreter module: licensed as the file says; full",
        "text below.",
        RULE, "", licensed("wasi-libc.txt"), "",
        RULE, "libc++, libc++abi, libunwind and compiler-rt (the LLVM Project)",
        "https://llvm.org",
        "The C++ library, its ABI and unwinding libraries and the compiler's runtime, linked",
        "into the interpreter module: the Apache License 2.0 with LLVM exceptions (the text",
        "of the Apache License is given above, with the exceptions) and the licenses of the",
        "LLVM Project below.",
        RULE, "", licensed("llvm.txt"), "",
    ]


def pdf_data_notices(pkgs):
    """The data the PDF reader carries in the binary: the fonts it falls back on for a PDF
    that does not embed the fourteen standard ones (Foxit's, from PDFium) and the CMaps it
    reads Chinese, Japanese and Korean text by (Adobe's). Each text is the file beside the
    data in the crate that holds it, as it is."""
    def text_of(crate, relative):
        found = [p for (name, _), p in pkgs.items() if name == crate]
        if len(found) != 1:
            sys.exit(f"{crate}: expected one version in the build, found {len(found)}")
        path = os.path.join(os.path.dirname(found[0]["manifest_path"]), relative)
        if not os.path.isfile(path):
            sys.exit(f"{crate} carries no {relative}")
        with open(path, encoding="utf-8") as f:
            return f.read().rstrip()

    # The notice the fonts were extracted with: PDFium's line is in the license file, Foxit's is
    # the crate's note beside the fonts.
    foxit_lines = [line.strip() for line in text_of("hayro-font", "assets/README.md").splitlines()
                   if "Foxit Software" in line]
    if not foxit_lines:
        sys.exit("hayro-font: no Foxit copyright line in assets/README.md")

    return [
        RULE, "Foxit fonts (from PDFium), in hayro-font",
        "https://github.com/LaurenzV/hayro",
        "The fonts a PDF is drawn in where it does not carry the standard ones (Helvetica,",
        "Times, Courier, Symbol, ZapfDingbats): BSD-3-Clause; full text below, as the crate",
        "has it.",
        RULE, "", *foxit_lines, "", text_of("hayro-font", "assets/LICENSE_FOXIT"), "",
        RULE, "Predefined CMaps of Adobe, in hayro-cmap",
        "https://github.com/LaurenzV/hayro",
        "The tables that read the text of a PDF set in a CJK or other composite font: BSD-3-Clause;",
        "full text below, as the crate has it.",
        RULE, "", text_of("hayro-cmap", "assets/LICENSE.txt"), "",
    ]


def llvm_exception_notices(pkgs, crates):
    """The crates under the Apache License 2.0 with the LLVM exceptions, listed once with their
    copyright lines, and the license text given once: the first crate's license file that
    carries the exceptions (they follow the Apache text in the file), and each NOTICE."""
    out = [RULE,
           "The following Rust crates are used under the Apache License, Version 2.0, with",
           "the LLVM exceptions. The license text follows the list.",
           RULE, ""]
    text = None
    notices = []
    for name, version in crates:
        pkg = pkgs[(name, version)]
        root = os.path.dirname(pkg["manifest_path"])
        out += [f"{name} {version}", *(f"  {l}" for l in copyright_lines(pkg))]
        if pkg.get("repository"):
            out.append(f"  {pkg['repository']}")
        out.append("")
        candidate = read_first(root, ("LICENSE", "LICENSE.md", "LICENSE.txt"))
        if text is None and candidate and "LLVM Exceptions" in candidate \
                and "Apache License" in candidate:
            text = candidate
        notice = read_first(root, ("NOTICE", "NOTICE.md", "NOTICE.txt"))
        if notice and notice not in notices:
            notices.append(notice)
    if text is None:
        sys.exit("no license file of the crates under the LLVM exception carries the exceptions")
    for notice in notices:
        out += [notice, ""]
    out += [text, "", ""]
    return out


def theme_notices(apache_given):
    """The notices of the adapted color schemes, by project."""
    used = {project for _, _, _, project in import_themes.THEMES}
    projects = [(key, import_themes.PROJECTS[key]) for key in sorted(used)]
    licenses = {project["license"] for _, project in projects}
    if not licenses <= {"MIT", "Apache-2.0"}:
        sys.exit(f"color schemes: a license with no notice path here: {licenses}")
    if "Apache-2.0" in licenses and not apache_given:
        sys.exit("color schemes: an Apache-2.0 scheme needs the Apache text in the file")
    out = [RULE,
           "Color schemes. bateri's adapted themes carry the colors of the",
           "following projects. The values are read from iTerm2-Color-Schemes,",
           "Copyright (c) 2011 to Present Mark Badolato, MIT License,",
           import_themes.SOURCE,
           "Each project is used under the license named with it; the MIT text",
           "follows the list, the Apache License 2.0 text is given above.",
           RULE, ""]
    for key, project in projects:
        themes = ", ".join(title for _, title, _, of in import_themes.THEMES if of == key)
        out += [project["title"], f"  {project['copyright']}", f"  {project['url']}",
                f"  {project['license']}",
                *textwrap.wrap(f"bateri's themes: {themes}", 78,
                               initial_indent="  ", subsequent_indent="    "),
                ""]
    out += ["MIT License", "", MIT_TEXT, ""]
    return out


FONTS_DIR = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "assets", "fonts")
OFL_MARK = "SIL OPEN FONT LICENSE Version 1.1"


def font_notices():
    """The notices of the fonts that ship with the app, from `assets/fonts/*/`: for each
    family the title (the first sentence of its README), the copyright line its license file
    opens with and the license; the SIL Open Font License text once, from the first family
    that carries it. A family whose license is neither the OFL nor MIT stops the script."""
    families = sorted(
        d for d in os.listdir(FONTS_DIR) if os.path.isdir(os.path.join(FONTS_DIR, d)))
    if not families:
        sys.exit("fonts: assets/fonts holds no family")
    out = [RULE,
           "Fonts. bateri ships the following fonts, unmodified, in Contents/Resources/fonts;",
           "each family's license text lies beside its font files. The SIL Open Font License",
           "text follows the list.",
           RULE, ""]
    ofl_text = None
    symbols = []
    for family in families:
        root = os.path.join(FONTS_DIR, family)
        readme = read_first(root, ("README",))
        if readme is None:
            sys.exit(f"fonts: {family} has no README")
        title = " ".join(readme.split("\n\n")[0].split()).split(". ")[0].rstrip(".")
        text = read_first(root, ("OFL.txt", "LICENSE"))
        if text is None:
            sys.exit(f"fonts: {family} has no license file")
        lines = text.replace("\u00a9", "(c)").splitlines()
        if OFL_MARK in text:
            kind = "SIL Open Font License, Version 1.1"
            stop = next(i for i, l in enumerate(lines) if "This Font Software is licensed" in l)
            holder = [l for l in lines[:stop] if l.strip()]
            if ofl_text is None:
                ofl_text = text[text.index(OFL_MARK):]
        elif text.lstrip().startswith("The MIT License"):
            kind = "MIT License"
            holder = [l for l in lines if l.startswith("Copyright")]
            symbols.append((family, readme, text))
        else:
            sys.exit(f"fonts: {family}: a license that is neither the OFL nor MIT")
        out += [title, *(f"  {l}" for l in holder), f"  {kind}",
                f"  Contents/Resources/fonts/{family}/", ""]
    if ofl_text is None:
        sys.exit("fonts: no OFL text found")
    out += ["SIL Open Font License, Version 1.1", "", ofl_text.rstrip(), ""]
    for family, readme, text in symbols:
        sets = [l.rstrip() for l in readme.splitlines()
                if re.match(r"^  \S.*\s{2,}\S", l)]
        out += [RULE, f"{family}: the Nerd Fonts license and the icon sets in the font", RULE, "",
                text.rstrip(), "", "The font combines icon sets that keep their own licenses:",
                "", *sets, "",
                "The MIT and Apache-2.0 sets are covered by the license texts given above.",
                "The sets under the Creative Commons Attribution 4.0 International license",
                "(https://creativecommons.org/licenses/by/4.0/) are attributed here: Codicons,",
                "Copyright (c) Microsoft Corporation, https://github.com/microsoft/vscode-codicons;",
                "Font Awesome, Copyright (c) Fonticons, Inc., https://fontawesome.com. The Nerd",
                "Fonts project combined the icons into one font and scaled and aligned them; the",
                "font is otherwise as released. Font Logos is released into the public domain",
                "(the Unlicense).", "",
                "The notices of the other sets, as their projects state them (the Nerd Fonts",
                "release and each set's own repository):", "",
                "  Octicons (MIT): Copyright (c) 2023 GitHub Inc., https://github.com/primer/octicons",
                "  Devicons (MIT): Copyright (c) 2015 konpa, https://github.com/devicons/devicon",
                "  Powerline Symbols: Copyright 2013 Kim Silkebækken and other contributors,",
                "    https://github.com/powerline/powerline",
                "  Pomicons (SIL Open Font License 1.1): Copyright (c) 2021, Gabriele Lana",
                "    (gabriele.lana@gmail.com), with Reserved Font Name Pomicons",
                "  Weather Icons (SIL Open Font License 1.1): Erik Flowers,",
                "    https://github.com/erikflowers/weather-icons", ""]
    return out


if __name__ == "__main__":
    main()
