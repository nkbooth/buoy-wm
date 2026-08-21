// SPDX-FileCopyrightText: © 2026 Nick Booth
// SPDX-License-Identifier: RPL-1.5
//
// Unless explicitly acquired and licensed from Licensor under another
// license, the contents of this file are subject to the Reciprocal Public
// License ("RPL") Version 1.5, or subsequent versions as allowed by the
// RPL, and You may not copy or use this file in either source code or
// executable form, except in compliance with the terms and conditions of
// the RPL.
//
// All software distributed under the RPL is provided strictly on an "AS
// IS" basis, WITHOUT WARRANTY OF ANY KIND, EITHER EXPRESS OR IMPLIED, AND
// LICENSOR HEREBY DISCLAIMS ALL SUCH WARRANTIES, INCLUDING WITHOUT
// LIMITATION, ANY WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR
// PURPOSE, QUIET ENJOYMENT, OR NON-INFRINGEMENT. See the RPL for specific
// language governing rights and limitations under the RPL.

//! User configuration: `~/.config/buoy/config.toml`.
//!
//! Every value this module exposes was a compile-time constant before it
//! existed, so [`Config::default`] reproduces the previously-hardcoded
//! behavior exactly — a missing or empty config file is not an error and
//! changes nothing.

pub mod glob;
pub mod keysym;

use serde::Deserialize;
use std::ffi::OsStr;
use std::fmt;
use std::io::Read;
use std::os::unix::fs::MetadataExt;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

/// A modifier key a binding can require.
///
/// `rename_all` because every other enum in this file has it, and a config
/// block mixing `mod = ["Super"]` with `action = "resize"` made the file
/// look like it had two casing systems (audit finding C-11). The
/// PascalCase spellings are aliases rather than removals: they are what
/// the README and the shipped example documented for two releases, so
/// every config already on disk is written in them, and dropping one would
/// empty that user's keymap at their next login. The remaining aliases
/// accept the spellings the X11/xkb world uses interchangeably, so a user
/// writing `mod4` or `control` isn't told their config is wrong.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Modifier {
    #[serde(
        alias = "Super",
        alias = "mod4",
        alias = "Mod4",
        alias = "logo",
        alias = "Logo",
        alias = "win",
        alias = "Win"
    )]
    Super,
    #[serde(alias = "Ctrl", alias = "control", alias = "Control")]
    Ctrl,
    #[serde(alias = "Alt", alias = "mod1", alias = "Mod1")]
    Alt,
    #[serde(alias = "Shift")]
    Shift,
}

/// A pointer button a binding can require. PascalCase is an alias for the
/// same reason it is on [`Modifier`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Button {
    #[serde(alias = "Left")]
    Left,
    #[serde(alias = "Right")]
    Right,
    #[serde(alias = "Middle")]
    Middle,
}

/// What a binding does when it fires.
///
/// Serde's default externally-tagged representation is what lets a unit
/// variant be written as a bare string (`action = "close"`) and a
/// parameterized one as a single-key table (`action = { exec = "..." }`) in
/// the same field.
///
/// `Debug` is hand-written rather than derived so that
/// [`Action::Exec`]'s command line — the user's own shell text, which can
/// carry tokens — cannot be printed by accident (audit finding G-05). The
/// `match` is exhaustive, so a new variant is a compile error here rather
/// than a variant that quietly prints whatever it holds.
#[derive(Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Action {
    /// Spawn `defaults.terminal`.
    Terminal,
    /// Spawn `defaults.launcher`.
    Launcher,
    Close,
    FocusNext,
    Exit,
    CycleTag,
    /// Open the tag-assignment picker for the focused window.
    ///
    /// `tag_picker` remains an alias for the same reason [`Modifier`]'s
    /// PascalCase spellings do: it is the spelling the README and the
    /// shipped example documented, so it is what every config already on
    /// disk says, and a config whose `action` no longer parses loses that
    /// binding silently (audit finding J-08).
    #[serde(alias = "tag_picker")]
    OpenAssignPicker,
    /// Open the tag-switch picker for the active output.
    ///
    /// Named for what it opens, not for what the user eventually picks —
    /// `tag_switch` and [`Action::SwitchTag`]'s `switch_tag` were one
    /// transposition apart while meaning different things, so either
    /// spelling silently did the other one's job (audit finding J-08).
    /// `tag_switch` stays an alias, as on [`Action::OpenAssignPicker`].
    #[serde(alias = "tag_switch")]
    OpenSwitchPicker,
    /// Show the generated hotkey cheat-sheet.
    Hotkeys,
    Move,
    Resize,
    /// Run an arbitrary command line.
    Exec(String),
    /// Switch the active output to a tag by name, creating it if it does
    /// not exist yet.
    SwitchTag(String),
}

impl std::fmt::Debug for Action {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Exec(_) => f.write_str("Exec(<redacted>)"),
            Self::SwitchTag(name) => write!(f, "SwitchTag({name:?})"),
            Self::Terminal => f.write_str("Terminal"),
            Self::Launcher => f.write_str("Launcher"),
            Self::Close => f.write_str("Close"),
            Self::FocusNext => f.write_str("FocusNext"),
            Self::Exit => f.write_str("Exit"),
            Self::CycleTag => f.write_str("CycleTag"),
            Self::OpenAssignPicker => f.write_str("OpenAssignPicker"),
            Self::OpenSwitchPicker => f.write_str("OpenSwitchPicker"),
            Self::Hotkeys => f.write_str("Hotkeys"),
            Self::Move => f.write_str("Move"),
            Self::Resize => f.write_str("Resize"),
        }
    }
}

/// Program names and values that were compile-time constants before this
/// module existed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Defaults {
    pub terminal: String,
    pub launcher: String,
    /// The tag created and shown at startup when no tag exists yet.
    pub default_tag: String,
    /// Argv passed to `terminal` when spawning a tag's pinned terminal.
    /// `{app_id}` and `{session}` are substituted (see
    /// [`Defaults::pinned_terminal_argv`]). Configurable because the flag
    /// that sets a window's app-id is terminal-specific — foot spells it
    /// `-a`, most others `--class` — and hardcoding foot's spelling made
    /// setting `terminal` to anything else break every pinned terminal
    /// silently and permanently.
    pub pinned_terminal_args: Vec<String>,
    /// Whether each tag gets a lazily-spawned pinned terminal at all.
    ///
    /// The one user-facing off switch in this config, and it exists because
    /// this is the one subsystem that launches a process from the same
    /// binary that *is* the login session: a user whose terminal or
    /// `zellij` is broken had no supported way to stop buoy trying to
    /// spawn it (audit finding C-10). Off means no spawn and no claim
    /// spent; every other part of tagging is unaffected.
    pub pinned_terminals: bool,
}

impl Defaults {
    /// [`Defaults::pinned_terminal_args`] with its placeholders filled in:
    /// `{app_id}` becomes `app_id`, `{session}` becomes `session_name`.
    ///
    /// Substitution is per-argument and whole-token, so a tag name can
    /// contain anything at all — the result is passed to `Command` as
    /// separate argv entries, never through a shell.
    pub fn pinned_terminal_argv(&self, app_id: &str, session_name: &str) -> Vec<String> {
        self.pinned_terminal_args
            .iter()
            .map(|arg| {
                arg.replace("{app_id}", app_id)
                    .replace("{session}", session_name)
            })
            .collect()
    }
}

impl Default for Defaults {
    fn default() -> Self {
        Self {
            terminal: "foot".to_string(),
            launcher: "fuzzel".to_string(),
            default_tag: "default".to_string(),
            pinned_terminal_args: [
                "-a",
                "{app_id}",
                "zellij",
                "attach",
                "--create",
                "{session}",
            ]
            .iter()
            .map(|s| s.to_string())
            .collect(),
            // On, because it is what buoy did before the switch existed.
            pinned_terminals: true,
        }
    }
}

/// A modifier+key binding.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Keybind {
    /// Defaulted so a modifier-less binding (a bare `F1`, say) can simply
    /// omit `mod` — without this, serde rejects the whole file for a
    /// missing field and the user loses their entire keymap over one
    /// binding (code-review follow-up).
    #[serde(rename = "mod", default)]
    pub mods: Vec<Modifier>,
    pub key: String,
    pub action: Action,
}

impl Keybind {
    /// This binding's key name resolved to an X11 keysym, or `None` if the
    /// name isn't recognized. [`Config::parse_lenient`] rejects the latter
    /// at load,
    /// so a `Keybind` reaching the WM always resolves.
    pub fn keysym(&self) -> Option<u32> {
        keysym::from_name(&self.key)
    }
}

/// A modifier+pointer-button binding.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Mousebind {
    /// Defaulted for the same reason as [`Keybind::mods`].
    #[serde(rename = "mod", default)]
    pub mods: Vec<Modifier>,
    pub button: Button,
    pub action: Action,
}

/// Which button each finger count produces when tap-to-click is on.
/// Spellings match libinput's own, so a user reading `libinput
/// list-devices` output sees the same words.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TapButtonMap {
    /// 1/2/3-finger tap maps to left/right/middle.
    Lrm,
    /// 1/2/3-finger tap maps to left/middle/right.
    Lmr,
}

/// How a physical press on a clickpad resolves to a button.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClickMethod {
    /// No physical-click buttons at all.
    None,
    /// The bottom of the pad is split into left/right button zones.
    ButtonAreas,
    /// The button is decided by how many fingers rest on the pad.
    Clickfinger,
}

/// libinput settings for every device whose name matches [`InputConfig::
/// name`].
///
/// Every setting is an `Option` because `None` and `Some(false)` mean
/// different things here: `None` sends no request at all and leaves
/// libinput's own default alone, while `Some(false)` actively turns the
/// feature off. Collapsing the two would make it impossible to configure
/// one setting on a device without silently restating every other.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InputConfig {
    /// A `*`-wildcard pattern matched against the libinput device name
    /// (see [`glob`]).
    pub name: String,
    /// Tap-to-click. The one setting this whole section was added for:
    /// libinput defaults it off for any device with physical buttons, so a
    /// laptop touchpad ignores taps until something turns it on.
    pub tap: Option<bool>,
    pub tap_button_map: Option<TapButtonMap>,
    pub click_method: Option<ClickMethod>,
    pub natural_scroll: Option<bool>,
    /// libinput calls this `dwt`.
    pub disable_while_typing: Option<bool>,
    /// Pointer acceleration, `-1.0` (slowest) to `1.0` (fastest).
    pub accel_speed: Option<f64>,
}

/// One libinput setting to apply, already resolved from an
/// [`InputConfig`]'s `Option` fields to a concrete value.
///
/// Exists so the "which requests does this entry imply" decision is
/// testable on its own, without a compositor: the WM's job is then a
/// mechanical match from each variant to the matching protocol request.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum LibinputSetting {
    Tap(bool),
    TapButtonMap(TapButtonMap),
    ClickMethod(ClickMethod),
    NaturalScroll(bool),
    DisableWhileTyping(bool),
    AccelSpeed(f64),
}

impl InputConfig {
    /// Whether this entry applies to the device called `device_name`.
    pub fn matches(&self, device_name: &str) -> bool {
        glob::matches(&self.name, device_name)
    }

    /// The settings this entry actually asks for, in the order they must be
    /// applied.
    ///
    /// An unset field contributes nothing, so libinput keeps its own
    /// default for it — only a field the user wrote turns into a request.
    /// `tap` deliberately precedes `tap_button_map`, which is meaningless
    /// until tap is enabled.
    pub fn settings(&self) -> Vec<LibinputSetting> {
        [
            self.tap.map(LibinputSetting::Tap),
            self.tap_button_map.map(LibinputSetting::TapButtonMap),
            self.click_method.map(LibinputSetting::ClickMethod),
            self.natural_scroll.map(LibinputSetting::NaturalScroll),
            self.disable_while_typing
                .map(LibinputSetting::DisableWhileTyping),
            self.accel_speed.map(LibinputSetting::AccelSpeed),
        ]
        .into_iter()
        .flatten()
        .collect()
    }

