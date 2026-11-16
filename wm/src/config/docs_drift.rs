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

//! Holds `buoy-config(5)` and the README's defaults table to the code.
//!
//! Every expected name is derived, never listed here: the field and
//! variant names come from serde's own "expected one of" list in the
//! error for a deliberately unknown name, and the built-in bindings are
//! rendered with the cheat-sheet's label and description functions. A
//! name the code accepts and the page does not mention, as a bold term in
//! the section that documents it, is a failure. serde's lists include the
//! `#[serde(alias)]` spellings, so an alias added in code is held to the
//! page the same way.

use super::*;

const CONFIG_PAGE: &str = include_str!("../../../docs/man/buoy-config.5.scd");
const README: &str = include_str!("../../../README.md");

/// The backticked names after "expected" in a serde error message, which
/// is how serde lists the fields or variants it would have accepted.
fn expected_names(message: &str) -> Vec<String> {
    let (_, list) = message
        .rsplit_once("expected ")
        .unwrap_or_else(|| panic!("no 'expected' list in serde's message: {message}"));
    let names: Vec<String> = list
        .split('`')
        .skip(1)
        .step_by(2)
        .map(str::to_string)
        .collect();
    assert!(
        !names.is_empty(),
        "serde's message lists no names: {message}"
    );
    names
}

/// The error a document with one unknown name produces, as displayed.
fn rejection(document: &str) -> String {
    match toml::from_str::<RawConfig>(document) {
        Ok(_) => panic!("expected a rejection for:\n{document}"),
        Err(error) => error.to_string(),
    }
}

/// Where a name has to appear for it to count as documented.
#[derive(Clone, Copy)]
enum Placement {
    /// Opening a line: the term of its own definition entry, so prose
    /// that mentions it (`terminal` is both a key and an action) does
    /// not stand in for the entry.
    Term,
    /// Anywhere in the section, for values listed inline.
    Inline,
}

/// Each name must be a bold term inside one of `sections`, placed as
/// `placement` says.
fn require_bold_terms(what: &str, names: &[String], sections: &[&str], placement: Placement) {
    let lines: Vec<&str> = sections
        .iter()
        .flat_map(|title| page_section(CONFIG_PAGE, title))
        .collect();
    let documented = |name: &String| {
        let bold = format!("*{name}*");
        lines.iter().any(|line| match placement {
            Placement::Term => line.starts_with(&bold),
            Placement::Inline => line.contains(&bold),
        })
    };
    let missing: Vec<&String> = names.iter().filter(|name| !documented(name)).collect();
    assert!(
        missing.is_empty(),
        "buoy-config.5.scd does not name these {what} as bold terms in {sections:?}: {missing:?}"
    );
}

/// The `scdoc` section under `# <title>`, up to the next top-level heading.
fn page_section<'a>(page: &'a str, title: &str) -> Vec<&'a str> {
    let heading = format!("# {title}");
    let mut lines = page.lines().skip_while(|line| *line != heading);
    assert!(
        lines.next().is_some(),
        "buoy-config.5.scd has no '{heading}' section"
    );
    lines.take_while(|line| !line.starts_with("# ")).collect()
}

/// The built-in bindings as the cheat-sheet labels and describes them.
fn built_in_rows() -> Vec<(String, String)> {
    let built_in = Config::default();
    let keys = built_in.keybinds.iter().map(|bind| {
        (
            binding_label(&bind.mods, &bind.key),
            bind.action.description(&built_in.defaults),
        )
    });
    let buttons = built_in.mousebinds.iter().map(|bind| {
        (
            binding_label(&bind.mods, bind.button.label()),
            bind.action.description(&built_in.defaults),
        )
    });
    let mut rows: Vec<(String, String)> = keys.chain(buttons).collect();
    rows.sort();
    rows
}

#[test]
fn every_config_table_and_field_is_a_bold_term_in_buoy_config_5() {
    for (what, document, section) in [
        ("top-level tables", "bogus = 1", "DESCRIPTION"),
        ("[defaults] keys", "[defaults]\nbogus = 1", "DEFAULTS"),
        (
            "[[keybind]] fields",
            "[[keybind]]\nbogus = 1",
            "KEY BINDINGS",
        ),
        (
            "[[mousebind]] fields",
            "[[mousebind]]\nbogus = 1",
            "POINTER BINDINGS",
        ),
        ("[[input]] fields", "[[input]]\nbogus = 1", "INPUT DEVICES"),
    ] {
        let message = rejection(document);
        assert!(
            message.contains("unknown field `bogus`"),
            "{what}: not an unknown-field rejection: {message}"
        );
        require_bold_terms(what, &expected_names(&message), &[section], Placement::Term);
    }
}

