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
- `alacritty_terminal` yalnız Apache-2.0: metni tam olarak.
- Geri kalan her crate MIT'i seçenek olarak taşıyor (MIT, "MIT OR
  Apache-2.0", "Zlib OR Apache-2.0 OR MIT", "Unlicense OR MIT"); bildirim
  MIT'le veriliyor — crate başına telif satırı, metin bir kez. Telif satırı
  crate'in kendi lisans dosyasından, dosyada yoksa `Cargo.toml`'un
  yazarlarından. Lisansında MIT seçeneği olmayan crate betiği durdurur:
  o bir bağımlılık kararıdır, sessizce listeye eklenmez.
- Sparkle'ın LICENSE'ı (MIT + bsdiff/sais/ed25519 gibi dış bildirimler)
  olduğu gibi.
"""

import json
import os
import subprocess
import sys

ROOT = "bateri"
TARGET = "aarch64-apple-darwin"
APACHE_ONLY = {"alacritty_terminal"}
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


def main():
    if len(sys.argv) != 2:
        sys.exit("kullanım: third_party_notices.py <sparkle dizini>")
    sparkle_license = os.path.join(sys.argv[1], "LICENSE")
    pkgs = packages()
    crates = sorted(product_crates())

    out = ["bateri includes, among others, the following third-party software.", ""]

    for name, version in crates:
        if name not in APACHE_ONLY:
            continue
        pkg = pkgs[(name, version)]
        with open(os.path.join(os.path.dirname(pkg["manifest_path"]), "LICENSE-APACHE"),
                  encoding="utf-8") as f:
            text = f.read().rstrip()
        out += [RULE, name, pkg.get("repository") or "", *copyright_lines(pkg),
                "Licensed under the Apache License, Version 2.0; full text below.",
                RULE, "", text, "", ""]

    out += [RULE,
            "The following Rust crates are used under the MIT License (each offers",
            "MIT, alone or as one of its license options). The license text follows",
            "the list.",
            RULE, ""]
    for name, version in crates:
        if name in APACHE_ONLY:
            continue
        pkg = pkgs[(name, version)]
        license_expr = pkg.get("license") or ""
        if "MIT" not in license_expr.replace("(", " ").replace(")", " ").split():
            sys.exit(f"{name} {version}: lisans '{license_expr}' MIT seçeneği taşımıyor")
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
