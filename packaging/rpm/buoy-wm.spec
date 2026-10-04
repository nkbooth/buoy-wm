# SPDX-FileCopyrightText: © 2026 Nick Booth
# SPDX-License-Identifier: RPL-1.5
#
# Unless explicitly acquired and licensed from Licensor under another
# license, the contents of this file are subject to the Reciprocal Public
# License ("RPL") Version 1.5, or subsequent versions as allowed by the
# RPL, and You may not copy or use this file in either source code or
# executable form, except in compliance with the terms and conditions of
# the RPL.
#
# All software distributed under the RPL is provided strictly on an "AS
# IS" basis, WITHOUT WARRANTY OF ANY KIND, EITHER EXPRESS OR IMPLIED, AND
# LICENSOR HEREBY DISCLAIMS ALL SUCH WARRANTIES, INCLUDING WITHOUT
# LIMITATION, ANY WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR
# PURPOSE, QUIET ENJOYMENT, OR NON-INFRINGEMENT. See the RPL for specific
# language governing rights and limitations under the RPL.
#
# Built for COPR, which has no network access during builds — hence the
# vendored-crates tarball as Source1. See docs/PACKAGING.md for how to
# produce it.

Name:           buoy-wm
Version:        0.1.0
Release:        1%{?dist}
Summary:        Keyboard-driven window manager for the river Wayland compositor

# Reciprocal Public License 1.5. Note this is not on Fedora's reviewed
# allowed-licenses list, so this package targets COPR rather than Fedora
# proper; see docs/PACKAGING.md.
License:        RPL-1.5
URL:            https://github.com/nkbooth/buoy-wm
Source0:        %{url}/archive/refs/tags/v%{version}/%{name}-%{version}.tar.gz
Source1:        %{name}-%{version}-vendor.tar.gz
Source2:        buoy.desktop

ExclusiveArch:  x86_64 aarch64

BuildRequires:  cargo
BuildRequires:  rust >= 1.88
BuildRequires:  gcc
BuildRequires:  pkgconfig(wayland-client)

# Every runtime program is a Recommends, not a Requires, deliberately.
# river 0.4 (the version speaking river-window-management-v1) is not in
# Fedora at the time of writing, and a hard Requires on a `river` that
# resolves to river-classic 0.3 would make this package both uninstallable
# and wrong. The README lists the same set for users installing by hand.
Recommends:     river >= 0.4
Recommends:     foot
Recommends:     zellij
Recommends:     fuzzel
Suggests:       waybar

%description
buoy manages windows for the river Wayland compositor (0.4+) via
river-window-management-v1. It owns all tag state itself and gives each tag
a persistent fullscreen zellij terminal pinned behind every other window,
with graphical applications floating above it. Every operation is bound to
the keyboard and every binding is rebindable through one TOML file.

Ships three binaries: the window manager, a fuzzel-driven tag picker, and a
waybar buoy-status-bar feeder.

%prep
%autosetup -n %{name}-%{version}
tar -xzf %{SOURCE1}
mkdir -p .cargo
cat > .cargo/config.toml <<'EOF'
[source.crates-io]
replace-with = "vendored-sources"

[source.vendored-sources]
directory = "vendor"
EOF

%build
cargo build --workspace --release --offline --locked

%install
# All three binaries in one directory: the WM resolves buoy-tag-picker as a
# sibling of its own executable (current_exe), so splitting them breaks the
# pickers at runtime rather than at install time. Only buoy-wm is exposed
# on PATH — `buoy-tag-picker` and `buoy-status-bar` are too generic to own those names
# system-wide, and nothing resolves them through PATH anyway.
install -d %{buildroot}%{_libexecdir}/%{name}
install -m 0755 -t %{buildroot}%{_libexecdir}/%{name} \
    target/release/buoy-wm \
    target/release/buoy-tag-picker \
    target/release/buoy-status-bar

install -d %{buildroot}%{_bindir}
ln -s %{_libexecdir}/%{name}/buoy-wm %{buildroot}%{_bindir}/buoy-wm

install -Dpm 0644 %{SOURCE2} \
    %{buildroot}%{_datadir}/wayland-sessions/buoy.desktop

install -Dpm 0644 docs/config.example.toml \
    %{buildroot}%{_datadir}/%{name}/config.example.toml

%check
cargo test --workspace --release --offline --locked

%files
%license LICENSE.md
%doc README.md NOTICE.md
%{_bindir}/buoy-wm
%dir %{_libexecdir}/%{name}
%{_libexecdir}/%{name}/buoy-wm
%{_libexecdir}/%{name}/buoy-tag-picker
%{_libexecdir}/%{name}/buoy-status-bar
%{_datadir}/wayland-sessions/buoy.desktop
%dir %{_datadir}/%{name}
%{_datadir}/%{name}/config.example.toml

%changelog
* Thu Aug 20 2026 Nick Booth <n1cck@onlyhams.radio> - 0.1.0-1
- Initial package
