#!/usr/bin/env python3
"""Prints the folder of the pencere checkout this build is pinned to.

The kit (`pencere-sandbox`, `pencere-view`, `pencere-host`) comes from one commit of
github.com/bateri/pencere, which cargo keeps in its git checkouts. The page modules and the
example script the tests try the sandbox with are in that checkout's `kit/`, and this is how
`make guest` finds it: from what cargo resolved (`Cargo.lock`), never from a folder that
happens to sit beside this repository.
"""
import json
import os
import subprocess
import sys


def main() -> None:
    run = subprocess.run(
        ["cargo", "metadata", "--format-version", "1", "--locked", "--offline"],
        capture_output=True,
        text=True,
    )
    if run.returncode != 0:
        sys.exit(f"pencere_checkout: cargo metadata failed:\n{run.stderr}")
    found = {
        os.path.dirname(os.path.dirname(os.path.dirname(package["manifest_path"])))
        for package in json.loads(run.stdout)["packages"]
        if package["name"] == "pencere-sandbox"
    }
    if len(found) != 1:
        sys.exit(f"pencere_checkout: expected one pencere-sandbox in the build, found {len(found)}")
    (folder,) = found
    if not os.path.isdir(os.path.join(folder, "kit", "guest")):
        sys.exit(f"pencere_checkout: {folder} holds no kit/guest")
    print(folder)


if __name__ == "__main__":
    main()
