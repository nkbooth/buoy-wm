buoy-config(5)

# NAME

buoy-config - configuration file for buoy-wm(1)

# SYNOPSIS

_$XDG_CONFIG_HOME/buoy/config.toml_++
_~/.config/buoy/config.toml_

# DESCRIPTION

*buoy-wm*(1) reads one optional TOML file. When *XDG_CONFIG_HOME* is set to
an absolute path the file is _$XDG_CONFIG_HOME/buoy/config.toml_; otherwise,
when *HOME* is absolute, _$HOME/.config/buoy/config.toml_; with neither, no
file is read. A missing file is not an error: the built-in configuration
applies unchanged.

The file is read once, when *buoy-wm* starts. There is no reload: after an
edit, log out and back in.

The file holds at most four top-level names, each optional:

*defaults*
	The *[defaults]* table: program names and the pinned terminals.

*keybind*
	The *[[keybind]]* array of tables, one entry per key binding.

*mousebind*
	The *[[mousebind]]* array of tables, one entry per pointer binding.

*input*
	The *[[input]]* array of tables, one entry per libinput device pattern.

Any other name, at the top level or inside any table, rejects the whole file
(see *ERRORS*).

# DEFAULTS

Each key falls back to its built-in value on its own, so a *[defaults]*
table may set any subset.

*terminal* (string, default "foot")
	The program the *terminal* action starts, and the one each tag's pinned
	terminal runs in. Must not be empty.

*launcher* (string, default "fuzzel")
	The program the *launcher* action starts, and nothing else: the tag
	pickers and the cheat-sheet always drive *fuzzel*. Must not be empty.

*default_tag* (string, default "default")
	The tag created and shown at startup when no tag exists yet. Must not be
	empty.

*pinned_terminal_args* (array of strings)
	The arguments *terminal* is started with for a tag's pinned terminal,
	without a shell. The default is foot's spelling:

	```
	["-a", "{app_id}", "zellij", "attach", "--create", "{session}"]
	```

	Some argument must contain *{app_id}*; this is checked even when
	*pinned_terminals* is false. An empty array is accepted only together
	with *pinned_terminals = false*.

*pinned_terminals* (boolean, default true)
	Whether each tag gets a pinned terminal, started the first time the tag
	is shown. False turns them off; tags, the pickers and the status bar work
	as before.

## Placeholders

In each element of *pinned_terminal_args*, every occurrence of *{app_id}* is
replaced with *pinned-term-* and the tag's numeric id, which is how
*buoy-wm* recognises the window, and then every occurrence of *{session}* with
*tag-* and the tag's name. The prefix keeps a tag name from being read as an
option by the multiplexer. Replacement happens inside the argument, so
*--class={app_id}* works too, and never through a shell.

# KEY BINDINGS

Each *[[keybind]]* entry has these fields:

*mod* (array of modifiers, optional, default [])
	The modifiers held with the key.

*key* (string, required)
	The key, by name (see *KEY NAMES*).

*action* (required)
	What the binding does (see *ACTIONS*). *move* and *resize* drive a
	pointer drag and are skipped on a key binding.

## Modifiers

*super*, *ctrl*, *alt* and *shift*. *ALIASES* lists the other accepted
spellings.

## Key names

A key name is tried, in order, as:

. a named key, compared without regard to case: *space*, *Return*,
  *Escape*, *Tab*, *BackSpace*, *Delete*, *Insert*, *Home*, *End*, *Page_Up*,
  *Page_Down*, *Left*, *Up*, *Right*, *Down*, *Print*, *Pause*, *Menu*;
. a function key, *F1* to *F35*, also written with a lower-case *f*;
. one printable ASCII character, which is its own keysym, so case matters.

No other name exists: there is no *Enter* or *Esc*, no keypad key and no
XF86 media key. An upper-case letter is the shifted symbol, so it must be
bound with *shift* in *mod* or it is skipped, since it could never fire.

# POINTER BINDINGS

Each *[[mousebind]]* entry has these fields:

*mod* (array of modifiers, optional, default [])
	The modifiers held with the button.

*button* (required)
	*left*, *right* or *middle*.

*action* (required)
	Any action, including *move* and *resize*.

# ACTIONS

An action is a string, except *exec* and *switch_tag*, which take an
argument and are written as a table with one key.

*terminal*
	Start the *terminal* program as a floating window.

*launcher*
	Start the *launcher* program.

*close*
	Close the focused window. A tag's pinned terminal is never closed.

*focus_next*
	Move focus to the next window.

*exit*
	End *buoy-wm*, and with it the session.

*cycle_tag*
	Show the next tag on the active output, the one under the pointer.

*open_assign_picker*
	Open the picker that assigns tags to the focused window.

*open_switch_picker*
	Open the picker that switches the active output to a tag, or creates one
	from a typed name.

*hotkeys*
	Show the cheat-sheet of the bindings in effect.

*move*, *resize*
	Move or resize the window under the pointer while the button is held.
	Pointer bindings only.

*exec* = "_command_"
	Run _command_ with *sh -c*, so pipes, arguments and *~* work as in a
	shell.

*switch_tag* = "_name_"
	Show the tag _name_ on the active output, creating it if it does not
	exist.

# ALIASES

