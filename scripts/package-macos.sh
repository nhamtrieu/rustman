#!/usr/bin/env bash
# Build Rustman.app and a drag-to-Applications .dmg from a release binary. macOS only
# (sips, iconutil, codesign, hdiutil).
#
# Usage: scripts/package-macos.sh BINARY VERSION BUILD OUT.dmg
#   VERSION: 0.2.0 (shown in Finder); BUILD: an increasing build number.
# The app is ad-hoc signed, not notarized: the first launch needs right-click → Open.
set -euo pipefail

bin=$1
version=$2
build=$3
out=$4
repo=$(cd "$(dirname "$0")/.." && pwd)
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

app=$work/dmg/Rustman.app
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
cp "$bin" "$app/Contents/MacOS/rustman"
chmod 755 "$app/Contents/MacOS/rustman"
sed -e "s/@VERSION@/$version/" -e "s/@BUILD@/$build/" "$repo/packaging/macos/Info.plist" > "$app/Contents/Info.plist"

# Icon: every size iconutil expects, scaled down from the 512 px render.
iconset=$work/rustman.iconset
mkdir -p "$iconset"
for s in 16 32 128 256 512; do
  sips -z "$s" "$s" "$repo/assets/logo/icon-512.png" --out "$iconset/icon_${s}x${s}.png" >/dev/null
  d=$((s * 2))
  if [ "$d" -le 512 ]; then
    sips -z "$d" "$d" "$repo/assets/logo/icon-512.png" --out "$iconset/icon_${s}x${s}@2x.png" >/dev/null
  fi
done
iconutil -c icns "$iconset" -o "$app/Contents/Resources/rustman.icns"

codesign --force --deep --sign - "$app"
ln -s /Applications "$work/dmg/Applications"
hdiutil create -volname "Rustman" -srcfolder "$work/dmg" -ov -format UDZO "$out" >/dev/null
echo "Built $out"