#[test]
fn every_enum_spelling_is_a_bold_term_in_buoy_config_5() {
    // An alias is documented in ALIASES, beside its canonical name's own
    // section.
    for (what, document, sections) in [
        (
            "actions",
            "[[keybind]]\nkey = \"a\"\naction = \"bogus\"",
            &["ACTIONS", "ALIASES"][..],
        ),
        (
            "modifiers",
            "[[keybind]]\nmod = [\"bogus\"]\nkey = \"a\"\naction = \"close\"",
            &["KEY BINDINGS", "ALIASES"][..],
        ),
        (
            "buttons",
            "[[mousebind]]\nbutton = \"bogus\"\naction = \"move\"",
            &["POINTER BINDINGS", "ALIASES"][..],
        ),
        (
            "tap_button_map values",
            "[[input]]\nname = \"x\"\ntap_button_map = \"bogus\"",
            &["INPUT DEVICES"][..],
        ),
        (
            "click_method values",
            "[[input]]\nname = \"x\"\nclick_method = \"bogus\"",
            &["INPUT DEVICES"][..],
        ),
    ] {
        let message = rejection(document);
        assert!(
            message.contains("unknown variant `bogus`"),
            "{what}: not an unknown-variant rejection: {message}"
        );
        require_bold_terms(what, &expected_names(&message), sections, Placement::Inline);
    }
}

/// The page says which canonical name each alias stands for, so each
/// such line must really parse as the variant it names.
#[test]
fn every_alias_the_page_documents_parses() {
    let aliases = page_section(CONFIG_PAGE, "ALIASES");
    let mut checked = 0;
    for line in aliases {
        let Some((alias, canonical)) = line
            .strip_prefix('*')
            .and_then(|rest| rest.split_once("* is *"))
            .map(|(alias, rest)| (alias, rest.trim_end_matches(['*', '.'])))
        else {
            continue;
        };
        let as_action = |name: &str| {
            toml::from_str::<RawConfig>(&format!("[[keybind]]\nkey = \"a\"\naction = \"{name}\""))
                .map(|raw| raw.keybinds[0].action.clone())
        };
        let as_modifier = |name: &str| {
            toml::from_str::<RawConfig>(&format!(
                "[[keybind]]\nmod = [\"{name}\"]\nkey = \"a\"\naction = \"close\""
            ))
            .map(|raw| raw.keybinds[0].mods.clone())
        };
        let as_button = |name: &str| {
            toml::from_str::<RawConfig>(&format!(
                "[[mousebind]]\nbutton = \"{name}\"\naction = \"move\""
            ))
            .map(|raw| raw.mousebinds[0].button)
        };
        let same = match (as_action(alias), as_action(canonical)) {
            (Ok(a), Ok(b)) => a == b,
            _ => match (as_modifier(alias), as_modifier(canonical)) {
                (Ok(a), Ok(b)) => a == b,
                _ => matches!(
                    (as_button(alias), as_button(canonical)),
                    (Ok(a), Ok(b)) if a == b
                ),
            },
        };
        assert!(
            same,
            "buoy-config.5.scd says '{alias}' is '{canonical}', which the parser does not accept"
        );
        checked += 1;
    }
    assert!(
        checked > 0,
        "buoy-config.5.scd's ALIASES section documents no alias"
    );
}

#[test]
fn the_built_in_bindings_section_of_buoy_config_5_is_config_default() {
    let section = page_section(CONFIG_PAGE, "BUILT-IN BINDINGS");
    let mut rows: Vec<(String, String)> = section
        .windows(2)
        .filter_map(|pair| {
            let label = pair[0].strip_prefix('*')?.strip_suffix('*')?;
            let description = pair[1].strip_prefix('\t')?;
            Some((label.to_string(), description.to_string()))
        })
        .collect();
    rows.sort();
    assert_eq!(rows, built_in_rows());
}

#[test]
fn the_readme_default_keybindings_table_is_config_default() {
    let mut lines = README.lines().skip_while(|line| *line != "## Keybindings");
    assert!(
        lines.next().is_some(),
        "README.md has no '## Keybindings' heading"
    );
    let mut rows: Vec<(String, String)> = lines
        .take_while(|line| !line.starts_with("#"))
        .filter_map(|line| {
            let cells = line.strip_prefix("| `")?.strip_suffix(" |")?;
            let (label, description) = cells.split_once("` | ")?;
            Some((label.to_string(), description.to_string()))
        })
        .collect();
    rows.sort();
    assert_eq!(rows, built_in_rows());
}
