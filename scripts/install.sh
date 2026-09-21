#!/bin/bash
# Build Convoy, zip it, install to /Applications, and remove the build copy so Spotlight shows one app.
set -euo pipefail
cd "$(dirname "$0")/.."
bash scripts/build-app.sh
version=$(defaults read "$PWD/dist/Convoy.app/Contents/Info.plist" CFBundleShortVersionString)
rm -f "dist/Convoy-$version.zip"
ditto -c -k --keepParent dist/Convoy.app "dist/Convoy-$version.zip"
pkill -x Convoy 2>/dev/null || true
sleep 1
rm -rf /Applications/Convoy.app /Applications/SpecDesk.app
ditto dist/Convoy.app /Applications/Convoy.app
rm -rf dist/Convoy.app dist/SpecDesk.app
echo "Installed /Applications/Convoy.app ($version); archive: dist/Convoy-$version.zip"
