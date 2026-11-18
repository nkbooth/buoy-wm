buoy-wm(1)

# NAME

buoy-wm - a keyboard-driven window manager for the river Wayland compositor

# SYNOPSIS

*buoy-wm* [*--version*]

# DESCRIPTION

*buoy-wm* manages windows for river 0.4 and newer through the
river-window-management-v1 protocol: river draws and handles input, and
*buoy-wm* decides window policy. It owns all tag state itself. A tag is a
named workspace; each output shows one tag, a window can carry several, and
every tag owns a pinned terminal running a multiplexer session, held
fullscreen behind every other window. Graphical applications float above it.

*buoy-wm* is normally not run by hand. The *buoy* login entry that the
packages install runs *river -c /usr/libexec/buoy-wm/buoy-session*, and that
session supervisor starts *buoy-wm* as its child (see *SESSION*).

Its behaviour is set by one optional file, described in *buoy-config*(5).
*Super+Shift+?* shows the bindings in effect.

# OPTIONS

*--version*
	Print *buoy-wm* followed by the version and exit, before connecting to
	Wayland. Only the first argument is read; any other argument is ignored.

*--print-default-config*
	Print the built-in configuration as a complete, commented config file
	and exit. It reads no config file and needs no compositor, so it works
	from a TTY: redirect it to _~/.config/buoy/config.toml_ to start your
	own. Like *--version*, it counts only as the first argument.

# REQUIREMENTS

*buoy-wm* needs a Wayland connection to river and these river globals:

- *river_window_manager_v1*, version 4 or newer;
- *river_xkb_bindings_v1*, version 1 or newer.

Without either it logs
*river_window_manager_v1 global not found! Is river running?* or
*river_xkb_bindings_v1 global not found! Is river running with xkb support?*
and exits 1.

*river_layer_shell_v1*, *river_input_manager_v1* and
*river_libinput_config_v1* are optional. Without *river_layer_shell_v1* a
status bar cannot map; without the other two, *[[input]]* settings are not
applied. Either way one log line says so.

# SESSION

The supervisor, */usr/libexec/buoy-wm/buoy-session*, imports
*WAYLAND_DISPLAY*, *DISPLAY*, *XDG_CURRENT_DESKTOP*, *XDG_SESSION_TYPE* and
*XDG_SESSION_DESKTOP* into the systemd user manager and the D-Bus
activation environment, starts *buoy-session.target* (which brings up
*graphical-session.target* and the units bound to it, among them
xdg-desktop-portal), and runs *buoy-wm* through *systemd-cat*(1), so that
its output reaches the journal as *buoy-wm* with its severities. It forwards
TERM, INT and HUP to *buoy-wm* as TERM. When *buoy-wm* exits it stops
*buoy-session.target* and then *graphical-session.target*, and exits with
*buoy-wm*'s status.

The session's *PATH* is the systemd user manager's; no shell profile runs,
so programs in *~/.local/bin* or *~/.cargo/bin* are not found.

# ENVIRONMENT

*XDG_RUNTIME_DIR*
	Where the IPC socket is created, as *$XDG_RUNTIME_DIR/buoy-wm.sock*.
	There is no fallback: when it is unset, empty or relative, or names a
	directory that is not this user's or that other users can reach,
	*buoy-wm* runs with no IPC server, and the tag pickers and every status
	bar do nothing.

*XDG_CONFIG_HOME*
	The configuration is read from *$XDG_CONFIG_HOME/buoy/config.toml* when
	this is an absolute path.

*HOME*
	Otherwise the configuration is read from
	*$HOME/.config/buoy/config.toml*, again only when *HOME* is absolute.
	With neither, no configuration is read.

*WAYLAND_DISPLAY*
	The river socket to connect to, read by the Wayland client library.

*PATH*
	Where the terminal, the launcher, *fuzzel*, *notify-send* and the
	commands of *exec* bindings are looked up.

*WAYLAND_DEBUG* is removed from the environment of every program
*buoy-wm* starts.

# FILES

_~/.config/buoy/config.toml_
	The configuration, see *buoy-config*(5). Read once, at startup.

_$XDG_RUNTIME_DIR/buoy-wm.sock_
	The IPC socket, mode 0600. Both ends check that the peer runs as the
	same user. It is removed when *buoy-wm* exits cleanly.

_/usr/bin/buoy-wm_
	A symbolic link to _/usr/libexec/buoy-wm/buoy-wm_.

