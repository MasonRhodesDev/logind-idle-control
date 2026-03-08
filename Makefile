.PHONY: all build build-tray install install-tray uninstall clean rpm help

PREFIX ?= $(HOME)/.local
BINDIR = $(PREFIX)/bin
SYSTEMD_USER_DIR = $(HOME)/.config/systemd/user

VERSION = $(shell grep '^version' Cargo.toml | head -1 | cut -d'"' -f2)
PACKAGE_NAME = logind-idle-control
BINARY = target/release/$(PACKAGE_NAME)
TRAY_BINARY = target/release/$(PACKAGE_NAME)-tray

all: build build-tray

build:
	@echo "Building $(PACKAGE_NAME) v$(VERSION)..."
	cargo build --release

build-tray:
	@echo "Building $(PACKAGE_NAME)-tray v$(VERSION)..."
	cargo build --release --features tray

install: build build-tray
	@echo "Installing $(PACKAGE_NAME) v$(VERSION)..."
	@echo "Installing binaries to $(BINDIR)/"
	@mkdir -p $(BINDIR)
	@install -m 755 $(BINARY) $(BINDIR)/$(PACKAGE_NAME)
	@install -m 755 $(TRAY_BINARY) $(BINDIR)/$(PACKAGE_NAME)-tray
	@echo "Installing systemd services..."
	@mkdir -p $(SYSTEMD_USER_DIR)
	@install -m 644 systemd/logind-idle-control.service $(SYSTEMD_USER_DIR)/
	@install -m 644 systemd/logind-idle-control-tray.service $(SYSTEMD_USER_DIR)/
	@echo "Reloading systemd user daemon..."
	@systemctl --user daemon-reload || true
	@if systemctl --user is-active --quiet logind-idle-control.service; then \
		echo "Restarting logind-idle-control service..."; \
		systemctl --user restart logind-idle-control.service; \
	else \
		echo "Enabling and starting logind-idle-control service..."; \
		systemctl --user enable --now logind-idle-control.service || true; \
	fi
	@if systemctl --user is-active --quiet logind-idle-control-tray.service; then \
		echo "Restarting logind-idle-control-tray service..."; \
		systemctl --user restart logind-idle-control-tray.service; \
	else \
		echo "Enabling and starting logind-idle-control-tray service..."; \
		systemctl --user enable --now logind-idle-control-tray.service || true; \
	fi
	@echo ""
	@echo "✓ Installation complete!"
	@echo ""
	@echo "Service status:"
	@systemctl --user status logind-idle-control.service logind-idle-control-tray.service --no-pager -l || true

uninstall:
	@echo "Uninstalling $(PACKAGE_NAME)..."
	@if systemctl --user is-active --quiet logind-idle-control-tray.service; then \
		echo "Stopping tray service..."; \
		systemctl --user stop logind-idle-control-tray.service; \
	fi
	@if systemctl --user is-enabled --quiet logind-idle-control-tray.service 2>/dev/null; then \
		echo "Disabling tray service..."; \
		systemctl --user disable logind-idle-control-tray.service; \
	fi
	@if systemctl --user is-active --quiet logind-idle-control.service; then \
		echo "Stopping daemon service..."; \
		systemctl --user stop logind-idle-control.service; \
	fi
	@if systemctl --user is-enabled --quiet logind-idle-control.service 2>/dev/null; then \
		echo "Disabling daemon service..."; \
		systemctl --user disable logind-idle-control.service; \
	fi
	@echo "Removing systemd service files..."
	@rm -f $(SYSTEMD_USER_DIR)/logind-idle-control.service
	@rm -f $(SYSTEMD_USER_DIR)/logind-idle-control-tray.service
	@echo "Removing binaries..."
	@rm -f $(BINDIR)/$(PACKAGE_NAME)
	@rm -f $(BINDIR)/$(PACKAGE_NAME)-tray
	@echo "Reloading systemd daemon..."
	@systemctl --user daemon-reload || true
	@echo ""
	@echo "✓ Uninstallation complete!"
	@echo ""
	@echo "Note: Config files and state files preserved at:"
	@echo "  ~/.config/logind-idle-control/"
	@echo "  \$$XDG_RUNTIME_DIR/logind-idle-control-*.state"

clean:
	@echo "Cleaning build artifacts..."
	cargo clean
	@echo "✓ Clean complete!"

rpm: build
	@echo "Building RPM package..."
	@mkdir -p rpmbuild/{BUILD,RPMS,SOURCES,SPECS,SRPMS}
	@cp logind-idle-control.spec rpmbuild/SPECS/
	@tar czf rpmbuild/SOURCES/$(PACKAGE_NAME)-$(VERSION).tar.gz \
		--transform 's,^,$(PACKAGE_NAME)-$(VERSION)/,' \
		--exclude=target \
		--exclude=.git \
		--exclude=rpmbuild \
		Cargo.toml Cargo.lock src/ systemd/ README.md LICENSE
	@rpmbuild --define "_topdir $(PWD)/rpmbuild" \
		-ba rpmbuild/SPECS/logind-idle-control.spec
	@echo ""
	@echo "✓ RPM package built!"
	@echo ""
	@echo "Install with:"
	@echo "  sudo dnf install rpmbuild/RPMS/x86_64/$(PACKAGE_NAME)-$(VERSION)-1.fc41.x86_64.rpm"

help:
	@echo "$(PACKAGE_NAME) Makefile"
	@echo ""
	@echo "Targets:"
	@echo "  make              Build daemon and tray (default)"
	@echo "  make build        Build the daemon only"
	@echo "  make build-tray   Build the tray icon only"
	@echo "  make install      Install binaries and systemd services to ~/.local/bin"
	@echo "  make uninstall    Uninstall binaries and systemd services"
	@echo "  make rpm          Build RPM package for system-wide installation"
	@echo "  make clean        Clean build artifacts"
	@echo "  make help         Show this help message"
	@echo ""
	@echo "Installation methods:"
	@echo "  1. User install (recommended):  make install"
	@echo "  2. System-wide via RPM:         make rpm && sudo dnf install ..."
	@echo ""
	@echo "Variables:"
	@echo "  PREFIX       Installation prefix (default: ~/.local)"
	@echo "  BINDIR       Binary installation directory (default: \$$PREFIX/bin)"