    /// Whether this entry sets anything at all. An entry that names a
    /// device and configures nothing is rejected at load — it reads like it
    /// does something.
    ///
    /// Defined in terms of [`InputConfig::settings`] rather than as a
    /// hand-written inverse of it, so a field added to one and forgotten in
    /// the other cannot make the two disagree (audit finding C-09).
    fn is_empty(&self) -> bool {
        self.settings().is_empty()
    }
}

/// The parsed contents of `~/.config/buoy/config.toml`.
///
/// Not `Eq`: [`InputConfig::accel_speed`] is an `f64`, and every use here
/// is `assert_eq!` in tests rather than anything needing total equality.
#[derive(Debug, Clone, PartialEq)]
pub struct Config {
    pub defaults: Defaults,
    pub keybinds: Vec<Keybind>,
    pub mousebinds: Vec<Mousebind>,
    pub inputs: Vec<InputConfig>,
}

/// Why a config file was rejected. A bad config is always fatal to *that
/// file*, never silently partially applied — a half-loaded keymap is
/// harder to diagnose than a refusal.
#[derive(Debug)]
pub enum ConfigError {
    Toml(toml::de::Error),
    /// A `key = "..."` name no keysym matches, carried by name so the
    /// message can point at the actual typo.
    UnknownKey(String),
    /// A key that can only be produced with Shift held, bound without
    /// `Shift` in its modifier set — see [`shifted_without_shift`].
    ShiftedKeyWithoutShift(String),
    /// A binding action that only makes sense on a pointer button, bound
    /// to a key.
    PointerOnlyAction(String),
    /// A `[defaults]` value that is present but empty.
    EmptyDefault(&'static str),
    /// The same modifier set and trigger bound twice.
    DuplicateBinding(String),
    /// `pinned_terminal_args` without the `{app_id}` placeholder.
    MissingAppIdPlaceholder,
    /// `pinned_terminal_args = []` while pinned terminals are still on —
    /// the way a user tries to turn the feature off before finding
    /// [`Defaults::pinned_terminals`].
    EmptyPinnedTerminalArgs,
    /// An `[[input]]` with `name = ""`, which matches no real device.
    EmptyInputName,
    /// An `[[input]]` that names a device but configures nothing.
    InputWithoutSettings(String),
    /// The same `[[input]]` `name` pattern listed twice.
    DuplicateInput(String),
    /// An `accel_speed` outside libinput's `-1.0..=1.0`.
    AccelSpeedOutOfRange(f64),
    /// The file is bigger than [`MAX_CONFIG_BYTES`].
    TooLarge,
    /// The config path is a directory, a FIFO, a device — anything whose
    /// read would not simply return the file's bytes.
    NotARegularFile,
    Io(std::io::Error),
    /// More than one offending entry in the same file. Reported together
    /// so four independent mistakes take one edit-and-restart cycle
    /// instead of four (audit finding G-04).
    Multiple(Vec<ConfigError>),
}

impl ConfigError {
    /// Folds a list of per-entry errors into one, or `None` when the list
    /// is empty.
    ///
    /// A single error is returned as itself rather than wrapped, so
    /// existing call sites and tests can still match on the variant
    /// directly; only a genuine plural becomes [`ConfigError::Multiple`].
    pub fn from_many(mut errors: Vec<ConfigError>) -> Option<Self> {
        match errors.len() {
            0 => None,
            1 => errors.pop(),
            _ => Some(Self::Multiple(errors)),
        }
    }
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ConfigError::Toml(e) => write!(f, "{e}"),
            ConfigError::UnknownKey(name) => {
                write!(f, "unknown key name `{name}` (not a recognized keysym)")
            }
            ConfigError::ShiftedKeyWithoutShift(name) => write!(
                f,
                "key `{name}` is a shifted symbol, so its binding must list \
                 \"Shift\" as a modifier or it can never fire — either add \
                 \"Shift\" or use the unshifted key name (e.g. `{}`)",
                name.to_lowercase()
            ),
            ConfigError::PointerOnlyAction(action) => write!(
                f,
                "action `{action}` acts on a pointer drag, so it belongs to a \
                 [[mousebind]], not a [[keybind]]"
            ),
            ConfigError::EmptyDefault(field) => {
                write!(
                    f,
                    "[defaults] `{field}` is empty; remove it to keep the built-in value"
                )
            }
            ConfigError::DuplicateBinding(label) => {
                write!(f, "`{label}` is bound more than once")
            }
            ConfigError::MissingAppIdPlaceholder => write!(
                f,
                "[defaults] `pinned_terminal_args` must contain the `{{app_id}}` placeholder — it is how the WM recognizes a tag's pinned terminal"
            ),
            ConfigError::EmptyPinnedTerminalArgs => write!(
                f,
                "[defaults] `pinned_terminal_args` is empty, so a pinned \
                 terminal would spawn carrying no app id and never be \
                 recognized — write `pinned_terminals = false` to turn the \
                 feature off instead"
            ),
            ConfigError::EmptyInputName => write!(
                f,
                "[[input]] `name` is empty, so it matches no device — use \
                 `name = \"*\"` if matching every device was the intent"
            ),
            ConfigError::InputWithoutSettings(name) => write!(
                f,
                "[[input]] `{name}` sets nothing, so it has no effect — give \
                 it at least one setting or remove it"
            ),
            ConfigError::DuplicateInput(name) => write!(
                f,
                "[[input]] `{name}` is listed more than once; the first match \
                 wins, so the later entry could never apply"
            ),
            ConfigError::AccelSpeedOutOfRange(speed) => write!(
                f,
                "[[input]] `accel_speed` is {speed}, outside libinput's \
                 -1.0..=1.0 range"
            ),
            ConfigError::TooLarge => write!(
                f,
                "the config file is larger than the 1 MiB buoy-wm will read"
            ),
            ConfigError::NotARegularFile => write!(
                f,
                "the config path is not a regular file, so it was not read"
            ),
            ConfigError::Io(e) => write!(f, "{e}"),
            ConfigError::Multiple(errors) => {
                write!(f, "{} problems:", errors.len())?;
                for error in errors {
                    write!(f, "\n  - {error}")?;
                }
                Ok(())
            }
        }
    }
}

impl std::error::Error for ConfigError {
    /// The two wrapping variants expose their cause; the rest have none.
    ///
    /// The default empty body inherited `source() -> None` even for `Toml`
    /// and `Io`, so anything walking the chain — a `while let Some(src) =
    /// e.source()` loop, or any error-report crate — silently got nothing
    /// from an error that demonstrably had a cause (audit finding F-04).
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            ConfigError::Toml(e) => Some(e),
            ConfigError::Io(e) => Some(e),
            _ => None,
        }
    }
}

impl Default for Config {
    /// The built-in configuration: exactly the bindings and program names
    /// that were hardcoded before this module existed, so running with no
    /// config file changes nothing.
    fn default() -> Self {
        let keybind = |key: &str, action: Action| Keybind {
            mods: vec![Modifier::Super],
            key: key.to_string(),
            action,
        };
        Self {
            defaults: Defaults::default(),
            keybinds: vec![
                keybind("space", Action::Terminal),
                keybind("q", Action::Close),
                keybind("n", Action::FocusNext),
                keybind("Escape", Action::Exit),
                keybind("Tab", Action::CycleTag),
                keybind("a", Action::OpenAssignPicker),
                keybind("s", Action::OpenSwitchPicker),
                keybind("r", Action::Launcher),
                // The only built-in needing a second modifier: xkbcommon
                // has no unshifted `?` keysym, so the binding must match
                // the shifted symbol the layout actually produces.
                Keybind {
                    mods: vec![Modifier::Super, Modifier::Shift],
                    key: "?".to_string(),
                    action: Action::Hotkeys,
                },
            ],
            mousebinds: vec![
                Mousebind {
                    mods: vec![Modifier::Super],
                    button: Button::Left,
                    action: Action::Move,
                },
                Mousebind {
                    mods: vec![Modifier::Super],
                    button: Button::Right,
                    action: Action::Resize,
                },
            ],
            // Unlike every other default in this file, this one is *not*
            // reproducing prior hardcoded behavior — before `[[input]]`
            // nothing configured input devices at all, so a touchpad kept
            // libinput's own tap-to-click default of off and taps did
            // nothing. Enabling it here is what makes a fresh session
            // behave like any other desktop with no config file to write.
            //
            // Matched on the name rather than the protocol's device *type*
            // because type is only `pointer` — the same value an external
            // mouse or trackball reports, neither of which should have tap
            // forced on. Every touchpad the kernel names carries
            // "Touchpad" (this laptop's is `PIXA3854:00 093A:0274
            // Touchpad`); a device that somehow doesn't needs its own
            // `[[input]]` entry, which is exactly what the section is for.
            inputs: vec![InputConfig {
                name: "*Touchpad*".to_string(),
                tap: Some(true),
                tap_button_map: None,
                click_method: None,
                natural_scroll: None,
                disable_while_typing: None,
                accel_speed: None,
            }],
        }
    }
}

/// Returns the offending key name when a binding names a keysym that can
/// only be produced with Shift held, but doesn't list `Shift` as a
/// modifier — a binding that can therefore never fire.
///
/// Deliberately narrow: only a single ASCII *letter* is judged. Uppercase
/// letters require Shift on every Latin layout, so the rule is safe there.
/// Other shifted symbols (`?`, `!`, `@`) are layout-dependent — `?` needs
/// Shift on a US layout but that does not generalize — so rejecting them
/// would break legitimate configs on other layouts. Named keys are never
/// shifted symbols however they are capitalized.
fn shifted_without_shift(keybind: &Keybind) -> Option<&str> {
    let mut chars = keybind.key.chars();
    let only = chars.next().filter(|_| chars.next().is_none())?;
    let needs_shift = only.is_ascii_uppercase();
    let has_shift = keybind.mods.contains(&Modifier::Shift);
    (needs_shift && !has_shift).then_some(keybind.key.as_str())
}

/// The wire shape of the file, kept separate from [`Config`] so absent
/// sections can fall back per-key to the built-in defaults rather than
/// forcing the user to restate every value they didn't want to change.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawConfig {
    #[serde(default)]
    defaults: RawDefaults,
    #[serde(default, rename = "keybind")]
    keybinds: Vec<Keybind>,
    #[serde(default, rename = "mousebind")]
    mousebinds: Vec<Mousebind>,
    #[serde(default, rename = "input")]
    inputs: Vec<InputConfig>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawDefaults {
    terminal: Option<String>,
    launcher: Option<String>,
    default_tag: Option<String>,
    pinned_terminal_args: Option<Vec<String>>,
    pinned_terminals: Option<bool>,
}

/// The most this module will read from a config file.
///
/// river `exec`s `buoy-wm` as the login session leader, so an unbounded
/// read — plus the further multiple of it the TOML parser allocates — makes
/// an accidentally enormous file a login loop rather than a recoverable
/// error (audit finding C-07). A megabyte is roughly two thousand times the
/// shipped example. Mirrors
/// [`MAX_LINE_BYTES`](buoy_common::framing::MAX_LINE_BYTES), which bounds
/// the other place untrusted-length input reaches this process.
pub const MAX_CONFIG_BYTES: u64 = 1024 * 1024;

/// Why a config file's ownership or mode makes it a risk, given that it
/// grants arbitrary command execution through `{ exec = "..." }`.
///
/// This is a warning, never a refusal: refusing means running with the
/// built-in keybinds, and a user locked out of the keymap they actually use
/// is worse off than one who was told to run `chmod` (audit finding C-08).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfigTrustProblem {
    /// Group or others may write the file, so any local user in that set
    /// can add an `exec` binding to this session. Deliberately *not*
    /// triggered by readability: `644` is what an ordinary editor save or
    /// dotfile-manager apply produces, and warning about it would train the
    /// user to ignore the case that matters.
    WritableByOthers { mode: u32 },
    /// The file belongs to a different user, so its contents are that
    /// user's to change and this one cannot even fix the mode.
    ForeignOwner { file_uid: u32, effective_uid: u32 },
}

