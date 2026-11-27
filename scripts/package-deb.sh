#!/usr/bin/env sh
# Build a Debian/Ubuntu package from a release binary.
#
# Usage: scripts/package-deb.sh BINARY VERSION OUT.deb
#   VERSION is a Debian version: 0.2.0, or 0.2.0~nightly.42 for a pre-release build.
# Needs dpkg-deb (any Debian/Ubuntu system).
set -eu

bin=$1
version=$2
out=$3
repo=$(cd "$(dirname "$0")/.." && pwd)
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
root=$work/rustman

install -Dm755 "$bin" "$root/usr/bin/rustman"
install -Dm644 "$repo/packaging/linux/rustman.desktop" "$root/usr/share/applications/rustman.desktop"
install -Dm644 "$repo/assets/logo/icon-256.png" "$root/usr/share/icons/hicolor/256x256/apps/rustman.png"
install -Dm644 "$repo/assets/logo/icon.svg" "$root/usr/share/icons/hicolor/scalable/apps/rustman.svg"
install -Dm644 "$repo/LICENSE" "$root/usr/share/doc/rustman/copyright"

size=$(du -sk "$root/usr" | cut -f1)
mkdir -p "$root/DEBIAN"
cat > "$root/DEBIAN/control" <<EOF
Package: rustman
Version: $version
Section: devel
Priority: optional
Architecture: amd64
Installed-Size: $size
Depends: libc6 (>= 2.35), libgcc-s1, libxkbcommon0, libgl1 | libegl1
Recommends: xdg-desktop-portal
Maintainer: nhamtrieu <nhamtrieu@users.noreply.github.com>
Homepage: https://github.com/nhamtrieu/rustman
Description: Native Postman-style API client written in Rust
 Collections, environments, OAuth 2.0, AWS Signature and Digest auth,
 Postman-compatible pm scripts, mock servers, MCP toolsets and a
 newman-like command line runner (rustman run).
EOF

dpkg-deb --build --root-owner-group "$root" "$out" >/dev/null
echo "Built $out"
