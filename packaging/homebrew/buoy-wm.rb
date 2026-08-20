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
# Canonical copy. The published tap (nkbooth/homebrew-buoy) holds a copy of
# this file at Formula/buoy-wm.rb; `sha256` and `url` are the only lines
# that change per release. See docs/PACKAGING.md.

class BuoyWm < Formula
  desc "Keyboard-driven window manager for the river Wayland compositor"
  homepage "https://github.com/nkbooth/buoy-wm"
  url "https://github.com/nkbooth/buoy-wm/archive/refs/tags/v0.1.0.tar.gz"
  sha256 "0000000000000000000000000000000000000000000000000000000000000000"
  license "RPL-1.5"
  head "https://github.com/nkbooth/buoy-wm.git", branch: "main"

  depends_on "pkgconf" => :build
  depends_on "rust" => :build
  # Provides libwayland-client, which wayland-client/wayland-backend link.
  depends_on "wayland"
  # river is Linux-only, so anything managing its windows is too. Homebrew's
  # own `wayland` formula carries the same requirement.
  depends_on :linux

  def install
    # Into libexec, not bin: `buoy-tag-picker` and `buoy-status-bar` are generic
    # enough names to collide with unrelated tools, and nothing resolves
    # them through PATH. The WM finds buoy-tag-picker as a sibling of its own
    # executable, so all three must land in one directory.
    %w[wm buoy-tag-picker buoy-status-bar].each do |member|
      system "cargo", "install", *std_cargo_args(path: member, root: libexec)
    end

    # current_exe() reads /proc/self/exe, which resolves this symlink to the
    # real libexec path — so sibling lookup still works when launched from
    # PATH.
    bin.install_symlink libexec/"bin/buoy-wm"

    doc.install "README.md", "NOTICE.md"
    (pkgshare/"examples").install "docs/config.example.toml"
  end

  def caveats
    <<~EOS
      buoy-wm manages windows for the river compositor; it does not install
      river itself, nor the programs it drives. Provide these yourself:

        river 0.4+   the compositor (required)
        foot         default terminal (required)
        zellij       runs in each tag's pinned terminal (required)
        fuzzel       drives the tag pickers and cheat-sheet (required)
        waybar       renders the current-tag indicator (optional)

      Start a session with:
        river -c #{opt_bin}/buoy-wm

      An example config is at:
        #{opt_pkgshare}/examples/config.example.toml
      Copy it to ~/.config/buoy/config.toml to edit the defaults.
    EOS
  end

  test do
    # buoy-tag-picker must be a sibling of buoy-wm or the pickers break at
    # runtime with no build-time signal.
    assert_path_exists libexec/"bin/buoy-tag-picker"
    assert_path_exists libexec/"bin/buoy-status-bar"

    # Both helpers validate their arguments before touching the IPC socket,
    # so this exercises real code without needing a compositor.
    assert_match "usage: buoy-status-bar",
                 shell_output("#{libexec}/bin/buoy-status-bar 2>&1", 1)
    assert_match "usage: buoy-tag-picker",
                 shell_output("#{libexec}/bin/buoy-tag-picker a b c d 2>&1", 1)
  end
end
