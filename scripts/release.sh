#!/bin/bash
# Cut a Convoy release: bump the version, build, zip, install locally, tag, and publish on GitHub.
#   scripts/release.sh 0.5.6 [notes-file]
# Publishes two assets: Convoy-<version>.zip and a stable Convoy.zip, which
# https://github.com/iftech-team/convoy/releases/latest/download/Convoy.zip always resolves to,
# so peshbar.com and the installer never need a version bump.
set -euo pipefail
cd "$(dirname "$0")/.."
version="${1:?usage: scripts/release.sh <version> [notes-file]}"
notes="${2:-}"
plist_line=$(grep -n 'CFBundleShortVersionString' scripts/build-app.sh | cut -d: -f1)
current=$(sed -n "${plist_line}s/.*<string>\(.*\)<\/string>.*/\1/p" scripts/build-app.sh)
build=$(grep -o '<key>CFBundleVersion</key><string>[0-9]*' scripts/build-app.sh | grep -o '[0-9]*$')
next_build=$((build + 1))
[ -z "$(git status --porcelain)" ] || { echo "Working tree is dirty; commit first." >&2; exit 1; }
# Other sessions may have pushed meanwhile; build the release on top of the current main.
git pull --rebase origin main
sed -i '' "s|<string>${current}</string>|<string>${version}</string>|; s|<key>CFBundleVersion</key><string>${build}</string>|<key>CFBundleVersion</key><string>${next_build}</string>|" scripts/build-app.sh
git commit -qam "chore: version ${version}"
git push origin HEAD
bash scripts/build-app.sh
rm -f "dist/Convoy-${version}.zip" dist/Convoy.zip
ditto -c -k --keepParent dist/Convoy.app "dist/Convoy-${version}.zip"
cp "dist/Convoy-${version}.zip" dist/Convoy.zip
rm -rf /Applications/Convoy.app && ditto dist/Convoy.app /Applications/Convoy.app && rm -rf dist/Convoy.app
git tag -a "v${version}" -m "Convoy ${version}"
git push origin "v${version}"
if [ -n "$notes" ]; then
  gh release create "v${version}" "dist/Convoy-${version}.zip" dist/Convoy.zip --title "Convoy ${version}" --notes-file "$notes"
else
  gh release create "v${version}" "dist/Convoy-${version}.zip" dist/Convoy.zip --title "Convoy ${version}" --generate-notes
fi
rm -f dist/Convoy.zip
echo "Released v${version}: https://github.com/iftech-team/convoy/releases/tag/v${version}"
