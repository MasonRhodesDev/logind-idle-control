# DESTDIR/PREFIX-correct system-wide install. The canonical install path is
# the distro package (packaging/PKGBUILD, packaging/logind-idle-control.spec);
# `sudo make install` exists as the dev-install fallback and lays down the
# exact same payload: binaries in $(PREFIX)/bin, user units in
# $(PREFIX)/lib/systemd/user, icons in $(PREFIX)/share/icons. It never
# touches the running user session — enable/restart units yourself.
#
# RPMs are built by packaging/build-srpm.sh (vendored, offline-safe), not here.

.PHONY: all build build-tray install uninstall clean help

PREFIX ?= /usr/local
DESTDIR ?=
BINDIR = $(PREFIX)/bin
# systemd also searches $(PREFIX)/lib/systemd/user for /usr/local installs.
UNITDIR = $(PREFIX)/lib/systemd/user
ICONDIR = $(PREFIX)/share/icons/hicolor/scalable/status

VERSION = $(shell grep '^version' Cargo.toml | head -1 | cut -d'"' -f2)
PACKAGE_NAME = logind-idle-control
BINARY = target/release/$(PACKAGE_NAME)
TRAY_BINARY = target/release/$(PACKAGE_NAME)-tray

all: build-tray

build:
	cargo build --release

build-tray:
	cargo build --release --features tray

install:
	install -Dm755 $(BINARY) $(DESTDIR)$(BINDIR)/$(PACKAGE_NAME)
	install -Dm755 $(TRAY_BINARY) $(DESTDIR)$(BINDIR)/$(PACKAGE_NAME)-tray
	install -Dm644 dist/logind-idle-control.service $(DESTDIR)$(UNITDIR)/logind-idle-control.service
	install -Dm644 dist/logind-idle-control-tray.service $(DESTDIR)$(UNITDIR)/logind-idle-control-tray.service
	install -Dm644 icons/caffeine-cup-full-symbolic.svg $(DESTDIR)$(ICONDIR)/caffeine-cup-full-symbolic.svg
	install -Dm644 icons/caffeine-cup-empty-symbolic.svg $(DESTDIR)$(ICONDIR)/caffeine-cup-empty-symbolic.svg

uninstall:
	rm -f $(DESTDIR)$(BINDIR)/$(PACKAGE_NAME)
	rm -f $(DESTDIR)$(BINDIR)/$(PACKAGE_NAME)-tray
	rm -f $(DESTDIR)$(UNITDIR)/logind-idle-control.service
	rm -f $(DESTDIR)$(UNITDIR)/logind-idle-control-tray.service
	rm -f $(DESTDIR)$(ICONDIR)/caffeine-cup-full-symbolic.svg
	rm -f $(DESTDIR)$(ICONDIR)/caffeine-cup-empty-symbolic.svg

clean:
	cargo clean

help:
	@echo "$(PACKAGE_NAME) v$(VERSION)"
	@echo ""
	@echo "Targets:"
	@echo "  make               Build daemon + tray (default)"
	@echo "  make build         Build the daemon only"
	@echo "  make build-tray    Build daemon + tray"
	@echo "  make install       Install system-wide (dev fallback; respects DESTDIR/PREFIX)"
	@echo "  make uninstall     Remove installed files"
	@echo "  make clean         Clean build artifacts"
	@echo ""
	@echo "Variables: PREFIX (default /usr/local), DESTDIR"
	@echo ""
	@echo "Prefer the distro packages: see README 'Installation'."
	@echo "After install: systemctl --user enable --now logind-idle-control.service"
