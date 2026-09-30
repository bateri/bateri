#!/usr/bin/env python3
"""Pakete giren üçüncü taraf yazılımın bildirimlerini üretir.

Kullanım (depo kökünden, `make kur` Sparkle'ı indirdikten sonra):

    python3 tools/third_party_notices.py target/sparkle-<sürüm> \
        > assets/bundle/THIRD-PARTY-LICENSES.txt

Neden var: MIT ve Apache-2.0 bildirimin kopyalarla gitmesini istiyor ve
`bateri.app` dağıtılıyor. Liste elle tutulunca bağımlılık eklenince
sessizce eskiyordu (`docs/YOL-HARITASI.md`'deki borç); ölçü bu yüzden
`cargo tree` — `bateri`'nin **ürün** grafı (normal + build kenarları,
aarch64-apple-darwin), dev-dependency'ler (wgpu denemesi gibi) girmiyor.

Seçimler:
- MIT seçeneği olmayan ve lisansı izin listesinde (`OWN_LICENSES`) olan
  crate'ler kendi metinleriyle: Apache-2.0 tam metin (+ varsa NOTICE),
  Zlib ve ISC crate'in kendi lisans dosyası (telif satırı içinde).
- Geri kalan her crate MIT'i seçenek olarak taşıyor (MIT, "MIT OR
  Apache-2.0", "Zlib OR Apache-2.0 OR MIT", "Unlicense OR MIT"); bildirim
  MIT'le veriliyor — crate başına telif satırı, metin bir kez. Telif satırı
  crate'in kendi lisans dosyasından, dosyada yoksa `Cargo.toml`'un
  yazarlarından. Lisansında ne MIT seçeneği olan ne de izin listesinde
  duran crate betiği durdurur: o bir bağımlılık kararıdır, sessizce listeye
  eklenmez (GPL-2.0-only asla — bateri GPL-3.0-or-later).
- Sparkle'ın LICENSE'ı (MIT + bsdiff/sais/ed25519 gibi dış bildirimler)
  olduğu gibi.
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
# Lisans dosyası telif satırı taşımayan crate'lerde yazar listesi yerine
# projenin kendi atfı (alacritty'nin reposu ve `Credits.html` bunu kullanıyor).
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
    """`bateri`'nin ürün grafındaki (ad, sürüm) çiftleri, kendi crate'lerimiz hariç."""
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
    """Crate'in lisans dosyasındaki telif satırları; yoksa yazarlardan bir satır."""
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
        # Sahibi adsız satır ("Copyright (c) 2016--2017") ve yönlendirme
        # dosyası (Unlicense/COPYING) bildirim sayılmıyor; sonraki aday, en
        # sonda yazarlar.
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
        sys.exit(f"{pkg['name']} {pkg['version']}: lisans '{expr}' ne MIT seçeneği "
                 "taşıyor ne izin listesinde")
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
        sys.exit("kullanım: third_party_notices.py <sparkle dizini>")
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
                sys.exit(f"{name} {version}: Apache-2.0 metni bulunamadı")
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
                sys.exit(f"{name} {version}: {license} lisans dosyası bulunamadı")
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
