#!/bin/sh
# Build an app bundle for native-window testing. This does not install or launch it.
set -eu
cd "$(dirname "$0")/.."
if [ "$(uname -s)" != Darwin ]; then
    echo "This bundle is for macOS; run cargo run --example hello on other desktops." >&2
    exit 1
fi
cargo build --example hello
bundle="${VOIDUI_BUNDLE_PATH:-$PWD/target/Voidui.app}"
mkdir -p "$bundle/Contents/MacOS"
cp target/debug/examples/hello "$bundle/Contents/MacOS/hello"
python3 - "$bundle" <<'PY'
import os, plistlib, sys
from pathlib import Path
bundle = Path(sys.argv[1])
info = {
    'CFBundleExecutable': 'hello',
    'CFBundleIdentifier': os.environ.get('VOIDUI_BUNDLE_ID', 'org.voidui.demo'),
    'CFBundleName': 'Voidui',
    'CFBundleDisplayName': 'Voidui',
    'CFBundlePackageType': 'APPL',
    'CFBundleVersion': '1',
    'NSHighResolutionCapable': True,
    'NSPrincipalClass': 'NSApplication',
}
with (bundle / 'Contents/Info.plist').open('wb') as output:
    plistlib.dump(info, output)
PY
printf '%s\n' "$bundle"
