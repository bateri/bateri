# The tests of the programs bateri follows

`main.rs` runs `bt_shell_common::apps` against a temporary apps folder and a made-up network (a map
of addresses to bytes), and, for the `file:` sources, against the real `curl`. `support.rs` is what
the scenarios are made of: the network, the folder, and authors who sign the releases the tests
publish with keys the tests invent.

## `fixture/`

One release of a program called `demo`, made **once** with pencere's `pencere dist` (a release
step of the author's, which asks the program itself what the release needs). It is here to prove
that what the tool writes is what bateri reads: the `app.json` and its signature are the tool's
bytes, not something these tests wrote. The tests serve its files at the addresses its `app.json`
names (`https://apps.example.test/demo/1.0.0/`).

| file | what it is |
|---|---|
| `app.json`, `app.json.sig` | the release and its signature, as `pencere dist` wrote them |
| `demo-1.0.0-<target>` | the program of each target: three lines of shell, so it can run on a server too |
| `SHA256SUMS` | the sums, as the tool writes them (the tests do not read it) |
| `throwaway.key` | the key the fixture is signed with: **a key made for these tests and nothing else**. It guards nothing; it is not bateri's key (whose secret half is never in this repository: only its public half, `BATERI_KEY`) |

The program says what `demo` needs (`protocol` 1, `views` `lua` 0.1) by `--describe-with`:
the files are shell scripts, which send no `hello`, so the tool asks bateri's own `demopen`
(`crates/bt-core/examples/demopen.rs`, `make demopen`), which shows a view of the kind `lua` 0.1 —
the one kit version `Config.views` offers.

### Making it again

From the root of the repository, with `pencere` (`cargo install --git https://github.com/bateri/pencere
pencere-cli --locked`, or one built from a checkout) on the `PATH`. The result is the same bytes:
the signature is deterministic, so a run that changes `fixture/` has changed an input.

```sh
make demopen
fixture=crates/bt-shell-common/tests/apps/fixture
work=$(mktemp -d)
for target in aarch64-apple-darwin x86_64-apple-darwin x86_64-unknown-linux-musl aarch64-unknown-linux-musl; do
  printf '#!/bin/sh\n# demo 1.0.0 for %s: a stand-in program for the tests of bateri'"'"'s apps (see ../README.md).\necho "demo 1.0.0"\n' "$target" > "$work/demo-1.0.0-$target"
  chmod 755 "$work/demo-1.0.0-$target"
done
pencere dist 1.0.0 --name demo --files "$work" --key "$fixture/throwaway.key" \
  --base https://apps.example.test/demo/1.0.0/ --previous none \
  --describe-with target/debug/examples/demopen
cp "$work"/* "$fixture"/
rm -r "$work"
```

(`pencere key --key FILE` makes `throwaway.key` if it is ever lost; the fixture is then signed with
another key and `fixture_key()` in `main.rs` reads the new one from `app.json`.)

### A `file:` source to look at by hand

The same command with `--base file:///some/folder/` instead, run on a folder that is the one named,
gives a release that bateri follows from `[apps] sources = ["file:///some/folder/app.json"]`: the
key is not bateri's, so it waits for its approval — write the key from `app.json` to
`~/Library/Application Support/bateri/apps/demo/key` (one line, `ed25519:…`) to approve it by hand,
until the Apps page does.
