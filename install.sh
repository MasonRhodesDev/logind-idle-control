#!/bin/bash

set -euo pipefail

INSTALL_DIR="$HOME/.local/bin"
SYSTEMD_DIR="$HOME/.config/systemd/user"

echo "Installing logind-idle-control..."

mkdir -p "$INSTALL_DIR"
mkdir -p "$SYSTEMD_DIR"

echo "→ Installing binaries to $INSTALL_DIR"
cp logind-idle-daemon "$INSTALL_DIR/"
cp logind-idle-ctl "$INSTALL_DIR/"
cp waybar-module.sh "$INSTALL_DIR/logind-idle-waybar"

chmod +x "$INSTALL_DIR/logind-idle-daemon"
chmod +x "$INSTALL_DIR/logind-idle-ctl"
chmod +x "$INSTALL_DIR/logind-idle-waybar"

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
echo "  1. Add waybar integration (see README.md)"
echo "  2. Test CLI: logind-idle-ctl status"
echo "  3. Verify: systemd-inhibit --list | grep logind-idle-control"