impl fmt::Display for ConfigTrustProblem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ConfigTrustProblem::WritableByOthers { mode } => write!(
                f,
                "the config file's mode is {mode:04o}, so another local user \
                 can write it — and an `exec` binding runs whatever they put \
                 there in your session. Run `chmod 600` on it. It was loaded \
                 anyway."
            ),
            ConfigTrustProblem::ForeignOwner {
                file_uid,
                effective_uid,
            } => write!(
                f,
                "the config file belongs to uid {file_uid}, not to the uid \
                 running buoy-wm ({effective_uid}), so its `exec` bindings \
                 are that user's to change. It was loaded anyway."
            ),
        }
    }
}

/// Whether `mode` and `file_uid` make a config file untrustworthy for the
/// user identified by `effective_uid`.
///
/// Split from the file I/O so the decision is testable without a second
/// user account and without root — the same split
/// [`accept_peer`](buoy_common::peer::accept_peer) uses.
///
/// Only the `0o022` bits are examined. Bash refuses a world-writable
/// `~/.bashrc` and ssh a group-writable key for this reason; neither cares
/// who can *read* it, and neither does this.
pub fn config_trust_problem(
    mode: u32,
    file_uid: u32,
    effective_uid: u32,
) -> Option<ConfigTrustProblem> {
    if file_uid != effective_uid {
        return Some(ConfigTrustProblem::ForeignOwner {
            file_uid,
            effective_uid,
        });
    }
    if mode & 0o022 != 0 {
        return Some(ConfigTrustProblem::WritableByOthers {
            mode: mode & 0o7777,
        });
    }
    None
}

/// A config file that was read: what it says, what in it was skipped, and
/// anything about the file itself that the user should know.
///
/// A struct rather than a tuple because the two lists mean different things
/// to the caller — `skipped` entries were dropped from `config`, while
/// `trust_problem` cost nothing and changed nothing — and a caller reading
/// `(config, a, b)` at the call site cannot tell which is which.
#[derive(Debug)]
pub struct LoadedConfig {
    /// The configuration to run with. Never partial: an entry that was
    /// skipped is absent, and everything else in the file applies.
    pub config: Config,
    /// One error per `[[keybind]]`, `[[mousebind]]` or `[[input]]` entry
    /// that was skipped.
    pub skipped: Vec<ConfigError>,
    /// Set when the file's own ownership or mode is a risk (audit finding
    /// C-08). Independent of `skipped`: the file was used either way.
    pub trust_problem: Option<ConfigTrustProblem>,
}

impl Config {
    /// Parses config file contents, rejecting the file if anything in it
    /// is wrong.
    ///
    /// Declaring any `[[keybind]]` replaces the built-in keybind set
    /// outright rather than merging into it — merging would leave a
    /// built-in binding impossible to remove, and would silently
    /// reintroduce a default the user had deliberately rebound elsewhere.
    /// `[[mousebind]]` is an independent list with the same rule.
    ///
    /// Every offending entry is reported, not just the first: a file with
    /// four independent mistakes used to take four edit-and-restart cycles
    /// to clear (audit finding G-04). [`Config::load`] uses
    /// [`Config::parse_lenient`] instead — this strict form is what a test
    /// wants, where "the file is not what the user wrote" has to be a
    /// failure — including the test that asserts the shipped
    /// `docs/config.example.toml` has nothing wrong with it at all.
    /// `#[cfg(test)]` rather than `#[allow(dead_code)]`: nothing in the
    /// running WM should reject a whole file any more, and a strict entry
    /// point left reachable is one a future call site can reach by
    /// accident.
    #[cfg(test)]
    pub fn parse(contents: &str) -> Result<Self, ConfigError> {
        let (config, skipped) = Self::parse_lenient(contents)?;
        match ConfigError::from_many(skipped) {
            Some(error) => Err(error),
            None => Ok(config),
        }
    }

    /// Parses config file contents, skipping individual `[[keybind]]`,
    /// `[[mousebind]]` and `[[input]]` entries that are invalid and
    /// returning one error per skipped entry.
    ///
    /// This is what the WM actually loads with, and it is what the
    /// project's own recorded acceptance criterion always said should
    /// happen — a bad binding is *"skipped and logged"*. The behaviour it
    /// replaces threw the whole file away over one typo: every other
    /// binding, every `[defaults]` value and every `[[input]]` block
    /// reverted to the built-ins, which for a user who had long since
    /// rebound `Super+Return` restored a keymap they no longer knew (audit
    /// finding G-04).
    ///
    /// `Err` is reserved for what cannot be skipped: a file that is not
    /// valid TOML, and a `[defaults]` value that is wrong. There is no
    /// "this entry" to drop for a `[defaults]` mistake, and quietly
    /// substituting the built-in value would be the whole-file discard in
    /// miniature.
    pub fn parse_lenient(contents: &str) -> Result<(Self, Vec<ConfigError>), ConfigError> {
        let raw: RawConfig = toml::from_str(contents).map_err(ConfigError::Toml)?;
        let built_in = Config::default();
        validate_defaults(&raw.defaults)?;

        let mut skipped: Vec<ConfigError> = Vec::new();

        // Every check below names the offender rather than letting a
        // binding register and then silently never fire — the failure mode
        // is invisible from the user's side, so it has to be caught while
        // there is still something to point at.
        let mut seen: Vec<String> = Vec::new();
        let keybinds: Vec<Keybind> = raw
            .keybinds
            .into_iter()
            .filter(|keybind| match validate_keybind(keybind, &mut seen) {
                Ok(()) => true,
                Err(e) => {
                    skipped.push(e);
                    false
                }
            })
            .collect();

        let mut seen_buttons: Vec<String> = Vec::new();
        let mousebinds: Vec<Mousebind> = raw
            .mousebinds
            .into_iter()
            .filter(
                |mousebind| match validate_mousebind(mousebind, &mut seen_buttons) {
                    Ok(()) => true,
                    Err(e) => {
                        skipped.push(e);
                        false
                    }
                },
            )
            .collect();

        let mut seen_inputs: Vec<String> = Vec::new();
        let inputs: Vec<InputConfig> = raw
            .inputs
            .into_iter()
            .filter(|input| match validate_input(input, &mut seen_inputs) {
                Ok(()) => true,
                Err(e) => {
                    skipped.push(e);
                    false
                }
            })
            .collect();

        let config = Self {
            defaults: Defaults {
                terminal: raw.defaults.terminal.unwrap_or(built_in.defaults.terminal),
                launcher: raw.defaults.launcher.unwrap_or(built_in.defaults.launcher),
                default_tag: raw
                    .defaults
                    .default_tag
                    .unwrap_or(built_in.defaults.default_tag),
                pinned_terminal_args: raw
                    .defaults
                    .pinned_terminal_args
                    .unwrap_or(built_in.defaults.pinned_terminal_args),
                pinned_terminals: raw
                    .defaults
                    .pinned_terminals
                    .unwrap_or(built_in.defaults.pinned_terminals),
            },
            // An empty list after skipping lands on the same documented
            // rule as an empty list in the file: no declaration means the
            // built-in set.
            keybinds: if keybinds.is_empty() {
                built_in.keybinds
            } else {
                keybinds
            },
            mousebinds: if mousebinds.is_empty() {
                built_in.mousebinds
            } else {
                mousebinds
            },
            inputs: if inputs.is_empty() {
                built_in.inputs
            } else {
                inputs
            },
        };
        Ok((config, skipped))
    }

    /// Loads [`config_path`]'s file, falling back to [`Config::default`]
    /// when it doesn't exist — running without a config file is the
    /// expected case, not an error.
    ///
    /// `Err` means the file could not be used at all: unreadable, larger
    /// than [`MAX_CONFIG_BYTES`], not a regular file, not valid TOML, or a
    /// bad `[defaults]` value.
    pub fn load() -> Result<LoadedConfig, ConfigError> {
        let Some(path) = config_path() else {
            return Ok(LoadedConfig {
                config: Config::default(),
                skipped: Vec::new(),
                trust_problem: None,
            });
        };
        Config::load_from(&path)
    }

    /// [`Config::load`] against an explicit path, so the
    /// missing-versus-oversized-versus-unparseable trichotomy is testable
    /// without touching the real `~/.config` (audit finding T-01).
    pub fn load_from(path: &Path) -> Result<LoadedConfig, ConfigError> {
        let file = match std::fs::File::open(path) {
            Ok(file) => file,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return Ok(LoadedConfig {
                    config: Config::default(),
                    skipped: Vec::new(),
                    trust_problem: None,
                });
            }
            Err(e) => return Err(ConfigError::Io(e)),
        };
        let metadata = file.metadata().map_err(ConfigError::Io)?;
        // Checked before the read, not after: reading a FIFO or a
        // character device blocks forever, and this process is the login
        // session leader, so a read that never returns is a login that
        // never completes (audit finding C-07).
        if !metadata.file_type().is_file() {
            return Err(ConfigError::NotARegularFile);
        }
        let contents = read_bounded(file)?;
        let trust_problem = config_trust_problem(
            metadata.permissions().mode(),
            metadata.uid(),
            buoy_common::peer::own_uid(),
        );
        let (config, skipped) = Config::parse_lenient(&contents)?;
        Ok(LoadedConfig {
            config,
            skipped,
            trust_problem,
        })
    }
}

/// Reads at most [`MAX_CONFIG_BYTES`] from `file`, refusing anything
/// longer rather than reading it.
///
/// `take(MAX_CONFIG_BYTES + 1)` rather than a `metadata().len()` check:
/// the length a `stat` reports is not what a subsequent read returns for
/// anything that is growing while it is read, and the bound has to hold
/// against the bytes actually delivered.
fn read_bounded(file: std::fs::File) -> Result<String, ConfigError> {
    let mut contents = String::new();
    std::io::Read::take(file, MAX_CONFIG_BYTES + 1)
        .read_to_string(&mut contents)
        .map_err(ConfigError::Io)?;
    if contents.len() as u64 > MAX_CONFIG_BYTES {
        return Err(ConfigError::TooLarge);
    }
    Ok(contents)
}

/// The rules one `[[keybind]]` has to satisfy, in the order a reader would
/// check them. `seen` accumulates the modifier-plus-trigger labels already
/// accepted, so a duplicate is reported against the entry that came second.
fn validate_keybind(keybind: &Keybind, seen: &mut Vec<String>) -> Result<(), ConfigError> {
    if keybind.keysym().is_none() {
        return Err(ConfigError::UnknownKey(keybind.key.clone()));
    }
    if let Some(key) = shifted_without_shift(keybind) {
        return Err(ConfigError::ShiftedKeyWithoutShift(key.to_string()));
    }
    if keybind.action.is_pointer_only() {
        return Err(ConfigError::PointerOnlyAction(
            keybind.action.config_name().to_string(),
        ));
    }
    let label = binding_label(&keybind.mods, &keybind.key);
    if seen.contains(&label) {
        return Err(ConfigError::DuplicateBinding(label));
    }
    seen.push(label);
    Ok(())
}

/// As [`validate_keybind`], for a `[[mousebind]]`: only the duplicate rule
/// applies, since a button is always a real trigger and the pointer-only
/// actions are the ones that belong here.
fn validate_mousebind(mousebind: &Mousebind, seen: &mut Vec<String>) -> Result<(), ConfigError> {
    let label = binding_label(&mousebind.mods, mousebind.button.label());
    if seen.contains(&label) {
        return Err(ConfigError::DuplicateBinding(label));
    }
    seen.push(label);
    Ok(())
}