Older and alternative spellings, accepted wherever their canonical name is:

*tag_picker* is *open_assign_picker*.

*tag_switch* is *open_switch_picker*.

*Super* is *super*.

*mod4* is *super*.

*Mod4* is *super*.

*logo* is *super*.

*Logo* is *super*.

*win* is *super*.

*Win* is *super*.

*Ctrl* is *ctrl*.

*control* is *ctrl*.

*Control* is *ctrl*.

*Alt* is *alt*.

*mod1* is *alt*.

*Mod1* is *alt*.

*Shift* is *shift*.

*Left* is *left*.

*Right* is *right*.

*Middle* is *middle*.

# INPUT DEVICES

Each *[[input]]* entry configures every libinput device whose name matches
its pattern. Entries are tried top to bottom and the first match wins, so a
specific device goes above a catch-all *name = "\*"*. A setting left out
sends nothing and keeps libinput's own default; false actively turns a
feature off.

*name* (string, required)
	A case-sensitive pattern matched against the device name. *\** matches
	any run of characters and is the only wildcard. A pattern that matches
	no device is accepted silently; *libinput list-devices* shows the names.

*tap* (boolean)
	Tap-to-click.

*tap_button_map*
	Which button one, two and three fingers produce: *lrm* (left, right,
	middle) or *lmr* (left, middle, right).

*click_method*
	How a physical press on a clickpad picks a button: *none*,
	*button_areas* (zones along the bottom of the pad) or *clickfinger* (the
	number of fingers).

*natural_scroll* (boolean)
	Content follows the fingers.

*disable_while_typing* (boolean)
	Ignore the touchpad briefly while typing.

*accel_speed* (number)
	Pointer acceleration from -1.0, the slowest, to 1.0, the fastest.

For every device it configures, *buoy-wm* logs the tap and click-method
state the device reports, and names any setting the device refuses.

# LISTS REPLACE THE BUILT-INS

A *[[keybind]]* list that still has at least one entry after skipped
entries are dropped replaces the built-in key bindings wholesale; nothing is merged, which is what makes a
built-in binding removable. *[[mousebind]]* and *[[input]]* work the same
way, each on its own. A list that is absent, written as an empty array, or
left empty because every entry in it was skipped falls back to the built-in
list, so no list can be made empty. To stop the built-in touchpad entry
from enabling tap, write an entry for the touchpad with *tap = false*.

# ERRORS

An entry the parser reads but *buoy-wm* cannot use is skipped, and the rest
of the file applies:

- a key name that does not exist, or an upper-case letter without *shift*;
- *move* or *resize* on a key binding;
- a binding whose modifiers and key, or modifiers and button, are written
  exactly as an earlier one's; the later one is skipped. Modifiers written
  in another order, or a key in another case, are not caught;
- an *[[input]]* entry with an empty *name*, no setting, the *name* of an
  earlier entry, or an *accel_speed* outside -1.0 to 1.0.

Anything else rejects the whole file, and the built-in configuration
applies in full, key bindings included:

- a file that is not valid TOML;
- an unknown name in any table, or a value of the wrong type;
- a misspelled action, modifier, button, *tap_button_map* or *click_method*;
- a missing *key*, *button*, *action* or *name*;
- any problem in *[defaults]*;
- a file that cannot be read, is larger than 1 MiB, or is not a regular
  file.

Either way *buoy-wm* still starts. It raises a desktop notification naming
the file and every problem at once, and logs the same text:

```
journalctl --user -b --identifier=buoy-wm -p err
```

# TRUST

An *exec* binding runs whatever the file says, so the file is as trusted as
a shell profile. When it belongs to another user, or its group or others
may write it, *buoy-wm* loads it anyway and warns, for example that the
config file's mode is 0664. A new file is 0664 under the usual umask;
*chmod 600* on it clears the warning. Who can read the file is not checked.

# BUILT-IN BINDINGS

The bindings in effect with no file, as the cheat-sheet labels them:

*Super+space*
	Spawn terminal (foot)

*Super+q*
	Close focused window

*Super+n*
	Cycle focus

*Super+Escape*
	Exit session

*Super+Tab*
	Cycle tag

*Super+a*
	Tag manager (assign tags to focused window)

*Super+s*
	Switch tag (or type a new name to create)

*Super+r*
	App launcher (fuzzel)

*Super+Shift+?*
	Show this hotkey list

*Super+LeftClick*
	Move window

*Super+RightClick*
	Resize window

# BUILT-IN INPUT

One entry, *name = "\*Touchpad\*"* with *tap = true*, because libinput leaves
tap-to-click off on any device with physical buttons, which includes most
laptop touchpads.

# EXAMPLES

_/usr/share/buoy-wm/config.example.toml_ is the built-in configuration
written out with comments, plus four marked examples that are not built in.
Because a *[[keybind]]* list replaces the built-ins, a file that adds
bindings must also list every built-in binding it keeps. Two additions,
to be listed beside those:

```
[[keybind]]
mod = ["super"]
key = "1"
action = { switch_tag = "email" }

[[keybind]]
mod = ["super"]
key = "p"
action = { exec = "grim -g \\"$(slurp)\\" ~/shot.png" }
```

# SEE ALSO

*buoy-wm*(1)