_/usr/libexec/buoy-wm/buoy-wm_, _/usr/libexec/buoy-wm/buoy-tag-picker_, _/usr/libexec/buoy-wm/buoy-status-bar_, _/usr/libexec/buoy-wm/buoy-session_
	The window manager, its two companions and the session supervisor. They
	must stay in one directory: the supervisor finds *buoy-wm* beside
	itself, and *buoy-wm* finds *buoy-tag-picker* beside itself.

_/usr/share/wayland-sessions/buoy.desktop_
	The *buoy* login entry.

_/usr/lib/systemd/user/buoy-session.target_
	The user target the supervisor starts and stops.

_/usr/share/xdg-desktop-portal/buoy-portals.conf_
	Sends ScreenCast and Screenshot to xdg-desktop-portal-wlr and every
	other portal interface to xdg-desktop-portal-gtk.

_/usr/share/buoy-wm/config.example.toml_
	The output of *buoy-wm --print-default-config*: the built-in
	configuration, commented.

_/usr/share/man/man1/buoy-wm.1.gz_, _/usr/share/man/man5/buoy-config.5.gz_
	This page and *buoy-config*(5).

*buoy-wm* writes no state file and no log file.

# EXIT STATUS

*buoy-wm* exits:

*0*
	after *--version*, or when river ends the window management session.

*1*
	when a required river global is missing or too old; when another window
	manager already holds river (*Error: Another WM is already running*);
	when the Wayland connection fails or ends
	(*The Wayland connection ended, so buoy-wm is exiting:* and the reason);
	or when *--version* cannot write to standard output.

*101*
	when *buoy-wm* panics. A desktop notification beginning
	*buoy-wm panicked at* names where.

*buoy-wm* handles no signal: TERM ends it with the default action.

The supervisor exits with *buoy-wm*'s status, which is 143 when *buoy-wm*
was ended by TERM. It also exits 143 when TERM, INT or HUP arrives before
*buoy-wm* is started, and 127, after stopping both targets, when *buoy-wm*
is not executable.

# DIAGNOSTICS

The failures *buoy-wm* and the pickers report are in the journal:

```
journalctl --user -b --identifier=buoy-wm -p err
```

Only the first line of a report is logged as an error; the lines after
it, and the output of the terminals and applications *buoy-wm* starts,
are logged at info.
Without *-p err* the query shows every line. The supervisor's own lines
begin *buoy-session:* and are found with *journalctl --user -b
\_COMM=buoy-session*. On 1.0.0-rc.3 and earlier the window manager's lines
reach the journal under the display manager's name, so use
*journalctl --user -b \_COMM=buoy-wm --grep '^<3>'* there.

A problem the user must act on is also raised as a desktop notification
with *notify-send --app-name=buoy-wm --urgency=critical*: a rejected or
partly skipped configuration, a configuration file others can write, an
IPC server that did not start, a tag binding pressed before any output
exists, a panic, and every failure of the tag pickers. A session with no notification daemon shows none of them, and
each attempt logs *A child of this WM exited unsuccessfully (exit status: 1)*;
installing one (*mako*, for example) is enough, as D-Bus starts it on
demand.

# COMPANIONS

Two helpers are installed beside *buoy-wm* and are not on *PATH*. Both accept
*--version* as their first argument, and both talk to *buoy-wm* over the
IPC socket.

*buoy-tag-picker* [_output-id_ [_output-name_]]++
*buoy-tag-picker switch* _output-id_ [_output-name_]
	Started by the *open_assign_picker* and *open_switch_picker* actions.
	The first form opens a *fuzzel* checklist of every tag, checked for
	those the focused window carries; the second switches the output to a
	tag, creating it from a
	typed name. It exits 0 when done or cancelled, and 1 after any failure,
	which it also raises as a notification.

*buoy-status-bar* _output-id_
	Feeds a waybar *custom/tag* module the current tag of one output, one
	JSON line per change, until waybar stops it. The text is a warning sign
	and *disconnected* while the window manager cannot be reached, and a
	warning sign and *unknown output* when it answers but has no output with
	that id. It
	exits 1 when the argument is not one number, or when *XDG_RUNTIME_DIR*
	is unusable. The waybar module needs *"escape": true*, because the text
	is a tag name and waybar otherwise reads it as Pango markup.

# SEE ALSO

*buoy-config*(5), *river*(1), *journalctl*(1), *notify-send*(1)