/// The rules one `[[input]]` has to satisfy. A misconfigured device is
/// invisible from the user's side — it just behaves as though the section
/// were never written — so an entry that can never apply is reported while
/// there is still a name to point at.
fn validate_input(input: &InputConfig, seen: &mut Vec<String>) -> Result<(), ConfigError> {
    if input.name.is_empty() {
        return Err(ConfigError::EmptyInputName);
    }
    if input.is_empty() {
        return Err(ConfigError::InputWithoutSettings(input.name.clone()));
    }
    if seen.contains(&input.name) {
        return Err(ConfigError::DuplicateInput(input.name.clone()));
    }
    seen.push(input.name.clone());
    // libinput rejects an out-of-range speed itself, but only at the point
    // the request is sent — one silent `invalid` result per device, long
    // after the file was read.
    if let Some(speed) = input.accel_speed
        && !(-1.0..=1.0).contains(&speed)
    {
        return Err(ConfigError::AccelSpeedOutOfRange(speed));
    }
    Ok(())
}

/// The `[defaults]` rules, none of which can be satisfied by skipping
/// something.
///
/// An empty program name would reach `Command::new("")` and fail once per
/// keypress; an empty tag name would be permanent (ADR-006 has no delete).
/// Without `{app_id}` a spawned terminal never carries the app-id every
/// other part of the WM identifies a pinned terminal by, so it would map
/// as an ordinary floating window, the tag's backdrop would never appear,
/// and the idempotent spawn claim means it would never be retried either.
/// An *empty* argv is the same failure wearing the face of an off switch,
/// so it is rejected with a message naming the real one (audit finding
/// C-10).
fn validate_defaults(defaults: &RawDefaults) -> Result<(), ConfigError> {
    for (field, value) in [
        ("terminal", &defaults.terminal),
        ("launcher", &defaults.launcher),
        ("default_tag", &defaults.default_tag),
    ] {
        if value.as_deref().is_some_and(str::is_empty) {
            return Err(ConfigError::EmptyDefault(field));
        }
    }
    if let Some(args) = &defaults.pinned_terminal_args {
        // A *non-empty* argv is judged the same way whether or not pinned
        // terminals are on: `{app_id}` is how a mapped window says which
        // tag it belongs to (audit finding D-02), and the off switch must
        // not become a way past that rule for a user who later turns the
        // feature back on.
        if !args.is_empty() {
            if !args.iter().any(|arg| arg.contains("{app_id}")) {
                return Err(ConfigError::MissingAppIdPlaceholder);
            }
        } else if defaults.pinned_terminals != Some(false) {
            return Err(ConfigError::EmptyPinnedTerminalArgs);
        }
    }
    Ok(())
}

impl Modifier {
    fn label(self) -> &'static str {
        match self {
            Modifier::Super => "Super",
            Modifier::Ctrl => "Ctrl",
            Modifier::Alt => "Alt",
            Modifier::Shift => "Shift",
        }
    }
}

impl Button {
    fn label(self) -> &'static str {
        match self {
            Button::Left => "LeftClick",
            Button::Right => "RightClick",
            Button::Middle => "MiddleClick",
        }
    }
}

impl Action {
    /// Whether this action drives a pointer drag and so only means
    /// something on a `[[mousebind]]`. Bound to a key, `move`/`resize`
    /// would start an op with no button held, and the protocol only ends
    /// one when all buttons are released — leaving the hovered window
    /// stuck to the cursor. The deleted hardcoded table enforced this
    /// structurally by only ever passing them to `create_pointer_binding`;
    /// with one shared action enum it has to be a check (code-review
    /// follow-up).
    ///
    /// Exhaustive rather than a `matches!`, so adding a variant forces its
    /// classification instead of defaulting it to key-bindable — the
    /// `matches!` form let a new pointer-only action be silently accepted
    /// on a `[[keybind]]` and produce exactly the stuck-window failure
    /// above (audit finding J-05).
    fn is_pointer_only(&self) -> bool {
        match self {
            Action::Move | Action::Resize => true,
            Action::Terminal
            | Action::Launcher
            | Action::Close
            | Action::FocusNext
            | Action::Exit
            | Action::CycleTag
            | Action::OpenAssignPicker
            | Action::OpenSwitchPicker
            | Action::Hotkeys
            | Action::Exec(_)
            | Action::SwitchTag(_) => false,
        }
    }

    /// This action's spelling in the config file, for error messages.
    fn config_name(&self) -> &'static str {
        match self {
            Action::Terminal => "terminal",
            Action::Launcher => "launcher",
            Action::Close => "close",
            Action::FocusNext => "focus_next",
            Action::Exit => "exit",
            Action::CycleTag => "cycle_tag",
            Action::OpenAssignPicker => "open_assign_picker",
            Action::OpenSwitchPicker => "open_switch_picker",
            Action::Hotkeys => "hotkeys",
            Action::Move => "move",
            Action::Resize => "resize",
            Action::Exec(_) => "exec",
            Action::SwitchTag(_) => "switch_tag",
        }
    }

    /// A human-readable description for the hotkey cheat-sheet. Takes
    /// [`Defaults`] because the spawn actions describe the program they
    /// will actually launch, which is configurable.
    fn description(&self, defaults: &Defaults) -> String {
        match self {
            Action::Terminal => format!("Spawn terminal ({})", defaults.terminal),
            Action::Launcher => format!("App launcher ({})", defaults.launcher),
            Action::Close => "Close focused window".to_string(),
            Action::FocusNext => "Cycle focus".to_string(),
            Action::Exit => "Exit session".to_string(),
            Action::CycleTag => "Cycle tag".to_string(),
            Action::OpenAssignPicker => "Tag manager (assign tags to focused window)".to_string(),
            Action::OpenSwitchPicker => "Switch tag (or type a new name to create)".to_string(),
            Action::Hotkeys => "Show this hotkey list".to_string(),
            Action::Move => "Move window".to_string(),
            Action::Resize => "Resize window".to_string(),
            Action::Exec(command) => format!("Run: {command}"),
            Action::SwitchTag(name) => format!("Switch to tag \"{name}\""),
        }
    }
}

fn binding_label(mods: &[Modifier], trigger: &str) -> String {
    let mut parts: Vec<&str> = mods.iter().map(|m| m.label()).collect();
    parts.push(trigger);
    parts.join("+")
}

impl Config {
    /// The `[[input]]` entry that applies to `device_name`, or `None` when
    /// the device is not configured and should keep libinput's defaults.
    ///
    /// First match wins, so the order entries were written in is the order
    /// that decides — that is what lets a specific device sit above a
    /// catch-all `name = "*"` and still be reachable.
    pub fn input_for(&self, device_name: &str) -> Option<&InputConfig> {
        self.inputs.iter().find(|input| input.matches(device_name))
    }

    /// The hotkey cheat-sheet, generated from the live bindings rather than
    /// hand-maintained alongside them — the previous fixed list had to be
    /// kept in sync by hand, which is exactly the drift a user-editable
    /// keymap makes unmanageable.
    pub fn hotkey_help(&self) -> Vec<String> {
        let labelled: Vec<(String, String)> = self
            .keybinds
            .iter()
            .map(|bind| {
                (
                    binding_label(&bind.mods, &bind.key),
                    bind.action.description(&self.defaults),
                )
            })
            .chain(self.mousebinds.iter().map(|bind| {
                (
                    binding_label(&bind.mods, bind.button.label()),
                    bind.action.description(&self.defaults),
                )
            }))
            .collect();
        // Pad to the widest label so the descriptions line up in fuzzel's
        // fixed-width list, rather than assuming a width that a user's own
        // longer binding would break.
        let width = labelled
            .iter()
            .map(|(label, _)| label.chars().count())
            .max()
            .unwrap_or(0);
        labelled
            .iter()
            .map(|(label, description)| format!("{label:<width$}  {description}"))
            .collect()
    }
}

/// `$XDG_CONFIG_HOME/buoy/config.toml`, or `~/.config/buoy/config.toml`
/// when that isn't usable.
///
/// Thin, untested (I/O-reading, not logic) wrapper that reads the real
/// environment variables; [`resolve_config_path`] holds the decision and
/// the guard that keeps this from ever returning a relative path.
pub fn config_path() -> Option<PathBuf> {
    resolve_config_path(
        std::env::var_os("XDG_CONFIG_HOME").as_deref(),
        std::env::var_os("HOME").as_deref(),
    )
}

/// Resolves the config file's path from explicit, injectable parameters —
/// the same "test the decision, not the I/O" split
/// [`resolve_socket_path`](buoy_common::socket_path::resolve_socket_path)
/// uses.
///
/// `None` when neither directory is usable, rather than a relative path.
/// Audit finding C-01: an earlier fix closed only the both-variables-unset
/// case, so a set-but-empty `XDG_CONFIG_HOME` still yielded the *relative*
/// `buoy/config.toml`, which [`Config::load`] would resolve against the
/// WM's working directory — a `buoy/config.toml` sitting in whatever
/// directory the session happened to launch from would be read and its
/// `exec` actions run through `sh -c`. Having no usable base means there is
/// no config, not "look here instead", which is why this returns `None`
/// rather than guessing.
pub fn resolve_config_path(
    xdg_config_home: Option<&OsStr>,
    home: Option<&OsStr>,
) -> Option<PathBuf> {
    let base = usable_base_dir(xdg_config_home)
        .map(PathBuf::from)
        .or_else(|| usable_base_dir(home).map(|home| PathBuf::from(home).join(".config")))?;
    Some(base.join("buoy").join("config.toml"))
}

// A set-but-empty or set-but-relative base (both real systemd/container
// patterns) must fall through rather than produce the relative
// `buoy/config.toml`, which `load` would resolve against the WM's working
// directory. The XDG Base Directory spec requires a relative value be
// ignored, so `is_absolute` covers both variants.
fn usable_base_dir(value: Option<&OsStr>) -> Option<&OsStr> {
    value.filter(|dir| Path::new(dir).is_absolute())
}

#[cfg(test)]
mod tests {
    /// Audit finding F-04: `ConfigError` implemented `std::error::Error`
    /// with an empty body, so it inherited the default `source() -> None`
    /// while `Toml` and `Io` demonstrably wrap real causes — anything that
    /// walks the chain silently got nothing.
    #[test]
    fn a_wrapping_config_error_exposes_the_cause_it_wraps() {
        use std::error::Error;
        let wrapped = super::ConfigError::Io(std::io::Error::other("disk gave up"));
        let source = wrapped.source().expect("Io wraps a real io::Error");
        assert!(source.to_string().contains("disk gave up"));
    }

    #[test]
    fn a_config_error_that_wraps_nothing_has_no_source() {
        use std::error::Error;
        assert!(
            super::ConfigError::MissingAppIdPlaceholder
                .source()
                .is_none()
        );
    }
    use super::*;

    /// Audit finding G-04. One typo used to discard the *entire* file:
    /// every other binding, every `[defaults]` value and every `[[input]]`
    /// block reverted to the built-ins, which for a user who had rebound
    /// `Super+Return` months ago restored a keymap they no longer knew.
    #[test]
    fn a_single_bad_keybind_is_skipped_rather_than_discarding_the_file() {
        let contents = r#"
[[keybind]]
mod = ["Super"]
key = "q"
action = "close"

[[keybind]]
mod = ["Super"]
key = "NoSuchKeysym"
action = "exit"

[[keybind]]
mod = ["Super"]
key = "n"
action = "focus_next"
"#;
        let (config, skipped) = Config::parse_lenient(contents).expect("only entries are invalid");
        assert_eq!(skipped.len(), 1);
        assert!(matches!(skipped[0], ConfigError::UnknownKey(_)));
        let keys: Vec<&str> = config.keybinds.iter().map(|k| k.key.as_str()).collect();
        assert_eq!(keys, vec!["q", "n"]);
    }

