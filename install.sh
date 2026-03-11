#!/bin/bash

set -euo pipefail

INSTALL_DIR="$HOME/.local/bin"
SYSTEMD_DIR="$HOME/.config/systemd/user"
ICON_DIR="$HOME/.local/share/icons"

echo "Installing logind-idle-control..."

mkdir -p "$INSTALL_DIR"
mkdir -p "$SYSTEMD_DIR"

echo "→ Installing binaries to $INSTALL_DIR"
cp target/release/logind-idle-control "$INSTALL_DIR/"
cp target/release/logind-idle-control-tray "$INSTALL_DIR/" 2>/dev/null || true

chmod +x "$INSTALL_DIR/logind-idle-control"
chmod +x "$INSTALL_DIR/logind-idle-control-tray" 2>/dev/null || true

echo "→ Installing tray icons"
mkdir -p "$ICON_DIR/hicolor/scalable/status"
mkdir -p "$ICON_DIR/breeze/status/22"
mkdir -p "$ICON_DIR/breeze-dark/status/22"
for icon in caffeine-cup-full-symbolic.svg caffeine-cup-empty-symbolic.svg; do
    cp "icons/$icon" "$ICON_DIR/hicolor/scalable/status/"
    cp "icons/$icon" "$ICON_DIR/breeze/status/22/"
    cp "icons/$icon" "$ICON_DIR/breeze-dark/status/22/"
done
gtk-update-icon-cache -f -t "$ICON_DIR/hicolor" 2>/dev/null || true

echo "→ Installing systemd service to $SYSTEMD_DIR"
cp systemd/logind-idle-control.service "$SYSTEMD_DIR/"

echo "→ Reloading systemd user daemon"
systemctl --user daemon-reload

echo "→ Enabling and starting service"
systemctl --user enable logind-idle-control.service
systemctl --user restart logind-idle-control.service

echo ""
echo "✓ Installation complete!"
echo ""
echo "Service status:"
systemctl --user status logind-idle-control.service --no-pager -l

echo ""
echo "Next steps:"
echo "  1. Test CLI: logind-idle-control status"
echo "  2. Verify: systemd-inhibit --list | grep logind-idle-control"
