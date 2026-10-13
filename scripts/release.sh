#!/bin/bash
# Cut a Convoy release: tag main and publish it on GitHub.
#   scripts/release.sh 0.9.0 [notes-file]
# Publishing starts .github/workflows/convoy-release.yml, which builds the
# macOS, Windows and Linux apps with the release version and attaches them.
set -euo pipefail
cd "$(dirname "$0")/.."
version="${1:?usage: scripts/release.sh <version> [notes-file]}"
notes="${2:-}"
[ -z "$(git status --porcelain)" ] || { echo "Working tree is dirty; commit first." >&2; exit 1; }
# Other sessions may have pushed meanwhile; release the current main.
git pull --rebase origin main
git push origin HEAD
git tag -a "v${version}" -m "Convoy ${version}"
git push origin "v${version}"
if [ -n "$notes" ]; then
  gh release create "v${version}" --title "Convoy ${version}" --notes-file "$notes"
else
  gh release create "v${version}" --title "Convoy ${version}" --generate-notes
fi
echo "Published v${version}; the builds attach themselves: gh run watch"
