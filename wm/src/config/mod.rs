// SPDX-FileCopyrightText: © 2026 Nick Booth
// SPDX-License-Identifier: 0BSD

//! User configuration: `~/.config/buoy/config.toml`.
//!
//! Every value this module exposes was a compile-time constant before it
//! existed, so [`Config::default`] reproduces the previously-hardcoded
//! behavior exactly — a missing or empty config file is not an error and
//! changes nothing.

pub mod keysym;

use serde::Deserialize;
use std::fmt;
use std::path::PathBuf;

/// A modifier key a binding can require. Aliases accept the spellings the
/// X11/xkb world uses interchangeably, so a user writing `Mod4` or
/// `Control` isn't told their config is wrong.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
pub enum Modifier {
    #[serde(alias = "Mod4", alias = "Logo", alias = "Win")]
    Super,
    #[serde(alias = "Control")]
    Ctrl,
    #[serde(alias = "Mod1")]
    Alt,
    Shift,
}

/// A pointer button a binding can require.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
pub enum Button {
    Left,
    Right,
    Middle,
}

/// What a binding does when it fires.
///
/// Serde's default externally-tagged representation is what lets a unit
/// variant be written as a bare string (`action = "close"`) and a
/// parameterized one as a single-key table (`action = { exec = "..." }`) in
/// the same field.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
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
    TagPicker,
    /// Open the tag-switch picker for the active output.
    TagSwitch,
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
    /// [`Config::pinned_terminal_argv`]). Configurable because the flag
    /// that sets a window's app-id is terminal-specific — foot spells it
    /// `-a`, most others `--class` — and hardcoding foot's spelling made
    /// setting `terminal` to anything else break every pinned terminal
    /// silently and permanently.
    pub pinned_terminal_args: Vec<String>,
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
    /// name isn't recognized. [`Config::parse`] rejects the latter at load,
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

/// The parsed contents of `~/.config/buoy/config.toml`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    pub defaults: Defaults,
    pub keybinds: Vec<Keybind>,
    pub mousebinds: Vec<Mousebind>,
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
    Io(std::io::Error),
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
            ConfigError::Io(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for ConfigError {}

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
                keybind("a", Action::TagPicker),
                keybind("s", Action::TagSwitch),
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
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawDefaults {
    terminal: Option<String>,
    launcher: Option<String>,
    default_tag: Option<String>,
    pinned_terminal_args: Option<Vec<String>>,
}

impl Config {
    /// Parses config file contents.
    ///
    /// Declaring any `[[keybind]]` replaces the built-in keybind set
    /// outright rather than merging into it — merging would leave a
    /// built-in binding impossible to remove, and would silently
    /// reintroduce a default the user had deliberately rebound elsewhere.
    /// `[[mousebind]]` is an independent list with the same rule.
    pub fn parse(contents: &str) -> Result<Self, ConfigError> {
        let raw: RawConfig = toml::from_str(contents).map_err(ConfigError::Toml)?;
        let built_in = Config::default();
        // Every check below rejects at load, naming the offender, rather
        // than letting a binding register and then silently never fire —
        // the failure mode is invisible from the user's side, so it has to
        // be caught while there is still something to point at.
        let mut seen: Vec<String> = Vec::new();
        for keybind in &raw.keybinds {
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
        }
        let mut seen_buttons: Vec<String> = Vec::new();
        for mousebind in &raw.mousebinds {
            let label = binding_label(&mousebind.mods, mousebind.button.label());
            if seen_buttons.contains(&label) {
                return Err(ConfigError::DuplicateBinding(label));
            }
            seen_buttons.push(label);
        }
        // An empty string here would reach `Command::new("")` and fail once
        // per keypress; an empty tag name would be permanent (ADR-006 has
        // no delete). Both are far better reported once, at load.
        for (field, value) in [
            ("terminal", &raw.defaults.terminal),
            ("launcher", &raw.defaults.launcher),
            ("default_tag", &raw.defaults.default_tag),
        ] {
            if value.as_deref().is_some_and(str::is_empty) {
                return Err(ConfigError::EmptyDefault(field));
            }
        }
        // Without `{app_id}` the spawned terminal never carries the app-id
        // every other part of the WM identifies the pinned terminal by, so
        // it would map as an ordinary floating window and the tag's
        // backdrop would never appear — and the spawn claim is idempotent,
        // so it would never be retried either.
        if let Some(args) = &raw.defaults.pinned_terminal_args
            && !args.iter().any(|arg| arg.contains("{app_id}"))
        {
            return Err(ConfigError::MissingAppIdPlaceholder);
        }
        Ok(Self {
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
            },
            keybinds: if raw.keybinds.is_empty() {
                built_in.keybinds
            } else {
                raw.keybinds
            },
            mousebinds: if raw.mousebinds.is_empty() {
                built_in.mousebinds
            } else {
                raw.mousebinds
            },
        })
    }

    /// Loads [`config_path`]'s file, falling back to [`Config::default`]
    /// when it doesn't exist — running without a config file is the
    /// expected case, not an error. A file that exists but can't be read or
    /// parsed *is* an error, so a typo never degrades silently into
    /// "defaults".
    pub fn load() -> Result<Self, ConfigError> {
        let Some(path) = config_path() else {
            return Ok(Config::default());
        };
        match std::fs::read_to_string(&path) {
            Ok(contents) => Config::parse(&contents),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Config::default()),
            Err(e) => Err(ConfigError::Io(e)),
        }
    }
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
    fn is_pointer_only(&self) -> bool {
        matches!(self, Action::Move | Action::Resize)
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
            Action::TagPicker => "tag_picker",
            Action::TagSwitch => "tag_switch",
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
            Action::TagPicker => "Tag manager (assign tags to focused window)".to_string(),
            Action::TagSwitch => "Switch tag (or type a new name to create)".to_string(),
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
/// when that isn't set.
///
/// `None` when neither variable is set, rather than a relative path.
/// Code-review follow-up: this previously fell back to an empty base,
/// yielding the *relative* `buoy/config.toml`, which `load` would then
/// resolve against the WM's working directory — so a `buoy/config.toml`
/// sitting in whatever directory the session happened to launch from would
/// be read and its `exec` actions run through `sh -c`. Having no home to
/// look in means there is no config, not "look here instead".
pub fn config_path() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))?;
    Some(base.join("buoy").join("config.toml"))
}

#[cfg(test)]
mod tests {
    use super::*;

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
        assert!(actions.contains(&&Action::TagPicker));
        assert!(actions.contains(&&Action::TagSwitch));
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

    #[test]
    fn pinned_terminal_argv_substitutes_both_placeholders() {
        let config = Config::default();
        assert_eq!(
            config
                .defaults
                .pinned_terminal_argv("pinned-term", "tag-email"),
            vec![
                "-a",
                "pinned-term",
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
}
