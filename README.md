# buoy

A keyboard-driven window manager for the [river](https://codeberg.org/river/river)
Wayland compositor (0.4+), built around a terminal you never have to open.

`buoy` speaks `river-window-management-v1`, which means river handles pixels
and input while `buoy` decides window policy. It owns **all** tag state
itself — the compositor keeps none — so the model below is entirely `buoy`'s
to define.

It has been the author's daily driver since August 2026.

## The idea

**Terminal first.** Every tag has a permanent, fullscreen terminal running a
multiplexer session pinned behind everything else — [`zellij`](https://zellij.dev)
by default, though `tmux` or any command that attaches to a named session
works in its place. It is not a window you launch, focus, or close; it is
the tag's floor. Switch to the `email` tag and its session is simply
*there*, exactly as you left it. Graphical applications float on top.

**Keyboard primary.** Every operation has a binding, all of them rebindable
and removable; the mouse moves and resizes floating windows and is required
for nothing else. `Super+Shift+?` shows the bindings actually in effect.

What `buoy` deliberately does not do: automatic tiling, window decorations,
gaps, animations, or a session/state file. Floating windows are placed once
and then left alone.

## Supported platforms

| Release | Tier | river 0.4+ | Session multiplexer | Download |
| --- | --- | --- | --- | --- |
| Fedora 44 | 1 | 0.5.0 in the archive; the RPM requires it | zellij is not packaged: the upstream asset or `cargo install --locked zellij`, either in `/usr/local/bin`, or `tmux` from the archive | `buoy-wm-${version}-1.fc44.x86_64.rpm` |
| Fedora 45 | 1 | 0.5.0 in the archive; the RPM requires it | as Fedora 44 | `buoy-wm-${version}-1.fc45.x86_64.rpm` |
| Ubuntu 26.04 LTS | 1 | not in the archive: `ppa:tchavadar/river` (0.4.1) or a source build; screen sharing is monitor-only on 0.4.1 | zellij is not packaged: the upstream asset or `cargo install --locked zellij`, either in `/usr/local/bin`, or `tmux` from the archive | `buoy-wm_${version}-1.ubuntu26.04_amd64.deb` |
| Ubuntu 26.10 | 1 | not in the archive: `ppa:tchavadar/river-unstable` (0.4.5) or a source build | as Ubuntu 26.04 | `buoy-wm_${version}-1.ubuntu26.10_amd64.deb` |
| Ubuntu 24.04 LTS and older | unsupported | no river from any apt source | — | the tarball may run if you build river yourself; nothing here is verified on it |

**Tier 1** means the release is session-verified on a clean machine before
every final release: install as the README says, pick `buoy` at the display
manager, get portals, bar and outputs, log out cleanly. **Tier 2** would mean
the package builds, installs and passes its tests in CI with no session
verified; no release is in tier 2 today. A supported release leaves the table
when it leaves standard support. Every release asset is x86_64; other
architectures build from source.

Ubuntu ships no `river` in any release. The two PPAs above are one person's
unofficial archives — Launchpad's own banner calls them "unsupported
packages from this untrusted PPA" — built for amd64 only; the stable PPA
carries river only for 26.04 and the unstable one only for 26.10. The
request to put river in the Ubuntu archive is
[LP #2142921](https://bugs.launchpad.net/ubuntu/+bug/2142921), open at
Wishlist importance and waiting on packaging review as of 2026-11-07.
Building river from source (river's README names the zig release it
needs) is the other way. Ubuntu 24.04 LTS is unsupported because no apt source
provides river for it, its xdg-desktop-portal is 1.18 and its rustc 1.75.

A session multiplexer is a default, not a dependency, and no release above
packages zellij. [Session multiplexer](#session-multiplexer) shows how to
install it, how to use `tmux` from the archive instead, or how to run with no
multiplexer and no pinned terminal at all.

## Install

Install the package for your release, then give each tag's pinned terminal
a [session multiplexer](#session-multiplexer).

### Requirements

| | |
| --- | --- |
| **river** | 0.4 or newer, built with `river-window-management-v1`. Fedora 44 and 45 ship 0.5.0; the RPM requires it. Ubuntu: see [Supported platforms](#supported-platforms) |
| **foot** | default terminal — swappable, see [Programs](#programs) |
| **fuzzel** | drives the tag pickers and the hotkey cheat-sheet — fixed, see [Programs](#programs) |
| **zellij** | the *default* multiplexer in each tag's pinned terminal, not a requirement — see [Session multiplexer](#session-multiplexer) |
| **waybar** | *optional* — renders the current-tag indicator. Fedora's and Ubuntu's packages ship `waybar.service`, which is how to start it under the packaged session, see [First login](#first-login) |
| **xdg-desktop-portal-wlr**, **xdg-desktop-portal-gtk** | *optional* — the packaged session sends ScreenCast and Screenshot to wlr and everything else to gtk; without them screen sharing is silently absent. The RPM recommends both |
| **systemd** (user manager), **dbus-update-activation-environment** (`dbus-tools`) | what the packaged session's supervisor calls on every login; present on every Fedora and Ubuntu, listed so this table is the whole set |

`river`'s `river_input_manager_v1` and `river_libinput_config_v1` globals are
also optional: without them, `[[input]]` blocks are skipped with one log line
and every device keeps libinput's own defaults.

### Fedora 44 and 45: the RPM

Each Fedora release has its own RPM, built with that release's toolchain;
the `rpm=` line picks the one for the release you are running.

```sh
version=1.0.0-rc.3
rpm="buoy-wm-${version}-1.fc$(rpm -E %fedora).x86_64.rpm"
base="https://github.com/nkbooth/buoy-wm/releases/download/v${version}"

workdir="$(mktemp -d)"
curl --proto '=https' --tlsv1.2 -fsSL -o "$workdir/$rpm" "$base/$rpm"
curl --proto '=https' --tlsv1.2 -fsSL -o "$workdir/$rpm.sha256" "$base/$rpm.sha256"
# Optional and stronger, with the GitHub CLI installed: proves this exact
# file came out of this repo's release workflow. See SECURITY.md.
# gh attestation verify "$workdir/$rpm" --repo nkbooth/buoy-wm

(cd "$workdir" && sha256sum -c "$rpm.sha256") &&
sudo dnf install "$workdir/$rpm"
# Silverblue, Kinoite and other atomic hosts: this line instead of the
# dnf one, then reboot.
# sudo rpm-ostree install "$workdir/$rpm"
rm -rf "$workdir"

# Optional, for the status bar (atomic hosts: sudo rpm-ostree install waybar):
sudo dnf install waybar
```

`rpm-ostree install` also takes the `https://` URL directly, as RPM Fusion's
own instructions do; the checksum is verified first here for the same
reason the tarball's is below. dnf resolves `river`, `systemd` and
`dbus-tools` as hard dependencies and, as weak ones, the two portal backends
(which bring `xdg-desktop-portal` itself), `foot` and `fuzzel` — not
`zellij`, see [Session multiplexer](#session-multiplexer).

The package installs the three binaries and
the session supervisor as `/usr/libexec/buoy-wm/{buoy-wm,buoy-tag-picker,buoy-status-bar,buoy-session}`,
a `/usr/bin/buoy-wm` symlink, the login entry
`/usr/share/wayland-sessions/buoy.desktop`, the user target
`/usr/lib/systemd/user/buoy-session.target`, the portal routing
`/usr/share/xdg-desktop-portal/buoy-portals.conf`,
`/usr/share/buoy-wm/config.example.toml` and the man pages `buoy-wm(1)` and
`buoy-config(5)`. `rpm -qp` reports the version as
`1.0.0~rc.3` while the file is named with the tag's hyphen: rpm cannot
carry a hyphen in a version and GitHub rewrites `~` in asset names. The same
release carries `buoy-wm-1.0.0-rc.3-vendor.tar.gz`, the vendored crates for
rebuilding the RPM offline (`docs/PACKAGING.md`).

### Ubuntu 26.04 and 26.10: river from a PPA, then the .deb

Neither release has `river` in its archive, so add the PPA for your release
first — [Supported platforms](#supported-platforms) says what these PPAs
are, and they build for amd64 only — then install river and the programs
`buoy` drives. One `add-apt-repository` line, not both: each PPA carries
river for one release.

```sh
sudo add-apt-repository ppa:tchavadar/river
# Ubuntu 26.10: this line instead of the one above.
# sudo add-apt-repository ppa:tchavadar/river-unstable

sudo apt update
sudo apt install river foot fuzzel tmux xdg-desktop-portal-wlr xdg-desktop-portal-gtk
# Optional, for the status bar:
sudo apt install waybar
```

`tmux` is on that line because no Ubuntu archive has zellij. Either get
zellij as [Session multiplexer](#session-multiplexer) says and leave the
default alone, or put the `tmux` `pinned_terminal_args` block from that
section in `~/.config/buoy/config.toml` and `chmod 600` it
([why](#when-something-in-the-file-is-wrong)).

Then install `buoy` itself from the `.deb` built for your release:

```sh
version=1.0.0-rc.3
deb="buoy-wm_${version}-1.ubuntu$(. /etc/os-release && echo "$VERSION_ID")_amd64.deb"
base="https://github.com/nkbooth/buoy-wm/releases/download/v${version}"

workdir="$(mktemp -d)"
curl --proto '=https' --tlsv1.2 -fsSL -o "$workdir/$deb" "$base/$deb"
curl --proto '=https' --tlsv1.2 -fsSL -o "$workdir/$deb.sha256" "$base/$deb.sha256"
# Optional and stronger, with the GitHub CLI installed: proves this exact
# file came out of this repo's release workflow. See SECURITY.md.
# gh attestation verify "$workdir/$deb" --repo nkbooth/buoy-wm

(cd "$workdir" && sha256sum -c "$deb.sha256") &&
sudo apt install "$workdir/$deb"
rm -rf "$workdir"
```

The `.deb` installs the same files as the RPM, listed in the Fedora section
above, including the `buoy` login entry. apt prints a `Notice: Download is
performed unsandboxed as root` line for it, because `mktemp -d` makes a
directory only you can read and apt's `_apt` user cannot look inside; the
install is unaffected.

### From a release

Run the `version=` line of the install block for your release first; the
archive name is built from it.

```sh
archive="buoy-wm-${version}-x86_64-unknown-linux-gnu.tar.gz"
base="https://github.com/nkbooth/buoy-wm/releases/download/v${version}"

workdir="$(mktemp -d)"
curl --proto '=https' --tlsv1.2 -fsSL -o "$workdir/$archive" "$base/$archive"
curl --proto '=https' --tlsv1.2 -fsSL -o "$workdir/$archive.sha256" "$base/$archive.sha256"
# Optional and stronger, with the GitHub CLI installed: proves this exact
# archive came out of this repo's release workflow. See SECURITY.md.
# gh attestation verify "$workdir/$archive" --repo nkbooth/buoy-wm

(cd "$workdir" && sha256sum -c "$archive.sha256") &&
tar -xzf "$workdir/$archive" -C "$workdir" &&
"$workdir/buoy-wm-${version}-x86_64-unknown-linux-gnu/install.sh"
rm -rf "$workdir"
```

Longer than a one-line pipe on purpose. `mktemp -d` rather than `/tmp`,
because `/tmp` is world-writable and sticky; an exact path rather than a
`/tmp/buoy-wm-*/install.sh` glob, because another local user can pre-create
a directory that sorts first and have the shell run *their* `install.sh` as
you; a pinned version rather than `releases/latest/download/...`, because
that URL is a moving target; and the checksum is verified because CI
publishes it and nothing was checking it.

The tarball carries no man pages; their sources are `docs/man/*.scd` in the
repository, and `scdoc < docs/man/buoy-config.5.scd | man -l -` renders one
from a checkout.

### From source

Rust 1.93 or newer and `libwayland-dev` are the only build dependencies — the
floor is the oldest supported release's rustc, as `docs/PACKAGING.md`
"Toolchain floor" explains.

```sh
cargo build --workspace --release
./scripts/install.sh
```

Both paths put all three binaries — `buoy-wm`, `buoy-tag-picker`, `buoy-status-bar` —
into `~/.local/lib/buoy-wm` (override with `BUOY_INSTALL_DIR`) and symlink
only `buoy-wm` onto your `PATH`. **They must stay in one directory**: the WM
locates `buoy-tag-picker` as a sibling of its own executable.

Installing outside the build tree is deliberate. A `river` session `exec`s
the WM as its session leader, so a `cargo clean` that deleted the running
binaries would cost you the session. For the same reason: **verify a new
build from a TTY or a nested river before logging out.** A broken WM binary
means a black screen and a bounce back to your display manager, with no shell
to fix it from.

Without the package there is no `buoy` login entry;
[CONTRIBUTING.md](CONTRIBUTING.md) shows how to start the session from a
checkout with a hand-written river init, and has the development gates and
the design history.

### Session multiplexer

`zellij` is a default, not a dependency: the default `pinned_terminal_args`
runs it inside each tag's pinned terminal, and the WM never calls it
otherwise. **No supported Fedora or Ubuntu release packages it**, so on
Fedora the RPM's `Recommends: zellij` is ignored by dnf. Put the upstream release asset's
binary in `/usr/local/bin`, checked against upstream's published checksum
first; the checksum names the binary, not the archive, so it is checked
after extracting:

```sh
zellij_version=0.46.0
zellij_asset="zellij-x86_64-unknown-linux-musl"
zellij_base="https://github.com/zellij-org/zellij/releases/download/v${zellij_version}"

zdir="$(mktemp -d)"
curl --proto '=https' --tlsv1.2 -fsSL -o "$zdir/$zellij_asset.tar.gz" "$zellij_base/$zellij_asset.tar.gz"
curl --proto '=https' --tlsv1.2 -fsSL -o "$zdir/$zellij_asset.sha256sum" "$zellij_base/$zellij_asset.sha256sum"
tar -xzf "$zdir/$zellij_asset.tar.gz" -C "$zdir" zellij &&
(cd "$zdir" && awk '{ print $1 "  zellij" }' "$zellij_asset.sha256sum" | sha256sum -c) &&
sudo install -m 755 "$zdir/zellij" /usr/local/bin/zellij
rm -rf "$zdir"
```

0.46.0 is the release the clean-machine checks ran; a newer one installs
the same way. `sudo install` leaves the binary owned by root; tar run as root
straight into `/usr/local/bin` would not, because it keeps the archive's
owner, uid 1001, the second account on a typical desktop.

`/usr/bin/zellij` works too. `~/.local/bin` and `~/.cargo/bin` do not: the
packaged session runs no shell profile, so its `PATH` is the systemd user
manager's (`/usr/local/bin:/usr/bin` on Fedora).

To build zellij instead, run `cargo install --locked zellij` (on Fedora,
`sudo dnf install cargo perl-core` first: its bundled OpenSSL build runs
perl), then copy the binary with `sudo install ~/.cargo/bin/zellij /usr/local/bin/`.

Or use `tmux`, which every Fedora and Ubuntu archive carries:

```toml
[defaults]
pinned_terminal_args = ["-a", "{app_id}", "tmux", "new-session", "-A", "-s", "{session}"]
```

`-A` attaches to the session if it exists and creates it otherwise, which is
what zellij's `attach --create` does. `pinned_terminals = false` drops the
multiplexer and the pinned terminal altogether.

tmux mishandles `.` and `:` in tag names, which zellij does not, in one of
two ways depending on its version. tmux 3.7 and newer (Ubuntu 26.10's 3.7b,
the 3.8 in Fedora 44 and 45's updates) accept both characters in a new
session's name but read them as target separators (`session:window.pane`)
when attaching. A tag named like `a.b` or `a:b` then gets its pinned terminal
on the first login only. While that tmux server is still running, every later
login either fails with `can't find pane` or `can't find window`, or attaches
to another tag's session whose name matches the target. tmux 3.6a (Ubuntu
26.04, and Fedora 44 before its updates) instead rewrites both characters to
`_`, so `a.b`, `a:b` and `a_b` share one session. Keep `.` and `:` out of tag
names when using tmux, or use zellij, which accepts them.

## First login

### The packaged session

Pick **buoy** at the display manager. The entry runs

```
Exec=river -c /usr/libexec/buoy-wm/buoy-session
```

so the display manager starts river, and river runs the supervisor as its
init child once the compositor socket exists. The supervisor:

- imports `WAYLAND_DISPLAY`, `DISPLAY`, `XDG_CURRENT_DESKTOP`,
  `XDG_SESSION_TYPE` and `XDG_SESSION_DESKTOP` into the systemd user manager
  and the D-Bus activation environment;
- starts `buoy-session.target`, which is bound to `graphical-session.target`.
  That target refuses a manual start, so this is how xdg-desktop-portal and
  every other unit tied to the session comes up;
- runs `buoy-wm` as a child and forwards termination signals to it;
- when `buoy-wm` exits, stops the target and exits with its status.

The entry's
`DesktopNames=buoy` selects `buoy-portals.conf`, which sends ScreenCast and
Screenshot to `xdg-desktop-portal-wlr` and everything else to gtk.

So start helpers from user units bound to `graphical-session.target`, not
from a script. For the bar (configure it first, as [Status bar](#status-bar)
shows), `systemctl --user enable
waybar.service` uses the distribution's own unit. Waybar's `-c`/`-s`
arguments, which a per-output `custom/tag` setup may need, go in a drop-in:
run `systemctl --user edit waybar.service` and add a `[Service]` section with
an empty `ExecStart=` and then yours. If you use kanshi, enable its unit the
same way.

The WM, and the pickers it spawns, log to the journal as `buoy-wm`
(`journalctl --user -b --identifier=buoy-wm`). The supervisor's own
messages are prefixed `buoy-session:` and stay in the display manager's
stream, under `journalctl --user -b _COMM=buoy-session`.

### Logging out

`Super+Escape` ends the WM, the supervisor stops `buoy-session.target` and
then `graphical-session.target` (which stops every unit bound to it), and
the session ends.

## Tags

A tag is a named workspace. It is created the moment you first name it and
lives until the session ends; there is no fixed set to configure up front and
no numbered grid to memorise. Up to 64 tags exist at once.

Three rules cover the whole model:

- **One tag is displayed per output.** Each monitor shows exactly one tag,
  and each tag can be displayed on at most one monitor at a time.
- **A window can carry several tags.** It is visible whenever any tag it
  carries is the one being displayed. This is how a chat window follows you
  between `work` and `personal` without being duplicated.
- **Every tag owns one pinned terminal.** Spawned on the tag's first use,
  held fullscreen behind every other window, and not closable with the
  ordinary close binding.

"The active output" — the target of tag switches — is whichever monitor the
pointer is currently over.

## Keybindings

The built-in bindings, spelled as the cheat-sheet shows them. A config file
can replace them ([Bindings](#bindings)). `Super` is the logo key (`Mod4`).

| Keybind | Action |
| --- | --- |
| `Super+space` | Spawn terminal (foot) |
| `Super+q` | Close focused window |
| `Super+n` | Cycle focus |
| `Super+Escape` | Exit session |
| `Super+Tab` | Cycle tag |
| `Super+a` | Tag manager (assign tags to focused window) |
| `Super+s` | Switch tag (or type a new name to create) |
| `Super+r` | App launcher (fuzzel) |
| `Super+Shift+?` | Show this hotkey list |
| `Super+LeftClick` | Move window |
| `Super+RightClick` | Resize window |

`Super+q` never closes a tag's pinned terminal. The cheat-sheet
(`Super+Shift+?`) is generated from the bindings actually loaded, so it
always reflects your config rather than this table.

### Switching and creating tags — `Super+s`

Opens a `fuzzel` list of every tag; picking one displays it on the active
output, spawning its pinned terminal if this is its first use.

Typing a name that matches nothing and confirming **creates that tag and
switches to it in one action**. Creating a tag is how you start working
somewhere new, so it lives here rather than buried in the assignment picker.
Typing the exact name of an existing tag switches to it instead of
duplicating it. At the 64-tag cap the picker reopens with the rejection shown
and your typed name restored — nothing is created, nothing is switched.

### Assigning tags to a window — `Super+a`

Opens a `fuzzel` checklist of every tag, pre-checked for the tags the focused
window already carries. Selecting a row toggles that tag and reopens the
list, so you can toggle several in one visit. Existing tags only.

## Status bar

`buoy-status-bar` is a companion binary that feeds one waybar `custom/tag` module
per output. It takes the output's numeric id as its only argument and loops
itself, so waybar needs neither `interval` nor `signal`:

```jsonc
// ~/.config/waybar/config.jsonc
[
  {
    "layer": "top",
    "modules-right": ["custom/tag"],
    "custom/tag": {
      "exec": "/usr/libexec/buoy-wm/buoy-status-bar 0",
      "return-type": "json",
      "escape": true
    }
  }
]
```

That is one bar, drawn on every monitor, for buoy's output `0`: right for a
single monitor. With more, give each bar an `"output"` (the connector name
river logs, such as `eDP-1`) and its own id, `buoy-status-bar 1` and so on.
The path is the packaged one; the tarball installs the binary as
`~/.local/lib/buoy-wm/buoy-status-bar`. Then enable the bar as
[First login](#first-login) says.

`"escape": true` is not optional. Tag names are arbitrary strings and waybar
renders module text as Pango markup with `escape` defaulting to `false`, so a
tag called `<3` produces invalid markup and blanks the label. `buoy-status-bar`
escapes its output for *JSON*, which is a different layer and does not help
here.

Output ids are `buoy`'s own, assigned in the order river announces monitors —
they are not connector names, and nothing translates between the two. Find
which is which by switching tags and watching which bar reacts. The module
emits a `normal` or `disconnected` CSS class for your stylesheet to target;
if the WM stops responding the bar degrades to `⚠ disconnected` rather than
freezing on a stale tag name, and an id that names no output shows
`⚠ unknown output`.

The bar and the pickers reach the WM over a Unix socket in
`$XDG_RUNTIME_DIR`; [CONTRIBUTING.md](CONTRIBUTING.md) documents the
protocol for scripting against it.

## Configuration

Optional, at `~/.config/buoy/config.toml` (or under `$XDG_CONFIG_HOME`). With
no config file the built-in defaults apply unchanged. To start your own file
from every default, written out and commented:

```sh
mkdir -p ~/.config/buoy
buoy-wm --print-default-config > ~/.config/buoy/config.toml
```

then make it private with the `chmod 600` below. The same output, under a
licence header, ships as [`docs/config.example.toml`](docs/config.example.toml)
and is installed as `/usr/share/buoy-wm/config.example.toml`;
`man 5 buoy-config` is the full reference for every key, action and value.
Remember that declaring any `[[keybind]]` replaces the whole built-in set, and
`[[mousebind]]` and `[[input]]` work the same way; when your lists leave a
built-in action bound to nothing, or stop applying a built-in `[[input]]`
setting such as the touchpad's `tap`, a notification at login names it.

```toml
[defaults]
terminal = "foot"      # also used for each tag's pinned terminal
launcher = "fuzzel"    # the `launcher` action only — see below
default_tag = "default"

[[keybind]]
mod = ["super"]
key = "1"
action = { switch_tag = "email" }   # created on first press

[[keybind]]
mod = ["super"]
key = "p"
action = { exec = "grim -g \"$(slurp)\" ~/shot.png" }
```

Because it declares `[[keybind]]`, this file would leave only those two key
bindings ([Bindings](#bindings)).

### When something in the file is wrong

A `[[keybind]]`, `[[mousebind]]` or `[[input]]` entry that parses but
cannot be used is **skipped**, and everything else in the file still
applies: an unknown key name, an uppercase letter bound without `shift`,
`move` or `resize` on a key, a binding written twice, and an `[[input]]`
entry with an empty or repeated `name`, no setting, or an `accel_speed`
outside `-1.0..=1.0`. Every skipped entry is reported at once, so a file with
four independent mistakes takes one edit-and-restart cycle rather than four.

Anything the parser itself refuses rejects the **whole file**: invalid TOML,
an unknown field anywhere, a value of the wrong type, a misspelled action,
modifier, button, `tap_button_map` or `click_method`, a missing `key`,
`button`, `action` or `name`, and any wrong value in `[defaults]`. A
rejected file falls back to the built-in defaults *entirely* — including the
built-in keybinds, which may be a keymap you have not used in months.
Refusing to start would leave you in a session with no way to reach the file
and fix it, so the WM starts either way. The file is read once, at login:
after an edit, log out and back in.

Three more refusals are about the file rather than its contents, and each
rejects it whole: a file that cannot be read, one larger than 1 MiB, and
anything that is not a regular file — a directory or a FIFO at that path is
reported instead of read, because a read that never returns is a login that
never completes.

This file is as trusted as `~/.bashrc`: an `{ exec = "..." }` binding runs
through `sh -c` in your session. So if it is group- or world-**writable**, or
owned by another user, buoy loads it anyway and tells you. Fedora and Ubuntu give a new file
`664` (the umask leaves your own group writable), which counts, so after
creating the file run `chmod 600 ~/.config/buoy/config.toml` once; `644` is
fine too. Who can *read* the file is not the concern.

Each of these problems, a skipped entry, a rejected file or this warning, is
logged, and raised as a desktop notification naming the file and the problem
when a notification daemon (mako, dunst, swaync) runs in the session. A fresh
install has none, so read the log:

```sh
journalctl --user -b --identifier=buoy-wm -p err
```

That shows the first line of each report. A report that runs over several
lines, such as one listing every skipped entry, carries the error severity on
its first line only, so drop `-p err` to read it whole.

[Troubleshooting](#troubleshooting) has the rest.

### Bindings

**Declaring any `[[keybind]]` replaces the entire built-in set**, so list
every binding you want. That is what makes a default rebindable *and*
removable. `[[mousebind]]` is a separate list with the same rule. An empty
list, or one whose every entry was skipped, keeps the built-ins. Binding the
same trigger twice is reported, not a silent race: the second binding is
skipped. Two bindings count as the same only when written the same way, so
`["super", "shift"]` and `["shift", "super"]`, or `Space` and `space`, are
not caught.

Keys are keysym names: a single character (`a`, `1`, `?`), a named key
(`Return`, `space`, `Tab`, `Escape`, `Left`, `Page_Up`; `man 5 buoy-config`
lists all eighteen), or `F1`–`F35`. Named
keys are case-insensitive; **single characters are not**. An uppercase letter
is the *shifted* symbol, so `Super`+`Q` can never fire — write `key = "q"`,
or add `"shift"` to `mod`. That binding is skipped at load, and reported, rather than
silently doing nothing. `mod` is optional; omit it to bind an unmodified key.

`mod` values are `super` (or `mod4`, `logo`, `win`), `ctrl` (or `control`),
`alt` (or `mod1`) and `shift`; `button` values are `left`, `right` and
`middle`. The PascalCase spellings this file used to document (`"Super"`,
`"Left"`) are still accepted, so an existing config needs no edit.

Actions: `terminal`, `launcher`, `close`, `focus_next`, `exit`, `cycle_tag`,
`open_assign_picker`, `open_switch_picker`, `hotkeys`,
`{ switch_tag = "<name>" }`, and `{ exec = "<command>" }`. The pickers were
previously spelled `tag_picker` and `tag_switch` — one transposition away
from `switch_tag`, which does something else entirely — and both old
spellings are still accepted. `exec` runs through `sh -c`, so pipes, arguments
and `~` work as they would in a shell. `move` and `resize` drive a pointer
drag and are mousebind-only; a mousebind takes any action.

### Programs

`launcher` sets what the `launcher` action spawns, and nothing else. `fuzzel`
stays hardcoded where it is driven as a *menu* rather than a launcher — the
two tag pickers and the cheat-sheet — because those pass fuzzel-specific
flags (`--dmenu`, `--layer`, `--prompt`).

A tag's pinned terminal is spawned as `terminal` plus `pinned_terminal_args`,
defaulting to foot's spelling:

```toml
[defaults]
pinned_terminal_args = ["-a", "{app_id}", "zellij", "attach", "--create", "{session}"]
```

`pinned_terminals = false` turns the feature off entirely — the supported
escape hatch if your terminal or `zellij` is broken, or if you just do not
want one. Tags, the pickers and the status bar work exactly as before; only
the per-tag terminal and its backdrop go away. Emptying `pinned_terminal_args`
is not the off switch and, unless `pinned_terminals = false` is set too, is
rejected as a mistake, since it would spawn a
terminal carrying no app id that nothing could recognise.

Set `pinned_terminal_args` alongside `terminal` if your terminal spells the app-id flag
differently — most use `--class`. `{app_id}` is **required**: it is how the
WM recognises the pinned window, and its absence is rejected at load. It is
substituted per tag, as `pinned-term-<tag id>` (for example
`pinned-term-3`), which is how a mapped window says which tag it belongs to
rather than the WM having to guess from the order spawns happen to start in.
`{session}` becomes `tag-<tag name>`, the session name for whichever
multiplexer runs there. Both are replaced inside each argument, never
through a shell.

### Input devices

Touchpads, keyboards and mice are configured with `[[input]]` blocks matched
against libinput device names:

```toml
[[input]]
name = "*Touchpad*"           # `*` is the only wildcard
tap = true                    # 1/2/3 fingers = left/right/middle
tap_button_map = "lrm"        # or "lmr" (1/2/3 = left/middle/right)
click_method = "button_areas" # or "clickfinger", "none"
natural_scroll = true
disable_while_typing = true
accel_speed = 0.3             # -1.0 (slowest) .. 1.0 (fastest)
```

The only built-in entry is `name = "*Touchpad*"` with `tap = true`, because
libinput ships tap-to-click *disabled* on any device with physical buttons —
so without it a laptop touchpad accepts motion and scrolling but ignores taps
entirely. Matching is on name rather than the protocol's device *type*, which
reports plain `pointer` for touchpads, mice and trackballs alike; forcing tap
on an external mouse is not wanted.

An omitted setting is **not** the same as `false`. Omitting one sends no
request and leaves libinput's default alone; `tap = false` actively turns tap
off. As with bindings, declaring any `[[input]]` replaces the built-in list,
so restate `tap = true` for your touchpad if you add entries for other
devices. Entries match top to bottom, first match wins — which is what lets a
specific device sit above a catch-all `name = "*"`.

Skipped at load and reported, rather than looking configured and doing
nothing: an empty `name`, an entry setting nothing, a duplicate pattern, and
an `accel_speed` outside `-1.0..=1.0`. A pattern that matches no device is
**accepted silently**, so check it against the real device names:
`libinput list-devices` (needs root) prints them, and so does
`/proc/bus/input/devices`. Configured devices log their tap state and click
method at startup and after each setting is applied; anything the device
rejects is named in the log.

## Troubleshooting

Start with the journal. This prints the first line of every failure the WM
and the pickers reported since boot:

```sh
journalctl --user -b --identifier=buoy-wm -p err
```

Drop `-p err` to see everything: the rest of a report that runs over
several lines, and the output of the terminals and applications the WM
starts, are logged at info. Replace `-b` with `-b -1` for the previous boot,
and use `journalctl --user -b _COMM=buoy-session` for the session
supervisor's own lines. On 1.0.0-rc.3 and earlier the WM's lines reach the
journal under the display manager's name and without a severity, so the
query above finds nothing there; use
`journalctl --user -b _COMM=buoy-wm --grep '^<3>'` instead.

### Black screen, then back at the login screen

The session ended as soon as it started. Switch to a text console
(`Ctrl+Alt+F3`), log in, and run the journal commands above for the failed
login, adding `--since` and its time, or using `-b -1` if you have rebooted
since. Look for:

- `river_window_manager_v1 global not found! Is river running?` or
  `river_xkb_bindings_v1 global not found! Is river running with xkb support?`:
  river is older than 0.4 or built without `river-window-management-v1`.
  Install the river [Supported platforms](#supported-platforms) names.
- `Error: Another WM is already running`: another window manager already
  holds river's `river_window_manager_v1`, and river allows one. Remove
  whatever else starts a window manager in this session.
- `buoy-session: /usr/libexec/buoy-wm/buoy-wm is not executable`, under
  `_COMM=buoy-session`: the package is damaged; reinstall it.

A bad config file never stops the WM starting, but if you want it out of
the way while you look, `mv ~/.config/buoy/config.toml ~/.config/buoy/config.toml.off`
from the console and log in again.

### Config rejected, or my bindings are gone

A notification, and the same text in the journal (without `-p err` for the
lines after the first), says either
`<path> was rejected (<reason>).` followed by
`Running with built-in default keybinds until it is fixed.`, or
`<path>: <reason>` followed by
`Those entries were skipped; everything else in the file is in effect.`
A rejected file is replaced by the built-in defaults in full, keybinds
included; [When something in the file is wrong](#when-something-in-the-file-is-wrong)
says what rejects a file and what only skips an entry. Fix what the reason
names, then log out and back in: the file is read once, at login.

### A warning that the config file's mode is 0664

`the config file's mode is 0664, so another local user can write it` means
your umask made the new file group-writable. The file was loaded anyway;
`chmod 600 ~/.config/buoy/config.toml` clears the warning from the next
login.

### No desktop notifications at all

A clean session has no notification daemon, so every notification fails
and the journal shows `A child of this WM exited unsuccessfully (exit status: 1)`
in its place. Install one: `sudo dnf install mako` on Fedora,
`sudo apt install mako-notifier` on Ubuntu. Its D-Bus activation file
starts it on the first notification; there is nothing to enable.

### Screen sharing or screenshots missing

```sh
systemctl --user is-active xdg-desktop-portal.service xdg-desktop-portal-wlr.service xdg-desktop-portal-gtk.service
journalctl --user -b --grep 'Requested backend'
```

`Requested backend wlr does not exist` (or `gtk`) means that backend is
not installed: install `xdg-desktop-portal-wlr` or `xdg-desktop-portal-gtk`
and log in again. On Ubuntu 26.04, river 0.4.1 offers whole monitors only,
never a single window ([Supported platforms](#supported-platforms)). If you
copy `buoy-portals.conf` to change it, never put `none` in one of its
lists: it switches that interface off.

### The status bar shows `⚠ disconnected`

The bar cannot reach the WM: buoy-wm is not running, or it started with
no IPC server, which a `buoy-wm started with no IPC server:` notification
reports with the reason. `$XDG_RUNTIME_DIR` must be set, absolute, and a
directory you own that no other user can reach; there is no fallback.
`⚠ unknown output` is different: the WM answered, but the number after
`buoy-status-bar` names no output. Ids are buoy's own, `0` for the first
monitor river announces.

### No pinned terminal on a tag

- ``Failed to spawn pinned terminal `foot`: No such file or directory (os error 2)``:
  the terminal is not installed. Install it or set `terminal`; the next
  visit to the tag tries again.
- `A child of this WM exited unsuccessfully (exit status: 1)` and no
  terminal: the multiplexer is missing or not on the session's `PATH`,
  which is the user manager's and never your shell's. Put it in
  `/usr/local/bin` ([Session multiplexer](#session-multiplexer)). The tag
  does not try again until the next login.
- With tmux, a tag whose name holds `.` or `:` fails on a later login with
  `can't find pane` or `can't find window`
  ([Session multiplexer](#session-multiplexer) explains why).

`pinned_terminals = false` in `[defaults]` turns pinned terminals off
while you sort it out.

### `Super+a` or `Super+s` does nothing

The pickers talk to the WM over the same socket as the bar, so a
`buoy-wm started with no IPC server:` notification explains both. If
there is no such notification, look for the picker's own: when `fuzzel` is
missing it says ``cannot run `fuzzel` `` with the reason.

## Uninstall

On Fedora, `sudo dnf remove buoy-wm`; on an atomic host,
`sudo rpm-ostree uninstall buoy-wm` and reboot. On Ubuntu,
`sudo apt remove buoy-wm`, and if nothing else needs river from the PPA,
`sudo add-apt-repository --remove ppa:tchavadar/river` (on 26.10,
`ppa:tchavadar/river-unstable`). From a release or from source,
`rm -rf ~/.local/lib/buoy-wm`, plus `rm ~/.local/bin/buoy-wm` if
`install.sh` made that link. `~/.config/buoy/` is yours; delete it if you
are done with it.

## License

[Reciprocal Public License 1.5](LICENSE.md). RPL 1.5 is a strong reciprocal
license: if you deploy a modified `buoy`, including internally, you are
required to publish your changes. Personal and research use carry no such
obligation.

Third-party material redistributed here — river's protocol XML (MIT) and the
[`tinyrwm`](https://codeberg.org/river/tinyrwm) scaffolding this started from
(0BSD) — is itemised in [NOTICE.md](NOTICE.md).
