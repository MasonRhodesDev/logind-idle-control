#!/bin/bash

set -euo pipefail

INSTALL_DIR="$HOME/.local/bin"
SYSTEMD_DIR="$HOME/.config/systemd/user"

echo "Installing logind-idle-control..."

mkdir -p "$INSTALL_DIR"
mkdir -p "$SYSTEMD_DIR"

echo "→ Installing binaries to $INSTALL_DIR"
cp target/release/logind-idle-control "$INSTALL_DIR/"

chmod +x "$INSTALL_DIR/logind-idle-control"

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
