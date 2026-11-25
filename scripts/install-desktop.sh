#!/usr/bin/env sh
# Install the Rustman launcher and icon for the current user on Linux desktops
# (GNOME, KDE, ...). The dock/taskbar matches the window's app_id "rustman" to
# rustman.desktop, so without this it shows a generic icon.
#
# Usage: scripts/install-desktop.sh [path/to/rustman]   (default: target/release/rustman)
#        scripts/install-desktop.sh --uninstall
set -eu

repo=$(cd "$(dirname "$0")/.." && pwd)
data=${XDG_DATA_HOME:-$HOME/.local/share}
apps=$data/applications
icon_dir=$data/icons/hicolor/256x256/apps
scalable_dir=$data/icons/hicolor/scalable/apps

refresh() {
    command -v update-desktop-database >/dev/null 2>&1 && update-desktop-database -q "$apps" || true
    # The per-user hicolor dir usually has no index.theme; desktops find icons there without a cache.
    if [ -f "$data/icons/hicolor/index.theme" ] && command -v gtk-update-icon-cache >/dev/null 2>&1; then
        gtk-update-icon-cache -q -t "$data/icons/hicolor" || true
    fi
}

if [ "${1:-}" = "--uninstall" ]; then
    rm -f "$apps/rustman.desktop" "$icon_dir/rustman.png" "$scalable_dir/rustman.svg"
    refresh
    echo "Removed Rustman launcher and icon."
    exit 0
fi

bin=${1:-$repo/target/release/rustman}
if [ ! -x "$bin" ]; then
    echo "Binary not found: $bin (build it with: cargo build --release)" >&2
    exit 1
fi
bin=$(cd "$(dirname "$bin")" && pwd)/$(basename "$bin")

mkdir -p "$apps" "$icon_dir" "$scalable_dir"
cp "$repo/assets/logo/icon-256.png" "$icon_dir/rustman.png"
cp "$repo/assets/logo/logo.svg" "$scalable_dir/rustman.svg"

cat > "$apps/rustman.desktop" <<EOF
[Desktop Entry]
Type=Application
Name=Rustman
GenericName=API Client
Comment=Native Postman-style API client written in Rust
Exec="$bin"
Icon=rustman
Terminal=false
Categories=Development;
Keywords=api;http;rest;postman;graphql;
StartupWMClass=rustman
EOF

refresh
echo "Installed $apps/rustman.desktop (Exec=$bin)"
