Name:           logind-idle-control
Version:        0.1.0
Release:        1%{?dist}
Summary:        Systemd-logind idle inhibitor control with D-Bus interface

License:        MIT
URL:            https://github.com/MasonRhodesDev/logind-idle-control
Source0:        %{name}-%{version}.tar.gz

BuildRequires:  cargo
BuildRequires:  rust
BuildRequires:  systemd-rpm-macros

Requires:       systemd

%description
A lightweight Rust daemon for managing systemd-logind idle inhibitor locks
with per-session D-Bus event system. Provides reactive control over screen
idle behavior with zero-polling architecture.

%prep
%autosetup

%build
cargo build --release

%install
# Install binary
install -D -m 755 target/release/%{name} %{buildroot}%{_bindir}/%{name}

# Install systemd user service
install -D -m 644 systemd/%{name}.service %{buildroot}%{_userunitdir}/%{name}.service

# Install documentation
install -D -m 644 README.md %{buildroot}%{_docdir}/%{name}/README.md
install -D -m 644 LICENSE %{buildroot}%{_docdir}/%{name}/LICENSE

%post
# Enable user service for all users on first install
if [ $1 -eq 1 ]; then
    echo "Run 'systemctl --user enable --now logind-idle-control.service' to start the daemon"
fi

%preun
# Stop service on uninstall (not upgrade)
if [ $1 -eq 0 ]; then
    systemctl --user --global disable logind-idle-control.service 2>/dev/null || :
fi

%files
%license LICENSE
%doc README.md
%{_bindir}/%{name}
%{_userunitdir}/%{name}.service
%{_docdir}/%{name}/README.md
%{_docdir}/%{name}/LICENSE

%changelog
* Thu Oct 23 2025 Mason Rhodes <mason@masonrhodes.dev> - 0.1.0-1
- Initial RPM package
- Single binary with daemon/ctl modes
- Event-driven waybar integration
- Per-session D-Bus interface
- Lock/Unlock signal support
