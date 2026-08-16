# RPM spec for logind-idle-control. Built in COPR from a local SRPM
# produced by packaging/build-srpm.sh (source tarball from the git tag +
# vendored cargo deps as Source1 — no rust-*-devel packages needed).
# Builds with the tray feature so both binaries ship.
# The test suite runs by default; disable for a one-off build with
# --without check. COPR builds run the suite.
%bcond_without check

Name:           logind-idle-control
Version:        0.2.2
Release:        1%{?dist}
Summary:        Per-session idle inhibitor control for systemd-logind
License:        MIT
URL:            https://github.com/MasonRhodesDev/logind-idle-control
Source0:        %{url}/archive/v%{version}/%{name}-%{version}.tar.gz
Source1:        %{name}-%{version}-vendor.tar.xz

BuildRequires:  cargo-rpm-macros >= 24
BuildRequires:  systemd-rpm-macros
Requires:       systemd
Requires:       hicolor-icon-theme
%{?systemd_requires}

%description
A lightweight Rust daemon for managing systemd-logind idle inhibitor locks
with a per-session D-Bus event system. Provides reactive control over screen
idle behavior with a zero-polling architecture, a CLI
(enable/disable/toggle/status/monitor), and an optional StatusNotifierItem
tray icon. Both user units are shipped; enable them with
`systemctl --user enable --now logind-idle-control.service
logind-idle-control-tray.service`.

%prep
# -a1 unpacks the vendor tarball (vendor/ at its root) into the source dir.
%autosetup -p1 -a1
%cargo_prep -v vendor

%build
%cargo_build -f tray
%{cargo_license_summary -f tray}
%{cargo_license -f tray} > LICENSE.dependencies

%install
# %%cargo_install re-resolves without Cargo.lock; git pins then fail offline.
# %%cargo_build already produced the rpm-profile binaries.
install -Dpm0755 target/rpm/logind-idle-control %{buildroot}%{_bindir}/logind-idle-control
install -Dpm0755 target/rpm/logind-idle-control-tray %{buildroot}%{_bindir}/logind-idle-control-tray
install -Dpm0644 dist/logind-idle-control.service %{buildroot}%{_userunitdir}/logind-idle-control.service
install -Dpm0644 dist/logind-idle-control-tray.service %{buildroot}%{_userunitdir}/logind-idle-control-tray.service
install -Dpm0644 icons/caffeine-cup-full-symbolic.svg %{buildroot}%{_datadir}/icons/hicolor/scalable/status/caffeine-cup-full-symbolic.svg
install -Dpm0644 icons/caffeine-cup-empty-symbolic.svg %{buildroot}%{_datadir}/icons/hicolor/scalable/status/caffeine-cup-empty-symbolic.svg

%if %{with check}
%check
%cargo_test -f tray
%endif

%post
%systemd_user_post logind-idle-control.service
%systemd_user_post logind-idle-control-tray.service

%preun
%systemd_user_preun logind-idle-control.service
%systemd_user_preun logind-idle-control-tray.service

%postun
%systemd_user_postun_with_restart logind-idle-control.service
%systemd_user_postun_with_restart logind-idle-control-tray.service

%files
%license LICENSE LICENSE.dependencies
%doc README.md
%{_bindir}/logind-idle-control
%{_bindir}/logind-idle-control-tray
%{_userunitdir}/logind-idle-control.service
%{_userunitdir}/logind-idle-control-tray.service
%{_datadir}/icons/hicolor/scalable/status/caffeine-cup-full-symbolic.svg
%{_datadir}/icons/hicolor/scalable/status/caffeine-cup-empty-symbolic.svg

%changelog
* Sun Aug 16 2026 Mason Rhodes <mrhodesdev@gmail.com> - 0.2.2-1
- Pin hypr-paths and hypr-logind to crates.io 0.1.0.

* Tue Jul 14 2026 Mason Rhodes <mrhodesdev@gmail.com> - 0.2.1-1
- Drop unpackaged cargo-registry files installed by F44 rust macros

* Thu Jul 02 2026 Mason Rhodes <mrhodesdev@gmail.com> - 0.2.0-1
- Packaged install only: binaries in /usr/bin, user units in
  /usr/lib/systemd/user, tray icons in hicolor scalable/status
- Build with the tray feature; ship both the daemon and the SNI tray
- Vendored-cargo SRPM build (offline COPR) via packaging/build-srpm.sh

* Thu Oct 23 2025 Mason Rhodes <mason@masonrhodes.dev> - 0.1.0-1
- Initial RPM package
- Single binary with daemon/ctl modes
- Native system tray icon (SNI)
- Per-session D-Bus interface
- Lock/Unlock signal support