    #[test]
    fn a_bad_input_block_is_skipped_and_the_others_still_apply() {
        let contents = r#"
[[input]]
name = "good"
tap = true

[[input]]
name = ""
tap = true
"#;
        let (config, skipped) = Config::parse_lenient(contents).expect("only entries are invalid");
        assert_eq!(skipped.len(), 1);
        assert_eq!(config.inputs.len(), 1);
        assert_eq!(config.inputs[0].name, "good");
    }

    // --- Reading the file (audit findings C-07 and C-08) ---------------

    /// A temporary directory to write config files into, removed when the
    /// guard drops so a failing assertion does not litter `/tmp`. Same
    /// shape as `buoy_common::socket_path`'s test-local guard; there is no
    /// shared test-support target to hold one copy until this crate has a
    /// `[lib]` (audit finding T-04).
    struct TempDir(PathBuf);

    impl TempDir {
        fn new(label: &str) -> Self {
            static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
            let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let path = std::env::temp_dir()
                .join(format!("buoy-config-{label}-{}-{n}", std::process::id()));
            let _ = std::fs::remove_dir_all(&path);
            std::fs::create_dir_all(&path).expect("create test directory");
            Self(path)
        }

        /// Writes `contents` to a config file in this directory with an
        /// explicit mode, and returns its path.
        fn write_config(&self, contents: &str, mode: u32) -> PathBuf {
            use std::os::unix::fs::PermissionsExt;
            let path = self.0.join("config.toml");
            std::fs::write(&path, contents).expect("write test config");
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(mode))
                .expect("set test config mode");
            path
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    const A_MINIMAL_CONFIG: &str =
        "[[keybind]]\nmod = [\"super\"]\nkey = \"z\"\naction = \"close\"\n";

    #[test]
    fn a_missing_config_file_is_not_an_error() {
        let dir = TempDir::new("missing");
        let loaded = Config::load_from(&dir.0.join("config.toml"))
            .expect("running with no config file is the expected case");
        assert_eq!(loaded.config, Config::default());
        assert!(loaded.skipped.is_empty());
        assert_eq!(loaded.trust_problem, None);
    }

    /// Audit finding C-07: river `exec`s this binary as the session
    /// leader, so an unbounded read plus the parser's own allocation makes
    /// a runaway generator or a mistyped redirect a login loop rather than
    /// a recoverable error.
    #[test]
    fn a_config_file_over_the_size_cap_is_refused_before_it_is_parsed() {
        let dir = TempDir::new("oversize");
        let mut contents = String::from("# padding\n");
        while contents.len() as u64 <= MAX_CONFIG_BYTES {
            contents.push_str("# padding padding padding padding padding padding\n");
        }
        let path = dir.write_config(&contents, 0o600);
        let error = Config::load_from(&path).expect_err("an oversized file must be refused");
        assert!(matches!(error, ConfigError::TooLarge));
        assert!(error.to_string().contains("1 MiB"), "got: {error}");
    }

    /// The cap is a cap, not a rejection of large-but-plausible files: a
    /// heavily commented config right below it still loads.
    #[test]
    fn a_config_file_just_under_the_size_cap_still_loads() {
        let dir = TempDir::new("just-under");
        let mut contents = String::from(A_MINIMAL_CONFIG);
        let filler = "# padding padding padding padding padding padding\n";
        while (contents.len() + filler.len()) as u64 <= MAX_CONFIG_BYTES {
            contents.push_str(filler);
        }
        let path = dir.write_config(&contents, 0o600);
        let loaded = Config::load_from(&path).expect("a file under the cap must still load");
        assert_eq!(loaded.config.keybinds.len(), 1);
        assert_eq!(loaded.config.keybinds[0].key, "z");
    }

    /// A FIFO or a character device at the config path would otherwise
    /// block the read forever, which for the session leader is a login
    /// that never completes — strictly worse than falling back to the
    /// built-in defaults. A directory is the same rejection with no
    /// `mkfifo` needed.
    #[test]
    fn a_config_path_that_is_not_a_regular_file_is_refused() {
        let dir = TempDir::new("not-a-file");
        let path = dir.0.join("config.toml");
        std::fs::create_dir(&path).expect("create a directory at the config path");
        let error = Config::load_from(&path).expect_err("a non-regular file must be refused");
        assert!(matches!(error, ConfigError::NotARegularFile));
    }

    /// **The compatibility case.** A `644` config is what an ordinary
    /// `install`, editor save or `chezmoi apply` produces, and it is what
    /// the author's own machine has. Audit finding C-08 is about
    /// *writability* by other users, so a world-readable config must load
    /// with nothing said about it — a warning here would train the user to
    /// ignore the one that matters.
    #[test]
    fn a_mode_644_config_loads_with_no_warning() {
        let dir = TempDir::new("mode-644");
        let path = dir.write_config(A_MINIMAL_CONFIG, 0o644);
        let loaded = Config::load_from(&path).expect("a 644 config must load");
        assert_eq!(loaded.trust_problem, None);
        assert_eq!(loaded.config.keybinds.len(), 1);
        assert_eq!(loaded.config.keybinds[0].key, "z");
    }

    /// Audit finding C-08: this file grants arbitrary command execution
    /// through `{ exec = "..." }`, so a group- or world-writable one is a
    /// persistent foothold for any other local user. Warned about rather
    /// than refused, because refusing means falling back to built-in
    /// keybinds — a user locked out of the keymap they actually use is a
    /// worse outcome than one told to run `chmod`.
    #[test]
    fn a_group_writable_config_is_still_loaded_but_warned_about() {
        let dir = TempDir::new("group-writable");
        let path = dir.write_config(A_MINIMAL_CONFIG, 0o664);
        let loaded = Config::load_from(&path).expect("the file is still used");
        assert_eq!(
            loaded.trust_problem,
            Some(ConfigTrustProblem::WritableByOthers { mode: 0o664 })
        );
        assert_eq!(
            loaded.config.keybinds.len(),
            1,
            "the warning must not cost the user their keybinds"
        );
    }

    /// The decision, tabulated: only the `0o022` bits matter. Readability
    /// is not a finding, and neither is any owner permission.
    #[test]
    fn only_write_permission_for_group_or_others_is_a_trust_problem() {
        let own_uid = 1000;
        for mode in [0o600, 0o640, 0o644, 0o444, 0o700, 0o755, 0o400] {
            assert_eq!(
                config_trust_problem(mode, own_uid, own_uid),
                None,
                "mode {mode:o} grants no write access to another user"
            );
        }
        for mode in [0o620, 0o602, 0o622, 0o664, 0o646, 0o666, 0o777] {
            assert_eq!(
                config_trust_problem(mode, own_uid, own_uid),
                Some(ConfigTrustProblem::WritableByOthers { mode }),
                "mode {mode:o} lets another user rewrite the file"
            );
        }
    }

    /// A file this user cannot even fix is reported whatever its mode:
    /// `root`-owned with `0644` is a config the session leader executes
    /// and the session's own user cannot edit.
    #[test]
    fn a_config_owned_by_another_user_is_a_trust_problem_whatever_its_mode() {
        assert_eq!(
            config_trust_problem(0o644, 0, 1000),
            Some(ConfigTrustProblem::ForeignOwner {
                file_uid: 0,
                effective_uid: 1000,
            })
        );
        assert_eq!(config_trust_problem(0o644, 1000, 1000), None);
    }

    /// The message is the whole point of warning rather than refusing, so
    /// it has to name the mode and the command that fixes it.
    #[test]
    fn a_trust_warning_names_the_mode_and_how_to_fix_it() {
        let writable = ConfigTrustProblem::WritableByOthers { mode: 0o664 }.to_string();
        assert!(writable.contains("664"), "got: {writable}");
        assert!(writable.contains("chmod"), "got: {writable}");
        let foreign = ConfigTrustProblem::ForeignOwner {
            file_uid: 0,
            effective_uid: 1000,
        }
        .to_string();
        assert!(foreign.contains("uid 0"), "got: {foreign}");
        assert!(foreign.contains("1000"), "got: {foreign}");
    }

    /// A `[defaults]` mistake is not skippable: there is no "this entry"
    /// to drop, and substituting the built-in value silently would be the
    /// whole-file discard in miniature.
    #[test]
    fn a_defaults_error_is_still_fatal_under_leniency() {
        let contents = "[defaults]\nterminal = \"\"\n";
        let error = Config::parse_lenient(contents).expect_err("[defaults] must stay fatal");
        assert!(matches!(error, ConfigError::EmptyDefault("terminal")));
    }

    /// Four independent mistakes used to take four edit-and-restart cycles
    /// to find, one per restart.
    #[test]
    fn strict_parse_reports_every_offending_entry_at_once() {
        let contents = r#"
[[keybind]]
mod = ["Super"]
key = "NoSuchKeysym"
action = "exit"

[[keybind]]
mod = ["Super"]
key = "AlsoNotAKeysym"
action = "close"
"#;
        let error = Config::parse(contents).expect_err("two bad keybinds");
        let ConfigError::Multiple(errors) = &error else {
            panic!("expected Multiple, got {error:?}");
        };
        assert_eq!(errors.len(), 2);
        let rendered = error.to_string();
        assert!(rendered.contains("NoSuchKeysym"), "{rendered}");
        assert!(rendered.contains("AlsoNotAKeysym"), "{rendered}");
    }

    /// A file with exactly one mistake keeps reporting that mistake
    /// directly, so nothing has to unwrap a one-element list to find out
    /// what went wrong.
    #[test]
    fn strict_parse_reports_a_lone_error_unwrapped() {
        let contents =
            "[[keybind]]\nmod = [\"Super\"]\nkey = \"NoSuchKeysym\"\naction = \"exit\"\n";
        let error = Config::parse(contents).expect_err("one bad keybind");
        assert!(matches!(error, ConfigError::UnknownKey(_)));
    }

    /// Dropping every declared keybind lands on the documented
    /// "no `[[keybind]]` means the built-in set" rule rather than on a WM
    /// with no bindings at all.
    #[test]
    fn skipping_the_only_keybind_falls_back_to_the_built_in_set() {
        let contents =
            "[[keybind]]\nmod = [\"Super\"]\nkey = \"NoSuchKeysym\"\naction = \"exit\"\n";
        let (config, skipped) = Config::parse_lenient(contents).expect("only entries are invalid");
        assert_eq!(skipped.len(), 1);
        assert_eq!(config.keybinds, Config::default().keybinds);
    }

    /// `Action::Exec` carries the user's own shell command lines, which can
    /// carry tokens. No `{:?}` is applied to `Action`, `Keybind` or
    /// `Config` in non-test code today, so the hazard is one future
    /// `log_err!("config: {config:?}")` away.
    #[test]
    fn debug_for_action_redacts_the_exec_command_line() {
        let rendered = format!("{:?}", Action::Exec("secret --token abc123".to_string()));
        assert_eq!(rendered, "Exec(<redacted>)");
        assert!(!rendered.contains("abc123"));
    }

    #[test]
    fn debug_for_action_still_names_every_other_variant() {
        assert_eq!(format!("{:?}", Action::Terminal), "Terminal");
        assert_eq!(
            format!("{:?}", Action::SwitchTag("email".to_string())),
            r#"SwitchTag("email")"#
        );
    }

    /// A `Keybind` reaches `Action`'s own `Debug`, so the redaction has to
    /// survive being nested inside a derived one.
    #[test]
    fn debug_for_a_keybind_redacts_its_exec_action_too() {
        let keybind = Keybind {
            mods: Vec::new(),
            key: "p".to_string(),
            action: Action::Exec("secret --token abc123".to_string()),
        };
        assert!(!format!("{keybind:?}").contains("abc123"));
    }

    #[test]
    fn default_config_reproduces_the_previously_hardcoded_programs() {
        let config = Config::default();
        assert_eq!(config.defaults.terminal, "foot");
        assert_eq!(config.defaults.launcher, "fuzzel");
        assert_eq!(config.defaults.default_tag, "default");
    }

