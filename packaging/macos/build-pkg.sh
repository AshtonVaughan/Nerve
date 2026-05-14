#!/usr/bin/env bash
#
# Build a macOS installer .pkg around the prebuilt `nerve` binary.
#
# Requires:
#   - `nerve` built for the active architecture under core/target/release/
#   - `pkgbuild` and `productbuild` (ship with macOS Xcode CLT)
#   - optional: `productsign -s "Developer ID Installer: ..."` for signing
#
# Run from the repo root:  packaging/macos/build-pkg.sh [version]

set -euo pipefail

VERSION="${1:-0.1.0}"
BIN="core/target/release/nerve"

if [ ! -x "$BIN" ]; then
  echo "Build the daemon first: (cd core && cargo build --release)"
  exit 1
fi

WORK="$(mktemp -d)"
ROOT="$WORK/root"
mkdir -p "$ROOT/usr/local/bin"
mkdir -p "$ROOT/Library/LaunchDaemons"

cp "$BIN" "$ROOT/usr/local/bin/nerve"
cat > "$ROOT/Library/LaunchDaemons/dev.nerve.daemon.plist" <<'PLIST'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key><string>dev.nerve.daemon</string>
  <key>ProgramArguments</key>
  <array>
    <string>/usr/local/bin/nerve</string>
    <string>start</string>
  </array>
  <key>RunAtLoad</key><true/>
  <key>KeepAlive</key><true/>
  <key>StandardOutPath</key><string>/var/log/nerve.log</string>
  <key>StandardErrorPath</key><string>/var/log/nerve.log</string>
</dict>
</plist>
PLIST

pkgbuild --root "$ROOT" \
  --identifier dev.nerve.daemon \
  --version "$VERSION" \
  --install-location / \
  "$WORK/nerve-daemon.pkg"

productbuild --distribution packaging/macos/Distribution.xml \
  --package-path "$WORK" \
  --version "$VERSION" \
  "nerve-$VERSION.pkg"

echo "wrote nerve-$VERSION.pkg"
