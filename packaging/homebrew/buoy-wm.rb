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
# The canonical copy is packaging/homebrew/buoy-wm.rb in buoy-wm; the tap
# nkbooth/homebrew-buoy holds it at Formula/buoy-wm.rb with only `sha256`
# changed. The url is the tag archive and the archive contains the
# canonical copy, so a real checksum there would change the archive it
# describes: the canonical copy keeps the all-zeros placeholder, and
# docs/PACKAGING.md's release runbook step 5 writes the real one into the
# tap after each final tag.

class BuoyWm < Formula
  desc "Keyboard-driven window manager for the river Wayland compositor"
  homepage "https://github.com/nkbooth/buoy-wm"
  url "https://github.com/nkbooth/buoy-wm/archive/refs/tags/v1.0.0-rc.3.tar.gz"
  sha256 "0000000000000000000000000000000000000000000000000000000000000000"
  license "RPL-1.5"
  head "https://github.com/nkbooth/buoy-wm.git", branch: "main"

  depends_on "rust" => :build
  # river is Linux-only, so anything managing its windows is too.
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
        foot         default terminal (swappable in the config file)
        fuzzel       drives the tag pickers and cheat-sheet
        zellij       default multiplexer in each tag's pinned terminal,
                     not required: tmux via pinned_terminal_args, or
                     pinned_terminals = false for none
        waybar       renders the current-tag indicator (optional)

      This formula installs the binaries only: the buoy login entry, the
      session supervisor, the user target and the portal config ship in
      the RPM and the .deb. Start a session from ~/.config/river/init (see
      CONTRIBUTING.md, Running a checkout as your session) with:
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

    %w[buoy-wm buoy-tag-picker buoy-status-bar].each do |name|
      output = shell_output("#{libexec}/bin/#{name} --version")
      # A --HEAD install's version is HEAD-<sha>; the binary prints Cargo's.
      if version.head?
        assert_match(/\A#{name} \S+\n\z/, output)
      else
        assert_equal "#{name} #{version}\n", output
      end
    end
  end
end
