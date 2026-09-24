#!/bin/bash
set -euo pipefail
cd "$(dirname "$0")/.."
export CLANG_MODULE_CACHE_PATH="$PWD/.build/ModuleCache"
export SWIFTPM_MODULECACHE_OVERRIDE="$PWD/.build/ModuleCache"
swift build -c release --disable-sandbox --cache-path .build/cache --config-path .build/config --security-path .build/security
# SwiftPM puts dependency resources at the bundle root. A signed macOS app
# requires them in Contents/Resources, so adjust the generated release accessor.
python3 - <<'PY'
from pathlib import Path
p = Path('.build/release/SwiftTerm.build/DerivedSources/resource_bundle_accessor.swift')
s = p.read_text()
updated = s.replace('Bundle.main.bundleURL.appendingPathComponent', '(Bundle.main.resourceURL ?? Bundle.main.bundleURL).appendingPathComponent')
if updated != s:
    p.write_text(updated)
PY
swift build -c release --disable-sandbox --cache-path .build/cache --config-path .build/config --security-path .build/security
app="dist/Convoy.app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
cp .build/release/Convoy "$app/Contents/MacOS/Convoy"
cp .build/release/ConvoyStatus "$app/Contents/MacOS/ConvoyStatus"
python3 - <<'PY'
from pathlib import Path
import shutil
old = Path('dist/Convoy.app/SwiftTerm_SwiftTerm.bundle')
if old.exists():
    shutil.rmtree(old)
destination = Path('dist/Convoy.app/Contents/Resources/SwiftTerm_SwiftTerm.bundle')
if destination.exists():
    shutil.rmtree(destination)
shutil.copytree('.build/release/SwiftTerm_SwiftTerm.bundle', destination)
PY
cat > "$app/Contents/Info.plist" <<'PLIST'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>CFBundleExecutable</key><string>Convoy</string>
<key>CFBundleIdentifier</key><string>com.iftech.convoy</string>
<key>CFBundleName</key><string>Convoy</string>
<key>CFBundleDisplayName</key><string>Convoy</string>
<key>CFBundleIconFile</key><string>AppIcon</string>
<key>CFBundlePackageType</key><string>APPL</string>
<key>CFBundleShortVersionString</key><string>0.6.2</string>
<key>CFBundleVersion</key><string>17</string>
<key>LSMinimumSystemVersion</key><string>14.0</string>
<key>NSHighResolutionCapable</key><true/>
</dict></plist>
PLIST
if [ -f Resources/AppIcon.icns ]; then cp Resources/AppIcon.icns "$app/Contents/Resources/AppIcon.icns"; fi
# Signing. Default is ad-hoc (local use). For distribution set SIGN_IDENTITY to a
# "Developer ID Application: …" certificate; the hardened runtime is required for notarization.
# Optionally set NOTARY_PROFILE to a `xcrun notarytool store-credentials` profile name to notarize and staple.
identity="${SIGN_IDENTITY:--}"
if [ "$identity" = "-" ]; then
  codesign --force --sign - "$app/Contents/MacOS/ConvoyStatus"
  codesign --force --sign - "$app"
else
  codesign --force --options runtime --timestamp --entitlements Convoy.entitlements --sign "$identity" "$app/Contents/MacOS/ConvoyStatus"
  codesign --force --options runtime --timestamp --entitlements Convoy.entitlements --deep --sign "$identity" "$app"
  codesign --verify --deep --strict "$app"
  if [ -n "${NOTARY_PROFILE:-}" ]; then
    ditto -c -k --keepParent "$app" "dist/notarize.zip"
    xcrun notarytool submit "dist/notarize.zip" --keychain-profile "$NOTARY_PROFILE" --wait
    xcrun stapler staple "$app"
    rm -f "dist/notarize.zip"
    spctl --assess --type execute "$app" && echo "Notarized and stapled."
  fi
fi
echo "Built $PWD/$app"