    /// The full built-in keybind set must survive having no config file:
    /// absence of configuration is not a request to lose every binding.
    #[test]
    fn default_config_keeps_every_built_in_keybind() {
        let config = Config::default();
        let actions: Vec<&Action> = config.keybinds.iter().map(|k| &k.action).collect();
        assert!(actions.contains(&&Action::Terminal));
        assert!(actions.contains(&&Action::Launcher));
        assert!(actions.contains(&&Action::Close));
        assert!(actions.contains(&&Action::FocusNext));
        assert!(actions.contains(&&Action::CycleTag));
        assert!(actions.contains(&&Action::OpenAssignPicker));
        assert!(actions.contains(&&Action::OpenSwitchPicker));
        assert!(actions.contains(&&Action::Hotkeys));
        assert!(actions.contains(&&Action::Exit));
        assert_eq!(config.mousebinds.len(), 2);
    }

    #[test]
    fn empty_config_file_is_valid_and_yields_the_defaults() {
        assert_eq!(Config::parse("").unwrap(), Config::default());
    }

    #[test]
    fn defaults_section_overrides_only_the_keys_it_names() {
        let config = Config::parse(
            r#"
            [defaults]
            terminal = "alacritty"
            "#,
        )
        .unwrap();
        assert_eq!(config.defaults.terminal, "alacritty");
        // Untouched keys keep their built-in values rather than resetting.
        assert_eq!(config.defaults.launcher, "fuzzel");
        assert_eq!(config.defaults.default_tag, "default");
    }

    /// Declaring any keybind replaces the built-in set outright. Merging
    /// instead would make a bind impossible to *remove*, and silently
    /// reintroduce defaults the user had deliberately rebound.
    #[test]
    fn declaring_keybinds_replaces_the_built_in_set() {
        let config = Config::parse(
            r#"
            [[keybind]]
            mod = ["Super"]
            key = "Space"
            action = "terminal"
            "#,
        )
        .unwrap();
        assert_eq!(config.keybinds.len(), 1);
        assert_eq!(config.keybinds[0].action, Action::Terminal);
        assert_eq!(config.keybinds[0].mods, vec![Modifier::Super]);
        assert_eq!(config.keybinds[0].key, "Space");
        // Mousebinds are a separate list and are NOT replaced by keybinds.
        assert_eq!(config.mousebinds.len(), 2);
    }

    #[test]
    fn parses_multiple_modifiers() {
        let config = Config::parse(
            r#"
            [[keybind]]
            mod = ["Super", "Shift"]
            key = "1"
            action = "close"
            "#,
        )
        .unwrap();
        assert_eq!(
            config.keybinds[0].mods,
            vec![Modifier::Super, Modifier::Shift]
        );
    }

    #[test]
    fn parses_every_modifier_spelling() {
        let config = Config::parse(
            r#"
            [[keybind]]
            mod = ["Super", "Ctrl", "Alt", "Shift"]
            key = "a"
            action = "close"
            "#,
        )
        .unwrap();
        assert_eq!(
            config.keybinds[0].mods,
            vec![
                Modifier::Super,
                Modifier::Ctrl,
                Modifier::Alt,
                Modifier::Shift
            ]
        );
    }

    /// Audit finding C-11 renamed these values to snake_case to match every
    /// other enum in the file. The PascalCase spellings the README and the
    /// shipped example advertised for two releases are what every existing
    /// config on disk contains, so they are aliases, not history — a
    /// rename that silently emptied a user's keymap would be the whole-file
    /// discard G-04 exists to prevent, arriving by a different route.
    #[test]
    fn every_previously_documented_modifier_and_button_spelling_still_parses() {
        let config = Config::parse(
            r#"
            [[keybind]]
            mod = ["Super", "Ctrl", "Alt", "Shift"]
            key = "a"
            action = "close"

            [[keybind]]
            mod = ["Mod4", "Control", "Mod1"]
            key = "b"
            action = "close"

            [[mousebind]]
            mod = ["Super"]
            button = "Left"
            action = "move"

            [[mousebind]]
            mod = ["Super"]
            button = "Right"
            action = "resize"

            [[mousebind]]
            mod = ["Super"]
            button = "Middle"
            action = "close"
            "#,
        )
        .expect("the previously documented spellings must keep working");
        assert_eq!(
            config.keybinds[0].mods,
            vec![
                Modifier::Super,
                Modifier::Ctrl,
                Modifier::Alt,
                Modifier::Shift
            ]
        );
        assert_eq!(
            config.keybinds[1].mods,
            vec![Modifier::Super, Modifier::Ctrl, Modifier::Alt]
        );
        assert_eq!(
            config
                .mousebinds
                .iter()
                .map(|bind| bind.button)
                .collect::<Vec<_>>(),
            vec![Button::Left, Button::Right, Button::Middle]
        );
    }

    /// The two casings have to produce the *same* config, not merely both
    /// parse: an alias that resolved to a different variant would be a
    /// keybind that fires on the wrong chord.
    #[test]
    fn snake_case_and_pascal_case_bindings_parse_to_the_same_config() {
        let snake_case = Config::parse(
            r#"
            [[keybind]]
            mod = ["super", "shift"]
            key = "a"
            action = "close"

            [[keybind]]
            mod = ["ctrl", "alt"]
            key = "b"
            action = "close"

            [[mousebind]]
            mod = ["super"]
            button = "middle"
            action = "move"
            "#,
        )
        .expect("snake_case is the documented spelling");
        let pascal_case = Config::parse(
            r#"
            [[keybind]]
            mod = ["Super", "Shift"]
            key = "a"
            action = "close"

            [[keybind]]
            mod = ["Ctrl", "Alt"]
            key = "b"
            action = "close"

            [[mousebind]]
            mod = ["Super"]
            button = "Middle"
            action = "move"
            "#,
        )
        .expect("PascalCase is what every config written before C-11 uses");
        assert_eq!(snake_case, pascal_case);
    }

    #[test]
    fn parses_a_parameterized_switch_tag_action() {
        let config = Config::parse(
            r#"
            [[keybind]]
            mod = ["Super"]
            key = "1"
            action = { switch_tag = "email" }
            "#,
        )
        .unwrap();
        assert_eq!(
            config.keybinds[0].action,
            Action::SwitchTag("email".to_string())
        );
    }

    #[test]
    fn parses_a_parameterized_exec_action() {
        let config = Config::parse(
            r#"
            [[keybind]]
            mod = ["Super"]
            key = "p"
            action = { exec = "grim -g slurp" }
            "#,
        )
        .unwrap();
        assert_eq!(
            config.keybinds[0].action,
            Action::Exec("grim -g slurp".to_string())
        );
    }

