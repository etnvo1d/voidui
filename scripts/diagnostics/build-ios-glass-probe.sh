#!/bin/sh
# Build only: pass the resulting bundle to simctl install on a research device.
set -eu
cd "$(dirname "$0")/../.."
bundle="$PWD/target/liquid-glass/ios26/GlassProbe.app"
mkdir -p "$bundle"
sdk="$(xcrun --sdk iphonesimulator --show-sdk-path)"
xcrun --sdk iphonesimulator clang -target arm64-apple-ios26.0-simulator \
    -isysroot "$sdk" -fobjc-arc -O1 -g -framework UIKit -framework QuartzCore -framework CoreGraphics \
    scripts/diagnostics/ios-glass-probe.m -o "$bundle/GlassProbe"
cat > "$bundle/Info.plist" <<'PLIST'
<?xml version="1.0" encoding="UTF-8"?>
<plist version="1.0"><dict>
<key>CFBundleExecutable</key><string>GlassProbe</string>
<key>CFBundleIdentifier</key><string>dev.voidui.ios-glass-probe</string>
<key>CFBundleName</key><string>Glass Probe</string>
<key>CFBundlePackageType</key><string>APPL</string>
<key>CFBundleVersion</key><string>1</string>
<key>CFBundleShortVersionString</key><string>1.0</string>
<key>MinimumOSVersion</key><string>26.0</string>
<key>UIDeviceFamily</key><array><integer>1</integer></array>
<key>UILaunchScreen</key><dict/>
<key>UISupportedInterfaceOrientations</key><array><string>UIInterfaceOrientationPortrait</string></array>
</dict></plist>
PLIST
codesign --force --sign - "$bundle"
printf '%s\n' "$bundle"
