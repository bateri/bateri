#!/bin/bash
# Makes a local release of the browser engine, for `make dev BROWSER_RELEASES=<folder>`: the
# folder holds what a GitHub release of bateri/browser holds — the helper's archive, its
# SHA256SUMS and notice.json — so that the development copy downloads the engine with the real
# downloader (the notice, the digests, the unpacking) from a folder instead of from GitHub.
#
#   browser-release.sh <the helper's app> <the folder>
#
# This is not a release: nothing is signed, notarized, uploaded or published here. The app is
# archived as it is (`bateri-browser.app` from the browser repository's `make app`, with the
# signature it was built with, which a development copy does not check), and the folder is the
# only thing written. Files of the folder this does not make are left alone; the three it makes
# are replaced. Environment: `CHROMIUM` (the engine's version, as the notice says it; read from
# the app's framework when not given) and `PROTOCOL` (the helper's wire protocol, 1).
set -euo pipefail

if [ "$#" -ne 2 ]; then
    echo "usage: $0 <the helper's app> <the folder of the release>" >&2
    exit 2
fi
app=$1
folder=$2

[ -d "$app/Contents/MacOS" ] && [ -f "$app/Contents/Info.plist" ] || {
    echo "browser-release: $app is not an app" >&2
    exit 1
}
[ "$(basename "$app")" = "bateri-browser.app" ] || {
    echo "browser-release: the helper's app is bateri-browser.app, not $(basename "$app")" >&2
    exit 1
}
codesign --verify --deep --strict "$app" 2>/dev/null || {
    echo "browser-release: $app is not signed (the browser repository's \`make app\` signs it)" >&2
    exit 1
}

plist() { /usr/libexec/PlistBuddy -c "Print :$2" "$1" 2>/dev/null; }

version=$(plist "$app/Contents/Info.plist" CFBundleShortVersionString)
case "$version" in
    [0-9]*.[0-9]*.[0-9]*) ;;
    *) echo "browser-release: the app's version is '$version', not x.y.z" >&2; exit 1 ;;
esac
chromium=${CHROMIUM:-$(plist "$app/Contents/Frameworks/Chromium Embedded Framework.framework/Resources/Info.plist" CFBundleShortVersionString || true)}
chromium=${chromium:-unknown}
protocol=${PROTOCOL:-1}
arch=$(uname -m | sed 's/aarch64/arm64/')
name=bateri-browser-$version-$arch.zip

mkdir -p "$folder"
folder=$(cd "$folder" && pwd)
rm -f "$folder/$name" "$folder/SHA256SUMS" "$folder/notice.json"
ditto -c -k --sequesterRsrc --keepParent "$app" "$folder/$name"
sha256=$(shasum -a 256 "$folder/$name" | cut -d' ' -f1)
size=$(stat -f%z "$folder/$name")
printf '%s  %s\n' "$sha256" "$name" >"$folder/SHA256SUMS"
printf '{\n  "format": 1,\n  "version": "%s",\n  "chromium": "%s",\n  "protocol": %s,\n  "minimum": "%s",\n  "assets": [\n    {\n      "arch": "%s",\n      "name": "%s",\n      "size": %s,\n      "sha256": "%s"\n    }\n  ]\n}\n' \
    "$version" "$chromium" "$protocol" "$version" "$arch" "$name" "$size" "$sha256" >"$folder/notice.json"

echo "browser-release: $folder holds $name ($size bytes), SHA256SUMS and notice.json (version $version; nothing was signed, uploaded or published)"
echo "browser-release: \`make dev BROWSER_RELEASES=$folder\` makes the development copy download the engine from it"
