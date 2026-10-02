#!/usr/bin/env python3
"""Which workspace crates `make quick` checks, from what changed.

A crate is "changed" when a file under its directory differs from HEAD
(staged, unstaged or untracked). Printed on two lines:

    lint: the changed crates plus every workspace crate that depends on them —
          an API change in bt-core must still compile in bt-gpu and above;
    test: the changed crates only — their own tests; the dependents' tests are
          the full gate's (`make check`).

A change to a workspace-wide file (Cargo.toml, Cargo.lock, Makefile,
.cargo/) counts every crate as changed. Nothing changed prints two empty lines.
"""

import json
import subprocess
import sys

WIDE = ("Cargo.toml", "Cargo.lock", "Makefile", ".cargo/")


def git_lines(*args):
    out = subprocess.run(["git", *args], capture_output=True, text=True, check=True)
    return [line for line in out.stdout.splitlines() if line]


def main():
    meta = json.loads(
        subprocess.run(
            ["cargo", "metadata", "--format-version", "1", "--no-deps"],
            capture_output=True,
            text=True,
            check=True,
        ).stdout
    )
    root = meta["workspace_root"].rstrip("/") + "/"
    crates = {}
    for package in meta["packages"]:
        directory = package["manifest_path"][len(root) :].rsplit("/", 1)[0] + "/"
        local = {
            dep["name"]
            for dep in package["dependencies"]
            if dep.get("path")
        }
        crates[package["name"]] = (directory, local)

    files = git_lines("diff", "--name-only", "HEAD") + git_lines(
        "ls-files", "--others", "--exclude-standard"
    )
    if any(f == w or f.startswith(w) for f in files for w in WIDE):
        changed = set(crates)
    else:
        changed = {
            name
            for name, (directory, _) in crates.items()
            if any(f.startswith(directory) for f in files)
        }

    lint = set(changed)
    grew = True
    while grew:
        grew = False
        for name, (_, deps) in crates.items():
            if name not in lint and deps & lint:
                lint.add(name)
                grew = True

    print(" ".join(sorted(lint)))
    print(" ".join(sorted(changed)))
    return 0


if __name__ == "__main__":
    sys.exit(main())