    /// Every action spelling this project has ever documented, asserted
    /// against the variant it must produce.
    ///
    /// Audit finding J-08 renamed `tag_picker`/`tag_switch` to
    /// `open_assign_picker`/`open_switch_picker`. Renaming a variant under
    /// `rename_all` silently changes the accepted config spelling, and a
    /// spelling that stops parsing does not fail loudly — `Config::parse`
    /// skips the offending `[[keybind]]`, so the user simply loses that
    /// binding at their next login. `#[serde(alias)]` keeps the old
    /// spellings, and this test is what stops a future rename from
    /// dropping one.
    #[test]
    fn every_documented_action_spelling_still_parses() {
        let parse_keybind = |action_toml: &str| -> Action {
            let contents =
                format!("[[keybind]]\nmod = [\"super\"]\nkey = \"F1\"\naction = {action_toml}\n");
            let config = Config::parse(&contents)
                .unwrap_or_else(|e| panic!("`action = {action_toml}` failed to parse: {e}"));
            // A rejected action is *skipped*, not an error, and an empty
            // keybind list falls back to the built-ins - so the length
            // assertion is what turns a silently-dropped binding into a
            // failure.
            assert_eq!(
                config.keybinds.len(),
                1,
                "`action = {action_toml}` was skipped rather than accepted"
            );
            config.keybinds[0].action.clone()
        };

        assert_eq!(parse_keybind(r#""terminal""#), Action::Terminal);
        assert_eq!(parse_keybind(r#""launcher""#), Action::Launcher);
        assert_eq!(parse_keybind(r#""close""#), Action::Close);
        assert_eq!(parse_keybind(r#""focus_next""#), Action::FocusNext);
        assert_eq!(parse_keybind(r#""exit""#), Action::Exit);
        assert_eq!(parse_keybind(r#""cycle_tag""#), Action::CycleTag);
        assert_eq!(parse_keybind(r#""hotkeys""#), Action::Hotkeys);
        assert_eq!(
            parse_keybind(r#"{ exec = "grim -g slurp" }"#),
            Action::Exec("grim -g slurp".to_string())
        );
        assert_eq!(
            parse_keybind(r#"{ switch_tag = "email" }"#),
            Action::SwitchTag("email".to_string())
        );

        // The J-08 renames: old spelling and new spelling, same variant.
        assert_eq!(parse_keybind(r#""tag_picker""#), Action::OpenAssignPicker);
        assert_eq!(
            parse_keybind(r#""open_assign_picker""#),
            Action::OpenAssignPicker
        );
        assert_eq!(parse_keybind(r#""tag_switch""#), Action::OpenSwitchPicker);
        assert_eq!(
            parse_keybind(r#""open_switch_picker""#),
            Action::OpenSwitchPicker
        );

        // `move`/`resize` are pointer-only, so they are only valid on a
        // `[[mousebind]]`; parsing them here would assert the rejection,
        // not the spelling.
        let pointer_only = Config::parse(
            r#"
            [[mousebind]]
            mod = ["Super"]
            button = "Left"
            action = "move"

            [[mousebind]]
            mod = ["Super"]
            button = "Right"
            action = "resize"
            "#,
        )
        .unwrap();
        assert_eq!(pointer_only.mousebinds[0].action, Action::Move);
        assert_eq!(pointer_only.mousebinds[1].action, Action::Resize);
    }

    #[test]
    fn parses_mousebinds() {
        let config = Config::parse(
            r#"
            [[mousebind]]
            mod = ["Super"]
            button = "Right"
            action = "resize"
            "#,
        )
        .unwrap();
        assert_eq!(config.mousebinds.len(), 1);
        assert_eq!(config.mousebinds[0].button, Button::Right);
        assert_eq!(config.mousebinds[0].action, Action::Resize);
    }

    #[test]
    fn rejects_an_unknown_modifier() {
        let err = Config::parse(
            r#"
            [[keybind]]
            mod = ["Hyper"]
            key = "a"
            action = "close"
            "#,
        )
        .unwrap_err();
        assert!(err.to_string().contains("Hyper"), "got: {err}");
    }

    #[test]
    fn rejects_an_unknown_action() {
        let err = Config::parse(
            r#"
            [[keybind]]
            mod = ["Super"]
            key = "a"
            action = "self_destruct"
            "#,
        )
        .unwrap_err();
        assert!(err.to_string().contains("self_destruct"), "got: {err}");
    }

    /// A typo in a key name must fail at load, not silently register a
    /// binding that can never fire.
    #[test]
    fn rejects_a_key_name_that_is_not_a_known_keysym() {
        let err = Config::parse(
            r#"
            [[keybind]]
            mod = ["Super"]
            key = "Retrun"
            action = "close"
            "#,
        )
        .unwrap_err();
        assert!(err.to_string().contains("Retrun"), "got: {err}");
    }

    /// Code-review follow-up. An uppercase letter IS the shifted keysym,
    /// so a `Super`-only binding on it can never match: pressing Super+q
    /// reports the unshifted keysym, and Super+Shift+q reports the shifted
    /// one with `Shift` in the modifier set. Neither matches `Super`+`Q`.
    /// This shipped in the example config and silently killed six binds,
    /// so it is rejected at load rather than left to fail invisibly.
    #[test]
    fn rejects_an_uppercase_letter_without_shift() {
        let err = Config::parse(
            r#"
            [[keybind]]
            mod = ["Super"]
            key = "Q"
            action = "close"
            "#,
        )
        .unwrap_err();
        let message = err.to_string();
        assert!(message.contains('Q'), "got: {message}");
        assert!(message.contains("Shift"), "got: {message}");
    }

    #[test]
    fn accepts_an_uppercase_letter_when_shift_is_held() {
        let config = Config::parse(
            r#"
            [[keybind]]
            mod = ["Super", "Shift"]
            key = "B"
            action = "close"
            "#,
        )
        .unwrap();
        assert_eq!(config.keybinds.len(), 1);
    }

    /// Only single ASCII letters are judged — a *named* key is not a
    /// shifted symbol however it is capitalized, and non-letter symbols
    /// are layout-dependent (`?` needs Shift on a US layout but the rule
    /// does not hold universally), so neither is rejected.
    #[test]
    fn uppercase_rule_does_not_touch_named_keys_or_symbols() {
        assert!(
            Config::parse(
                r#"
                [[keybind]]
                mod = ["Super"]
                key = "Tab"
                action = "close"
                "#,
            )
            .is_ok()
        );
        assert!(
            Config::parse(
                r#"
                [[keybind]]
                mod = ["Super"]
                key = "F5"
                action = "close"
                "#,
            )
            .is_ok()
        );
    }

    /// The built-in set and the shipped example must both satisfy the rule
    /// they are validated by — this is the guard that would have caught the
    /// original defect.
    #[test]
    fn built_in_defaults_and_shipped_example_have_no_shift_less_uppercase() {
        for keybind in &Config::default().keybinds {
            assert!(
                shifted_without_shift(keybind).is_none(),
                "built-in bind on `{}` needs Shift",
                keybind.key
            );
        }
        let example = include_str!("../../../docs/config.example.toml");
        Config::parse(example).expect("shipped example must satisfy its own validation");
    }

    #[test]
    fn default_pinned_terminal_args_are_foots_existing_invocation() {
        let config = Config::default();
        assert_eq!(
            config.defaults.pinned_terminal_args,
            vec![
                "-a",
                "{app_id}",
                "zellij",
                "attach",
                "--create",
                "{session}"
            ]
        );
    }

    /// `{app_id}` is substituted with the *per-tag* identity the WM
    /// recognises a pinned terminal by (`pinned-term-<tag id>`, audit
    /// finding D-02) — the placeholder and its position in the user's argv
    /// are unchanged, only the value substituted into it.
    #[test]
    fn pinned_terminal_argv_substitutes_both_placeholders() {
        let config = Config::default();
        assert_eq!(
            config
                .defaults
                .pinned_terminal_argv("pinned-term-3", "tag-email"),
            vec![
                "-a",
                "pinned-term-3",
                "zellij",
                "attach",
                "--create",
                "tag-email"
            ]
        );
    }

    /// Code-review follow-up. Setting `terminal` to anything that doesn't
    /// take foot's `-a` used to break every tag's pinned terminal
    /// silently *and* permanently: the spawn succeeds, the tag is marked
    /// spawned, no window with the right app-id ever maps, and the claim
    /// is idempotent so it is never retried. Making the argv configurable
    /// is the fix; requiring `{app_id}` keeps the recognisable-app-id
    /// invariant that the rest of the WM depends on.
    #[test]
    fn rejects_pinned_terminal_args_without_the_app_id_placeholder() {
        let err = Config::parse(
            r#"
            [defaults]
            terminal = "alacritty"
            pinned_terminal_args = ["zellij", "attach", "--create", "{session}"]
            "#,
        )
        .unwrap_err();
        assert!(err.to_string().contains("{app_id}"), "got: {err}");
    }

    /// Audit finding C-10: the riskiest subsystem in the process that *is*
    /// the login session had no off switch, and the obvious guess —
    /// emptying the argv — was rejected at load, which under G-04's
    /// `[defaults]` rule threw the user's whole config away.
    #[test]
    fn pinned_terminals_are_on_unless_the_config_turns_them_off() {
        assert!(Config::default().defaults.pinned_terminals);
        let unmentioned = Config::parse("[defaults]\nterminal = \"foot\"\n").unwrap();
        assert!(
            unmentioned.defaults.pinned_terminals,
            "a config that does not mention the switch must keep the old behaviour"
        );
        let off = Config::parse("[defaults]\npinned_terminals = false\n").unwrap();
        assert!(!off.defaults.pinned_terminals);
    }

    /// The off switch is what makes an empty argv sayable at all, and the
    /// error has to point at it — a user whose terminal or `zellij` is
    /// broken reaches for the argv first.
    #[test]
    fn an_empty_pinned_terminal_argv_is_rejected_and_names_the_off_switch() {
        let err = Config::parse(
            r#"
            [defaults]
            pinned_terminal_args = []
            "#,
        )
        .unwrap_err();
        assert!(matches!(err, ConfigError::EmptyPinnedTerminalArgs));
        assert!(err.to_string().contains("pinned_terminals"), "got: {err}");
    }

    #[test]
    fn an_empty_pinned_terminal_argv_is_accepted_once_the_feature_is_off() {
        let config = Config::parse(
            r#"
            [defaults]
            pinned_terminals = false
            pinned_terminal_args = []
            "#,
        )
        .expect("with nothing to spawn, the argv describes nothing");
        assert!(!config.defaults.pinned_terminals);
        assert!(config.defaults.pinned_terminal_args.is_empty());
    }

    /// The off switch must not become a way to smuggle an unrecognisable
    /// pinned terminal past the `{app_id}` rule: a non-empty argv is
    /// judged on its own terms, feature on or off, because `{app_id}` is
    /// how a mapped window says which tag it belongs to (audit finding
    /// D-02).
    #[test]
    fn a_non_empty_argv_without_the_placeholder_is_rejected_even_with_the_feature_off() {
        let err = Config::parse(
            r#"
            [defaults]
            pinned_terminals = false
            pinned_terminal_args = ["zellij", "attach"]
            "#,
        )
        .unwrap_err();
        assert!(matches!(err, ConfigError::MissingAppIdPlaceholder));
    }

    #[test]
    fn accepts_a_non_foot_pinned_terminal_invocation() {
        let config = Config::parse(
            r#"
            [defaults]
            terminal = "alacritty"
            pinned_terminal_args = ["--class", "{app_id}", "-e", "zellij", "attach", "--create", "{session}"]
            "#,
        )
        .unwrap();
        assert_eq!(
            config
                .defaults
                .pinned_terminal_argv("pinned-term", "tag-web"),
            vec![
                "--class",
                "pinned-term",
                "-e",
                "zellij",
                "attach",
                "--create",
                "tag-web"
            ]
        );
    }

    #[test]
    fn rejects_malformed_toml() {
        assert!(Config::parse("[[keybind]\nkey =").is_err());
    }

    /// The cheat-sheet is generated from the live bindings precisely so it
    /// cannot drift from them, which a hand-maintained list always
    /// eventually does.
    #[test]
    fn hotkey_help_covers_every_binding() {
        let config = Config::default();
        let help = config.hotkey_help();
        assert_eq!(help.len(), config.keybinds.len() + config.mousebinds.len());
    }

    #[test]
    fn hotkey_help_names_the_modifiers_key_and_action() {
        let config = Config::parse(
            r#"
            [[keybind]]
            mod = ["Super", "Shift"]
            key = "Q"
            action = "close"
            "#,
        )
        .unwrap();
        let line = &config.hotkey_help()[0];
        assert!(line.contains("Super+Shift+Q"), "got: {line}");
        assert!(line.contains("Close focused window"), "got: {line}");
    }

    /// A parameterized action's payload is the only thing distinguishing
    /// two otherwise-identical binds, so the help has to show it.
    #[test]
    fn hotkey_help_shows_parameterized_action_payloads() {
        let config = Config::parse(
            r#"
            [[keybind]]
            mod = ["Super"]
            key = "1"
            action = { switch_tag = "email" }

            [[keybind]]
            mod = ["Super"]
            key = "p"
            action = { exec = "grim -g slurp" }
            "#,
        )
        .unwrap();
        let help = config.hotkey_help();
        assert!(help[0].contains("email"), "got: {}", help[0]);
        assert!(help[1].contains("grim -g slurp"), "got: {}", help[1]);
    }

    /// The spawn actions name the program they will actually launch, which
    /// is a config value, not a constant.
    #[test]
    fn hotkey_help_reflects_the_configured_programs() {
        let config = Config::parse(
            r#"
            [defaults]
            terminal = "alacritty"

            [[keybind]]
            mod = ["Super"]
            key = "Return"
            action = "terminal"
            "#,
        )
        .unwrap();
        assert!(
            config.hotkey_help()[0].contains("alacritty"),
            "got: {}",
            config.hotkey_help()[0]
        );
    }

    #[test]
    fn hotkey_help_labels_mouse_buttons_as_clicks() {
        let config = Config::default();
        let help = config.hotkey_help();
        assert!(
            help.iter().any(|line| line.contains("Super+LeftClick")),
            "got: {help:?}"
        );
    }

    /// The shipped example is documentation users copy verbatim, so it has
    /// to stay parseable by the code it documents — including every action
    /// spelling it advertises.
    #[test]
    fn the_shipped_example_config_parses() {
        let example = include_str!("../../../docs/config.example.toml");
        let config = Config::parse(example).expect("docs/config.example.toml must parse");
        assert!(config.keybinds.iter().any(|bind| matches!(
            &bind.action,
            Action::SwitchTag(name) if name == "email"
        )));
        assert!(
            config
                .keybinds
                .iter()
                .any(|bind| matches!(&bind.action, Action::Exec(_)))
        );
        assert_eq!(config.mousebinds.len(), 2);
    }

    #[test]
    fn keybind_resolves_its_key_name_to_a_keysym() {
        let config = Config::parse(
            r#"
            [[keybind]]
            mod = ["Super"]
            key = "Return"
            action = "terminal"
            "#,
        )
        .unwrap();
        assert_eq!(config.keybinds[0].keysym().unwrap(), 0xff0d);
    }

    // --- [[input]] ---------------------------------------------------------

    /// The whole reason `[[input]]` exists: with no config file at all, a
    /// laptop touchpad must have tap-to-click on. libinput defaults it to
    /// off for any device with physical buttons, and buoy previously never
    /// configured input devices, so tap silently did nothing.
    #[test]
    fn default_config_enables_tap_on_touchpads() {
        let config = Config::default();
        let touchpad = config
            .input_for("PIXA3854:00 093A:0274 Touchpad")
            .expect("a built-in [[input]] entry must match a touchpad");
        assert_eq!(touchpad.tap, Some(true));
    }

    /// The built-in entry must be narrow enough not to reconfigure a
    /// keyboard or an external mouse as a side effect.
    #[test]
    fn default_input_entry_does_not_match_non_touchpad_devices() {
        let config = Config::default();
        assert!(config.input_for("AT Translated Set 2 keyboard").is_none());
        assert!(config.input_for("Logitech USB Receiver Mouse").is_none());
    }

    #[test]
    fn parses_an_input_section_with_every_supported_setting() {
        let config = Config::parse(
            r#"
            [[input]]
            name = "*Touchpad*"
            tap = true
            tap_button_map = "lmr"
            click_method = "clickfinger"
            natural_scroll = true
            disable_while_typing = false
            accel_speed = 0.3
            "#,
        )
        .unwrap();
        assert_eq!(config.inputs.len(), 1);
        let input = &config.inputs[0];
        assert_eq!(input.name, "*Touchpad*");
        assert_eq!(input.tap, Some(true));
        assert_eq!(input.tap_button_map, Some(TapButtonMap::Lmr));
        assert_eq!(input.click_method, Some(ClickMethod::Clickfinger));
        assert_eq!(input.natural_scroll, Some(true));
        assert_eq!(input.disable_while_typing, Some(false));
        assert_eq!(input.accel_speed, Some(0.3));
    }

    /// An omitted setting must stay `None` rather than picking up the
    /// built-in entry's value: `None` is what tells the WM to send no
    /// request at all and leave libinput's own default in place, so
    /// defaulting it to anything would silently override the device.
    #[test]
    fn omitted_input_settings_stay_unset() {
        let config = Config::parse(
            r#"
            [[input]]
            name = "*Touchpad*"
            natural_scroll = true
            "#,
        )
        .unwrap();
        let input = &config.inputs[0];
        assert_eq!(input.natural_scroll, Some(true));
        assert_eq!(input.tap, None);
        assert_eq!(input.tap_button_map, None);
        assert_eq!(input.click_method, None);
        assert_eq!(input.disable_while_typing, None);
        assert_eq!(input.accel_speed, None);
    }

    /// Same replace-not-merge rule as `[[keybind]]`/`[[mousebind]]`: the
    /// list is taken wholesale so an unwanted built-in entry can actually
    /// be removed.
    #[test]
    fn declaring_any_input_replaces_the_built_in_input_list() {
        let config = Config::parse(
            r#"
            [[input]]
            name = "Some Trackball"
            natural_scroll = true
            "#,
        )
        .unwrap();
        assert_eq!(config.inputs.len(), 1);
        assert_eq!(config.inputs[0].name, "Some Trackball");
        assert!(config.input_for("PIXA3854:00 093A:0274 Touchpad").is_none());
    }

    /// `[[input]]` is its own list, independent of the binding lists —
    /// rebinding a key must not cost you your touchpad settings.
    #[test]
    fn declaring_keybinds_leaves_the_built_in_inputs_alone() {
        let config = Config::parse(
            r#"
            [[keybind]]
            mod = ["Super"]
            key = "Return"
            action = "terminal"
            "#,
        )
        .unwrap();
        assert_eq!(config.inputs, Config::default().inputs);
    }

    #[test]
    fn input_entry_matches_device_names_by_wildcard() {
        let config = Config::parse(
            r#"
            [[input]]
            name = "*Touchpad*"
            tap = true
            "#,
        )
        .unwrap();
        assert!(config.inputs[0].matches("PIXA3854:00 093A:0274 Touchpad"));
        assert!(!config.inputs[0].matches("Some Keyboard"));
    }

    /// First match wins, so the order the user wrote is the order that
    /// decides — a specific entry above a catch-all has to be reachable.
    #[test]
    fn input_for_returns_the_first_matching_entry() {
        let config = Config::parse(
            r#"
            [[input]]
            name = "*Touchpad*"
            tap = true

            [[input]]
            name = "*"
            tap = false
            "#,
        )
        .unwrap();
        assert_eq!(
            config
                .input_for("PIXA3854:00 093A:0274 Touchpad")
                .unwrap()
                .tap,
            Some(true)
        );
        assert_eq!(config.input_for("Some Keyboard").unwrap().tap, Some(false));
    }

    /// An empty pattern matches no real device, so the entry could never
    /// fire — the same "reject at load rather than silently do nothing"
    /// rule the binding checks follow.
    #[test]
    fn input_with_an_empty_name_is_rejected() {
        let err = Config::parse(
            r#"
            [[input]]
            name = ""
            tap = true
            "#,
        )
        .unwrap_err();
        assert!(matches!(err, ConfigError::EmptyInputName));
    }

    /// An entry that names a device but sets nothing reads like it does
    /// something and does not.
    #[test]
    fn input_with_no_settings_is_rejected() {
        let err = Config::parse(
            r#"
            [[input]]
            name = "*Touchpad*"
            "#,
        )
        .unwrap_err();
        assert!(matches!(err, ConfigError::InputWithoutSettings(name) if name == "*Touchpad*"));
    }

    /// Two entries for the same pattern means the second can never win
    /// under first-match-wins.
    #[test]
    fn duplicate_input_name_is_rejected() {
        let err = Config::parse(
            r#"
            [[input]]
            name = "*Touchpad*"
            tap = true

            [[input]]
            name = "*Touchpad*"
            natural_scroll = true
            "#,
        )
        .unwrap_err();
        assert!(matches!(err, ConfigError::DuplicateInput(name) if name == "*Touchpad*"));
    }

    #[test]
    fn accel_speed_outside_libinputs_range_is_rejected() {
        for speed in ["1.5", "-1.5"] {
            let contents = format!(
                r#"
                [[input]]
                name = "*Touchpad*"
                accel_speed = {speed}
                "#
            );
            let err = Config::parse(&contents).unwrap_err();
            assert!(
                matches!(err, ConfigError::AccelSpeedOutOfRange(_)),
                "{speed} should have been rejected, got {err:?}"
            );
        }
    }

    /// libinput's range is inclusive at both ends, so the extremes must be
    /// accepted rather than caught by an off-by-one bound.
    #[test]
    fn accel_speed_at_the_range_boundaries_is_accepted() {
        for speed in ["-1.0", "0.0", "1.0"] {
            let contents = format!(
                r#"
                [[input]]
                name = "*Touchpad*"
                accel_speed = {speed}
                "#
            );
            assert!(
                Config::parse(&contents).is_ok(),
                "{speed} should have been accepted"
            );
        }
    }

    #[test]
    fn unknown_input_field_is_rejected_rather_than_ignored() {
        let err = Config::parse(
            r#"
            [[input]]
            name = "*Touchpad*"
            tap_to_click = true
            "#,
        )
        .unwrap_err();
        assert!(matches!(err, ConfigError::Toml(_)));
    }

    #[test]
    fn input_enum_spellings_are_the_snake_case_libinput_names() {
        let config = Config::parse(
            r#"
            [[input]]
            name = "a"
            tap_button_map = "lrm"
            click_method = "button_areas"
            "#,
        )
        .unwrap();
        assert_eq!(config.inputs[0].tap_button_map, Some(TapButtonMap::Lrm));
        assert_eq!(
            config.inputs[0].click_method,
            Some(ClickMethod::ButtonAreas)
        );
    }

    #[test]
    fn click_method_none_disables_physical_clicks_explicitly() {
        let config = Config::parse(
            r#"
            [[input]]
            name = "a"
            click_method = "none"
            "#,
        )
        .unwrap();
        assert_eq!(config.inputs[0].click_method, Some(ClickMethod::None));
    }

    #[test]
    fn settings_lists_only_what_the_entry_actually_sets() {
        let config = Config::parse(
            r#"
            [[input]]
            name = "*Touchpad*"
            tap = true
            "#,
        )
        .unwrap();
        assert_eq!(
            config.inputs[0].settings(),
            vec![LibinputSetting::Tap(true)]
        );
    }

    /// The distinction the `Option` exists for: `tap = false` must produce a
    /// request that actively disables tap, not vanish like an omitted key.
    #[test]
    fn an_explicit_false_still_produces_a_setting() {
        let config = Config::parse(
            r#"
            [[input]]
            name = "*Touchpad*"
            tap = false
            natural_scroll = false
            "#,
        )
        .unwrap();
        assert_eq!(
            config.inputs[0].settings(),
            vec![
                LibinputSetting::Tap(false),
                LibinputSetting::NaturalScroll(false),
            ]
        );
    }

    #[test]
    fn settings_covers_every_supported_field() {
        let config = Config::parse(
            r#"
            [[input]]
            name = "*Touchpad*"
            tap = true
            tap_button_map = "lmr"
            click_method = "clickfinger"
            natural_scroll = true
            disable_while_typing = true
            accel_speed = -0.5
            "#,
        )
        .unwrap();
        assert_eq!(
            config.inputs[0].settings(),
            vec![
                LibinputSetting::Tap(true),
                LibinputSetting::TapButtonMap(TapButtonMap::Lmr),
                LibinputSetting::ClickMethod(ClickMethod::Clickfinger),
                LibinputSetting::NaturalScroll(true),
                LibinputSetting::DisableWhileTyping(true),
                LibinputSetting::AccelSpeed(-0.5),
            ]
        );
    }

    /// `tap_button_map` only means anything once tap is on, so it has to be
    /// sent after `tap` — libinput keeps the two independently, and applying
    /// them out of order on a device that starts with tap off is the kind of
    /// thing that works by luck rather than by design.
    #[test]
    fn tap_is_applied_before_its_button_map() {
        let config = Config::parse(
            r#"
            [[input]]
            name = "*Touchpad*"
            tap_button_map = "lmr"
            tap = true
            "#,
        )
        .unwrap();
        let settings = config.inputs[0].settings();
        let tap = settings
            .iter()
            .position(|s| matches!(s, LibinputSetting::Tap(_)))
            .unwrap();
        let map = settings
            .iter()
            .position(|s| matches!(s, LibinputSetting::TapButtonMap(_)))
            .unwrap();
        assert!(tap < map, "{settings:?}");
    }

    /// A built-in default that produced no settings would be a silent no-op.
    #[test]
    fn the_built_in_touchpad_entry_produces_the_tap_setting() {
        let config = Config::default();
        let touchpad = config.input_for("PIXA3854:00 093A:0274 Touchpad").unwrap();
        assert_eq!(touchpad.settings(), vec![LibinputSetting::Tap(true)]);
    }

    #[test]
    fn resolve_config_path_is_none_when_neither_variable_is_set() {
        assert_eq!(resolve_config_path(None, None), None);
    }

    #[test]
    fn resolve_config_path_treats_empty_xdg_config_home_as_unset() {
        // A set-but-empty XDG_CONFIG_HOME (a real systemd/container
        // pattern) must not join onto an empty base and yield the relative
        // "buoy/config.toml", which `load` would resolve against the WM's
        // working directory.
        assert_eq!(resolve_config_path(Some(OsStr::new("")), None), None);
    }

    #[test]
    fn resolve_config_path_rejects_a_relative_xdg_config_home() {
        // The XDG Base Directory spec requires a relative value be ignored.
        assert_eq!(
            resolve_config_path(Some(OsStr::new("relative")), None),
            None
        );
    }

    #[test]
    fn resolve_config_path_falls_back_to_home_when_xdg_config_home_is_empty() {
        assert_eq!(
            resolve_config_path(Some(OsStr::new("")), Some(OsStr::new("/home/x"))),
            Some(PathBuf::from("/home/x/.config/buoy/config.toml"))
        );
    }

    #[test]
    fn resolve_config_path_prefers_xdg_config_home_over_home() {
        assert_eq!(
            resolve_config_path(Some(OsStr::new("/xdg")), Some(OsStr::new("/home/x"))),
            Some(PathBuf::from("/xdg/buoy/config.toml"))
        );
    }

    #[test]
    fn is_empty_agrees_with_settings_for_every_single_field_entry() {
        // Audit finding C-09: `is_empty` used to be a hand-written logical
        // inverse of `settings()`, so a field added to one and forgotten in
        // the other failed silently — either never applied, or an entry
        // setting only the new field rejected at load as "empty".
        let blank = InputConfig {
            name: "*".to_string(),
            tap: None,
            tap_button_map: None,
            click_method: None,
            natural_scroll: None,
            disable_while_typing: None,
            accel_speed: None,
        };
        assert!(blank.is_empty());
        assert_eq!(blank.settings(), vec![]);

        let one_field_each = [
            InputConfig {
                tap: Some(true),
                ..blank.clone()
            },
            InputConfig {
                tap_button_map: Some(TapButtonMap::Lrm),
                ..blank.clone()
            },
            InputConfig {
                click_method: Some(ClickMethod::Clickfinger),
                ..blank.clone()
            },
            InputConfig {
                natural_scroll: Some(true),
                ..blank.clone()
            },
            InputConfig {
                disable_while_typing: Some(true),
                ..blank.clone()
            },
            InputConfig {
                accel_speed: Some(0.5),
                ..blank.clone()
            },
        ];
        for entry in &one_field_each {
            assert!(!entry.is_empty(), "{entry:?} sets a field but reads empty");
            assert_eq!(
                entry.settings().len(),
                1,
                "{entry:?} sets one field but implies {} settings",
                entry.settings().len()
            );
        }
    }
}
