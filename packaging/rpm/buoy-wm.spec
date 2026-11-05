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

# The git tag archive unpacks as %%{name}-%%{upstream_version}/ and the tag
# keeps its hyphen; rpm Version cannot carry one (ADR-012: 1.0.0-rc.1 is
# packaged as 1.0.0~rc.1, which sorts before 1.0.0), so Source0 and
# %%autosetup use the upstream form and Version the tilde form.
%global upstream_version 1.0.0-rc.1

Name:           buoy-wm
Version:        1.0.0~rc.1
Release:        1%{?dist}
Summary:        Keyboard-driven window manager for the river Wayland compositor

# Reciprocal Public License 1.5. Note this is not on Fedora's reviewed
# allowed-licenses list, so this package targets COPR rather than Fedora
# proper; see docs/PACKAGING.md.
License:        RPL-1.5
URL:            https://github.com/nkbooth/buoy-wm
Source0:        %{url}/archive/refs/tags/v%{upstream_version}/%{name}-%{upstream_version}.tar.gz
Source1:        %{name}-%{upstream_version}-vendor.tar.gz

ExclusiveArch:  x86_64 aarch64

BuildRequires:  cargo
BuildRequires:  rust >= 1.99
BuildRequires:  gcc
BuildRequires:  pkgconfig(wayland-client)
# %%{_userunitdir} (escaped: rpm expands macros even in comments). The
# file triggers that reload the user manager when a unit is installed or
# removed are in the systemd package itself, which is Required below.
BuildRequires:  systemd-rpm-macros

# river is a hard dependency: the session entry runs `river -c` and the
# package is meaningless without it. Fedora 44 and 45 ship river 0.5.0 in
# the archive (0.4 is the first release speaking
# river-window-management-v1). The supervisor calls systemctl and
# dbus-update-activation-environment on every login, so their packages are
# required too. Everything else the WM drives is a Recommends: each can be
# swapped through the config file, and the README lists the same set for
# users installing by hand.
Requires:       river >= 0.4
Requires:       systemd
Requires:       dbus-tools
Recommends:     foot
Recommends:     zellij
Recommends:     fuzzel
# The two backends buoy-portals.conf names. Without them the portal only
# logs "Requested backend wlr does not exist" at info level and screen
# sharing is silently absent.
Recommends:     xdg-desktop-portal-wlr
Recommends:     xdg-desktop-portal-gtk
Suggests:       waybar

%description
buoy manages windows for the river Wayland compositor (0.4+) via
river-window-management-v1. It owns all tag state itself and gives each tag
a persistent fullscreen zellij terminal pinned behind every other window,
with graphical applications floating above it. Every operation is bound to
the keyboard and every binding is rebindable through one TOML file.

Ships three binaries: the window manager, a fuzzel-driven tag picker, and a
waybar buoy-status-bar feeder.

Also installs a `buoy` session for the display manager: river starts a
supervisor that imports the session into systemd and D-Bus, keeps
buoy-session.target up exactly as long as the window manager runs, and
routes screen capture through the wlroots portal backend.

%prep
%autosetup -n %{name}-%{upstream_version}
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
# The supervisor sits beside the binaries because it resolves buoy-wm as
# its own sibling (ADR-010); it is deliberately not on PATH.
install -Dpm 0755 packaging/buoy-session %{buildroot}%{_libexecdir}/%{name}/buoy-session

install -d %{buildroot}%{_bindir}
ln -s %{_libexecdir}/%{name}/buoy-wm %{buildroot}%{_bindir}/buoy-wm

# The session files come from the unpacked source tree rather than
# separate Source files, so the spec has one input besides the vendored
# crates and release.yml has one archive to produce.
install -Dpm 0644 packaging/buoy.desktop %{buildroot}%{_datadir}/wayland-sessions/buoy.desktop
install -Dpm 0644 packaging/buoy-session.target %{buildroot}%{_userunitdir}/buoy-session.target
install -Dpm 0644 packaging/buoy-portals.conf %{buildroot}%{_datadir}/xdg-desktop-portal/buoy-portals.conf

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
%{_libexecdir}/%{name}/buoy-session
%{_datadir}/wayland-sessions/buoy.desktop
%{_userunitdir}/buoy-session.target
# Co-owned with xdg-desktop-portal, as sway does: that package is only a
# Recommends away, so the directory would otherwise be unowned.
%dir %{_datadir}/xdg-desktop-portal
%{_datadir}/xdg-desktop-portal/buoy-portals.conf
%dir %{_datadir}/%{name}
%{_datadir}/%{name}/config.example.toml

%changelog
* Thu Nov 05 2026 Nick Booth <n1cck@onlyhams.radio> - 1.0.0~rc.1-1
- First release candidate: the packaged buoy session (supervisor, user
  target, portal routing) and the RC version mapping

* Thu Aug 20 2026 Nick Booth <n1cck@onlyhams.radio> - 0.1.0-1
- Initial package
