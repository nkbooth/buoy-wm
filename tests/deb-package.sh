#!/usr/bin/env bash
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
# Asserts the contract of packaging/deb/debian/: the files the .deb is
# built from, its relations (with the rustc floor read from Cargo.toml),
# the installed layout the RPM also has, the cargo configuration the
# spec's %prep writes, and the licence text the copyright file carries.
#
# The .deb is built only on a tag (release.yml), so a dropped Recommends,
# a misplaced file or a floor left behind would otherwise surface in the
# release build or on a user's machine, not on the push that caused it.
# Two tiers, as tests/rpm-spec.sh: a text gate that always runs and
# reports every mismatch, and, where dpkg-parsechangelog exists, the
# real changelog parser.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
readonly REPO_ROOT
# shellcheck source=tests/lib.sh
source "$REPO_ROOT/tests/lib.sh"
readonly DEB="$REPO_ROOT/packaging/deb/debian"
readonly CONTROL="$DEB/control"
readonly RULES="$DEB/rules"
readonly CHANGELOG="$DEB/changelog"
readonly COPYRIGHT="$DEB/copyright"
readonly OVERRIDES="$DEB/buoy-wm.lintian-overrides"
readonly CARGO="$REPO_ROOT/Cargo.toml"
readonly SPEC="$REPO_ROOT/packaging/rpm/buoy-wm.spec"
readonly LICENCE="$REPO_ROOT/LICENSE.md"
readonly RELEASE_VERSION="$REPO_ROOT/scripts/release-version.sh"

tmp="$(mktemp -d)"
readonly tmp
trap 'rm -rf "$tmp"' EXIT

status=0
: >"$tmp/report"

# Every failure is appended and printed together at the end, so one run
# names everything that disagrees rather than the first thing.
fail() {
    printf '%s\n' "$1" >>"$tmp/report"
    status=1
}

rel() {
    printf '%s' "${1#"$REPO_ROOT/"}"
}

# The debian/ directory is exactly these files: `compat` would be a
# second compat level beside Build-Depends' debhelper-compat, and any
# other file is packaging nobody reviewed against this list.
expected_files='buoy-wm.docs
buoy-wm.install
buoy-wm.links
buoy-wm.lintian-overrides
changelog
control
copyright
rules
source/format'
if [ -d "$DEB" ]; then
    (cd "$DEB" && find . -type f -o -type l | sed 's|^\./||' | LC_ALL=C sort) >"$tmp/actual-files"
else
    : >"$tmp/actual-files"
fi
printf '%s\n' "$expected_files" | LC_ALL=C sort >"$tmp/expected-files"
while IFS= read -r f; do
    fail "$(rel "$DEB")/$f: missing"
done < <(LC_ALL=C comm -23 "$tmp/expected-files" "$tmp/actual-files")
while IFS= read -r f; do
    fail "$(rel "$DEB")/$f: not one of the files debian/ holds (extra)"
done < <(LC_ALL=C comm -13 "$tmp/expected-files" "$tmp/actual-files")
if [ -f "$RULES" ] && [ ! -x "$RULES" ]; then
    fail "$(rel "$RULES"): not executable (dpkg-buildpackage runs it directly)"
fi

# have <file>: true when the file can be read; its absence is already on
# the report from the list above.
have() {
    [ -f "$1" ] && [ -r "$1" ]
}

# content <file>: the lines that are neither `#` comments nor blank,
# which is what debhelper and lintian read from their config files.
content() {
    grep -vE '^[[:space:]]*(#|$)' "$1" || true
}

# expect_set <label> <actual-file> <expected-file>: equal as sorted
# lists, duplicates kept, so a second copy of an entry is a difference.
expect_set() {
    local label=$1 actual=$2 expected=$3
    LC_ALL=C sort "$actual" >"$actual.sorted"
    LC_ALL=C sort "$expected" >"$expected.sorted"
    if ! cmp -s "$actual.sorted" "$expected.sorted"; then
        fail "$label differs from the contract (- expected, + actual):"
        diff -u --label expected --label actual "$expected.sorted" "$actual.sorted" |
            tail -n +3 >>"$tmp/report" || true
    fi
}

# The toolchain floor is read from the manifest, never spelled here, as
# tests/rpm-spec.sh reads it: debian/control is the third copy of the
# number (ADR-014), and nothing else would notice it staying behind.
msrv_status=0
msrv="$(read_msrv "$CARGO")" || msrv_status=$?
case $msrv_status in
    1) fail "Cargo.toml: no 'rust-version = \"…\"' line to derive debian/control's rustc floor from" ;;
    2)
        fail "Cargo.toml: rust-version '$msrv' is not digits and dots (only the double-quoted spelling is read)"
        msrv=""
        ;;
esac

# control_field <name>: the field's value with its continuation lines
# joined, `#` comment lines dropped (deb822 allows them).
control_field() {
    awk -v want="$1" -v stanza="$2" '
        BEGIN { want = tolower(want) ":"; n = 1 }
        /^#/ { next }
        /^$/ { in_field = 0; if (seen) { n++; seen = 0 }; next }
        { seen = 1 }
        /^[^ \t]/ { in_field = 0 }
        n != stanza { next }
        in_field { sub(/^[ \t]+/, ""); value = value " " $0; next }
        index(tolower($0), want) == 1 { in_field = 1; value = substr($0, length(want) + 1) }
        END { print value }' "$CONTROL"
}

# field_count <field> [<stanza>]: how many lines open <field>, in one
# stanza or (no stanza) the whole file. dpkg ignores a binary field in
# the source stanza and drops it from the .deb, so where a field sits
# is part of the contract.
field_count() {
    awk -v want="$1" -v stanza="${2:-0}" '
        BEGIN { want = tolower(want) ":"; n = 1 }
        /^#/ { next }
        /^$/ { if (seen) { n++; seen = 0 }; next }
        { seen = 1 }
        (stanza == 0 || n == stanza) && index(tolower($0), want) == 1 { count++ }
        END { print count + 0 }' "$CONTROL"
}

# relations <field>: one relation per line, whitespace trimmed.
relations() {
    control_field "$1" "$2" | tr ',' '\n' | sed -e 's/^[[:space:]]*//' -e 's/[[:space:]]*$//' | grep -v '^$' || true
}

# expect_field_once <field> <value>: exactly one line starting with the
# field, carrying exactly this value.
readonly SOURCE_STANZA=1 BINARY_STANZA=2
stanza_name() {
    if [ "$1" -eq "$SOURCE_STANZA" ]; then echo source; else echo binary; fi
}

expect_field_once() {
    local field=$1 value=$2 stanza=$3 n
    n="$(field_count "$field")"
    if [ "$n" -ne 1 ]; then
        fail "$(rel "$CONTROL"): expected exactly one '$field:' line, found $n"
    elif [ "$(field_count "$field" "$stanza")" -ne 1 ]; then
        fail "$(rel "$CONTROL"): '$field:' is not in the $(stanza_name "$stanza") stanza"
    elif ! grep -qxF -- "$field: $value" "$CONTROL"; then
        fail "$(rel "$CONTROL"): '$field:' is not '$value'"
    fi
}

if have "$CONTROL"; then
    expect_field_once Source buoy-wm "$SOURCE_STANZA"
    expect_field_once Package buoy-wm "$BINARY_STANZA"
    # The spec's ExclusiveArch, in Debian's names.
    expect_field_once Architecture 'amd64 arm64' "$BINARY_STANZA"
    expect_field_once Section x11 "$SOURCE_STANZA"
    expect_field_once Rules-Requires-Root no "$SOURCE_STANZA"

    # The relation sets (AC 2). Build-Depends is the one list the Ubuntu
    # legs install from; Depends lets shlibs name the libraries the
    # binaries link (libc6 and libgcc-s1; libwayland is not linked).
    for field in Build-Depends Depends Recommends Suggests; do
        stanza=$BINARY_STANZA
        if [ "$field" = Build-Depends ]; then stanza=$SOURCE_STANZA; fi
        n="$(field_count "$field")"
        if [ "$n" -ne 1 ]; then
            fail "$(rel "$CONTROL"): expected exactly one '$field:' field, found $n"
        elif [ "$(field_count "$field" "$stanza")" -ne 1 ]; then
            fail "$(rel "$CONTROL"): '$field:' is not in the $(stanza_name "$stanza") stanza"
        fi
        relations "$field" "$stanza" >"$tmp/$field.actual"
    done
    {
        echo 'debhelper-compat (= 13)'
        echo 'cargo'
        echo "rustc (>= ${msrv:-<rust-version>})"
        echo 'libwayland-dev'
        echo 'pkgconf'
    } >"$tmp/Build-Depends.expected"
    # shellcheck disable=SC2016 # debhelper substitution variables, literal
    printf '%s\n' '${shlibs:Depends}' '${misc:Depends}' dbus-bin systemd >"$tmp/Depends.expected"
    printf '%s\n' foot fuzzel xdg-desktop-portal-wlr xdg-desktop-portal-gtk libnotify-bin >"$tmp/Recommends.expected"
    printf '%s\n' waybar >"$tmp/Suggests.expected"
    for field in Build-Depends Depends Recommends Suggests; do
        expect_set "$(rel "$CONTROL"): $field" "$tmp/$field.actual" "$tmp/$field.expected"
    done

    # Exactly one rustc relation, so a second, bare copy cannot hide
    # beside the gated floor.
    rustc_count="$(grep -cE '^rustc([[:space:](]|$)' "$tmp/Build-Depends.actual" || true)"
    if [ "$rustc_count" -ne 1 ]; then
        fail "$(rel "$CONTROL"): expected exactly one rustc relation in Build-Depends, found $rustc_count"
    fi

    # No Ubuntu archive carries river or zellij (ADR-013): a relation on
    # either is uninstallable or inert, and the README's PPA and zellij
    # steps cover them. Every relation field, not only the four above, in
    # either stanza and in any case (dpkg reads field names that way).
    for field in Pre-Depends Depends Recommends Suggests Enhances Build-Depends Build-Depends-Indep Build-Depends-Arch; do
        for stanza in "$SOURCE_STANZA" "$BINARY_STANZA"; do
            if relations "$field" "$stanza" | grep -qE '^(river|zellij)([[:space:](|]|$)'; then
                fail "$(rel "$CONTROL"): $field names river or zellij (no Ubuntu archive carries either, ADR-013)"
            fi
        done
    done
fi

# The installed layout is the spec's %install, enumerated from it rather
# than restated: each `install` line becomes a debhelper `.install` line,
# the `ln -s` the `.links` line, and `%doc` the `.docs` list, so a path
# added to or moved in the spec is a difference here. A spec line of any
# other shape, or a macro without a Debian mapping, is a finding, never
# silently dropped.
# spec_path <path>: a spec path with its macros expanded to the Debian
# layout, relative to the package root.
spec_path() {
    local p=$1
    p="${p//'%{buildroot}'/}"
    p="${p//'%{name}'/buoy-wm}"
    p="${p//'%{_libexecdir}'//usr/libexec}"
    p="${p//'%{_bindir}'//usr/bin}"
    p="${p//'%{_datadir}'//usr/share}"
    p="${p//'%{_userunitdir}'//usr/lib/systemd/user}"
    case $p in
        *%*) fail "$(rel "$SPEC"): %install path '$1' uses a macro this test does not map to the Debian layout" ;;
    esac
    printf '%s' "${p#/}"
}

# spec_install_lines: the %install section, continuation lines joined.
spec_install_lines() {
    awk '/^%install$/ { on = 1; next } /^%/ { on = 0 } on' "$SPEC" |
        sed -e ':a' -e '/\\$/N' -e 's/\\\n//' -e 'ta'
}

: >"$tmp/install.expected"
: >"$tmp/links.expected"
while read -r -a words; do
    if [ "${#words[@]}" -eq 0 ] || [ "${words[0]:0:1}" = '#' ]; then
        continue
    fi
    case "${words[*]}" in
        'install -d '*) ;;
        'install -m '[0-7]*' -t '*)
            dir="$(spec_path "${words[4]}")"
            for src in "${words[@]:5}"; do
                echo "$src $dir" >>"$tmp/install.expected"
            done
            ;;
        'install -Dpm '[0-7]*)
            dest="$(spec_path "${words[4]:-}")"
            if [ "${#words[@]}" -ne 5 ] || [ "${words[3]##*/}" != "${dest##*/}" ]; then
                fail "$(rel "$SPEC"): '${words[*]}' is not one source to the same name (dh_install keeps the source's basename)"
            fi
            echo "${words[3]} ${dest%/*}" >>"$tmp/install.expected"
            ;;
        'ln -s '*)
            echo "$(spec_path "${words[2]}") $(spec_path "${words[3]:-}")" >>"$tmp/links.expected"
            ;;
        *) fail "$(rel "$SPEC"): %install line '${words[*]}' is not a form this test maps to debhelper" ;;
    esac
done < <(spec_install_lines)
sed -n 's/^%doc //p' "$SPEC" | tr ' ' '\n' | grep -v '^$' >"$tmp/docs.expected" || true
for kind in install links docs; do
    if [ ! -s "$tmp/$kind.expected" ]; then
        fail "$(rel "$SPEC"): nothing to derive buoy-wm.$kind from"
    fi
    if have "$DEB/buoy-wm.$kind"; then
        content "$DEB/buoy-wm.$kind" >"$tmp/$kind.actual"
        expect_set "$(rel "$DEB/buoy-wm.$kind") (from the spec)" "$tmp/$kind.actual" "$tmp/$kind.expected"
    fi
done

# require_rules_line <line>: the whole line, exactly once.
require_rules_line() {
    local n
    n="$(grep -cxF -- "$1" "$RULES" || true)"
    if [ "$n" -ne 1 ]; then
        fail "$(rel "$RULES"): expected the line '$1' exactly once, found $n"
    fi
}

# rules_recipe <target>: the recipe of one make target, its tab lines
# and the make conditionals among them, up to the next blank line.
rules_recipe() {
    awk -v target="$1:" '
        $0 == target { on = 1; next }
        on && /^(\t|ifeq |ifneq |else$|endif$)/ { print; next }
        { on = 0 }' "$RULES"
}

# expect_recipe <target> <line>...: the target exists once and its
# recipe is exactly these lines, so a gated command cannot move to a
# target dh never calls or behind an inverted condition.
expect_recipe() {
    local target=$1 n
    shift
    n="$(grep -cxF -- "$target:" "$RULES" || true)"
    if [ "$n" -ne 1 ]; then
        fail "$(rel "$RULES"): expected the target '$target:' exactly once, found $n"
        return 0
    fi
    rules_recipe "$target" >"$tmp/recipe.actual"
    printf '%s\n' "$@" >"$tmp/recipe.expected"
    if ! cmp -s "$tmp/recipe.actual" "$tmp/recipe.expected"; then
        fail "$(rel "$RULES"): the $target recipe differs from the contract (- expected, + actual):"
        diff -u --label expected --label actual "$tmp/recipe.expected" "$tmp/recipe.actual" |
            tail -n +3 >>"$tmp/report" || true
    fi
}

if have "$RULES"; then
    # dh_clean deletes every *.orig in the tree, which takes each vendored
    # crate's Cargo.toml.orig with it, and cargo then refuses the vendored
    # source ("failed to calculate checksum of: …/Cargo.toml.orig").
    require_rules_line $'\tdh_clean -XCargo.toml.orig'
    expect_recipe override_dh_clean $'\tdh_clean -XCargo.toml.orig'
    # The spec's %build and %check: offline, from the lock, and the tests
    # skipped only on an explicit nocheck.
    require_rules_line $'\tcargo build --workspace --release --offline --locked'
    require_rules_line $'\tcargo test --workspace --release --offline --locked'
    expect_recipe override_dh_auto_build $'\tcargo build --workspace --release --offline --locked'
    # shellcheck disable=SC2016 # make syntax, literal
    expect_recipe override_dh_auto_test \
        'ifeq (,$(filter nocheck,$(DEB_BUILD_OPTIONS)))' \
        $'\tcargo test --workspace --release --offline --locked' \
        'endif'
    # shellcheck disable=SC2016 # make syntax, literal
    require_rules_line 'export CARGO_HOME = $(CURDIR)/debian/cargo-home'
    if grep -qF -- 'vendor.tar' "$RULES"; then
        fail "$(rel "$RULES"): unpacks the vendor tarball (build-deb.sh does it for a binary build, a source package's orig-vendor component for -S)"
    fi

    # One cargo configuration, two copies: the printf the rules file runs
    # must render exactly the text the spec's %prep heredoc writes.
    printf_lines="$(grep -cE "^"$'\t'"printf '[^']*' > \.cargo/config\.toml$" "$RULES" || true)"
    heredoc="$(awk '/^cat > \.cargo\/config\.toml <<'"'"'EOF'"'"'$/ { on = 1; next }
        on && /^EOF$/ { on = 0 }
        on' "$SPEC")"
    if [ "$printf_lines" -ne 1 ]; then
        fail "$(rel "$RULES"): expected exactly one \"printf '…' > .cargo/config.toml\" recipe line, found $printf_lines"
    elif ! rules_recipe override_dh_auto_configure | grep -qE "^"$'\t'"printf '[^']*' > \.cargo/config\.toml$"; then
        fail "$(rel "$RULES"): the .cargo/config.toml printf is not in the override_dh_auto_configure recipe"
    elif [ -z "$heredoc" ]; then
        fail "$(rel "$SPEC"): no %prep heredoc for .cargo/config.toml to compare debian/rules' printf with"
    else
        format="$(sed -n "s/^"$'\t'"printf '\([^']*\)' > \.cargo\/config\.toml\$/\1/p" "$RULES")"
        # shellcheck disable=SC2059 # the format string is the thing under test
        printf "$format" >"$tmp/config.rules"
        printf '%s\n' "$heredoc" >"$tmp/config.spec"
        if ! cmp -s "$tmp/config.rules" "$tmp/config.spec"; then
            fail "$(rel "$RULES"): the printf renders a .cargo/config.toml that differs from the spec's %prep heredoc (- spec, + rules):"
            diff -u --label spec --label rules "$tmp/config.spec" "$tmp/config.rules" | tail -n +3 >>"$tmp/report" || true
        fi
    fi
fi

if have "$DEB/source/format"; then
    if [ "$(cat "$DEB/source/format")" != '3.0 (quilt)' ]; then
        fail "$(rel "$DEB/source/format"): is not exactly '3.0 (quilt)' (a source package needs the orig tarball and its vendor component)"
    fi
fi

# paragraph <file> <first-line>: the body of the paragraph (blank-line
# separated) whose first line is exactly <first-line>.
paragraph() {
    awk -v head="$2" '
        /^$/ { on = 0; started = 0; next }
        !started { started = 1; if ($0 == head) { on = 1; next } }
        on' "$1"
}

# dep5_decode: one leading space off each line, ` .` back to a blank.
dep5_decode() {
    sed -e 's/^ \.$//' -e 's/^ //'
}

# strip_blank_edges: leading and trailing blank lines removed.
strip_blank_edges() {
    awk '{ lines[NR] = $0 }
        END {
            first = 1; while (first <= NR && lines[first] ~ /^[[:space:]]*$/) first++
            last = NR; while (last >= first && lines[last] ~ /^[[:space:]]*$/) last--
            for (i = first; i <= last; i++) print lines[i]
        }'
}

if have "$COPYRIGHT"; then
    require_copyright_line() {
        grep -qxF -- "$1" "$COPYRIGHT" ||
            fail "$(rel "$COPYRIGHT"): missing line '$1'"
    }
    require_copyright_line 'Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/'
    require_copyright_line 'Upstream-Name: buoy-wm'
    # The stanzas: the tree is RPL-1.5, the vendored protocol files MIT
    # (NOTICE.md's table).
    if ! paragraph "$COPYRIGHT" 'Files: *' | grep -qxF 'License: RPL-1.5'; then
        fail "$(rel "$COPYRIGHT"): the 'Files: *' stanza does not say 'License: RPL-1.5'"
    fi
    if ! paragraph "$COPYRIGHT" 'Files: wm/protocol/*' | grep -qxF 'License: Expat'; then
        fail "$(rel "$COPYRIGHT"): no 'Files: wm/protocol/*' stanza with 'License: Expat' (NOTICE.md: the river protocol files are MIT)"
    fi

    # The RPL text is LICENSE.md's, verbatim: every line after its first
    # `---` separator. A word changed in either copy is a difference.
    awk 'found { print } /^---$/ && !found { found = 1 }' "$LICENCE" | strip_blank_edges >"$tmp/rpl.licence"
    paragraph "$COPYRIGHT" 'License: RPL-1.5' | dep5_decode | strip_blank_edges >"$tmp/rpl.copyright"
    if [ ! -s "$tmp/rpl.copyright" ]; then
        fail "$(rel "$COPYRIGHT"): no standalone 'License: RPL-1.5' paragraph carrying the licence text"
    elif ! cmp -s "$tmp/rpl.licence" "$tmp/rpl.copyright"; then
        fail "$(rel "$COPYRIGHT"): the RPL-1.5 paragraph is not LICENSE.md's text (- LICENSE.md, + copyright):"
        diff -u --label LICENSE.md --label copyright "$tmp/rpl.licence" "$tmp/rpl.copyright" | tail -n +3 | head -n 20 >>"$tmp/report" || true
    fi

    # The Expat text is the one each protocol file carries in its
    # <copyright> element, with the XML indentation removed.
    paragraph "$COPYRIGHT" 'License: Expat' | dep5_decode | strip_blank_edges >"$tmp/expat.copyright"
    if [ ! -s "$tmp/expat.copyright" ]; then
        fail "$(rel "$COPYRIGHT"): no standalone 'License: Expat' paragraph carrying the licence text"
    else
        for xml in "$REPO_ROOT"/wm/protocol/*.xml; do
            awk '/SPDX-License-Identifier: MIT/ { on = 1; next }
                /<\/copyright>/ { on = 0 }
                on' "$xml" | sed 's/^    //' | strip_blank_edges >"$tmp/expat.xml"
            if ! cmp -s "$tmp/expat.xml" "$tmp/expat.copyright"; then
                fail "$(rel "$COPYRIGHT"): the Expat paragraph is not the MIT text $(rel "$xml") carries"
            fi
        done
    fi
fi

# The maintainer is one identity: control's Maintainer, the changelog
# trailer that signs the entry, and the address the lintian override
# names, so changing one cannot leave an override that no longer matches.
maintainer=""
if have "$CONTROL"; then
    maintainer="$(sed -n 's/^Maintainer: //p' "$CONTROL" | head -n1)"
    if [ -z "$maintainer" ]; then
        fail "$(rel "$CONTROL"): no 'Maintainer:' field"
    fi
    # One packager identity for both formats: the spec's newest
    # %changelog entry names it.
    spec_identity="$(sed -n '/^%changelog$/{n;s/^\* [A-Z][a-z][a-z] [A-Z][a-z][a-z] [0-9][0-9] [0-9]\{4\} \(.*>\) - .*/\1/p;q;}' "$SPEC")"
    if [ -z "$spec_identity" ]; then
        fail "$(rel "$SPEC"): no '* <date> <name> <<address>> - <version>' line under %changelog to compare the Maintainer with"
    elif [ -n "$maintainer" ] && [ "$maintainer" != "$spec_identity" ]; then
        fail "$(rel "$CONTROL"): Maintainer '$maintainer' is not the spec's %changelog identity '$spec_identity'"
    fi
fi
if have "$CHANGELOG" && [ -n "$maintainer" ]; then
    trailer="$(grep -m1 '^ -- ' "$CHANGELOG" || true)"
    case $trailer in
        " -- $maintainer  "*) ;;
        *) fail "$(rel "$CHANGELOG"): the first entry's trailer is not signed by the control file's Maintainer '$maintainer'" ;;
    esac
fi
if have "$OVERRIDES"; then
    content "$OVERRIDES" >"$tmp/overrides.actual"
    address="${maintainer#*<}"
    address="${address%>}"
    # lintian 2.139 on Ubuntu rejects the .radio TLD as bogus-mail-host;
    # that tag, on that address, and nothing else may be overridden.
    echo "bogus-mail-host Maintainer $address" >"$tmp/overrides.expected"
    expect_set "$(rel "$OVERRIDES")" "$tmp/overrides.actual" "$tmp/overrides.expected"
fi

if [ "$status" -ne 0 ]; then
    cat "$tmp/report" >&2
    exit 1
fi

# Tier (b): the real parser. Optional because the host has no
# dpkg-dev; CI installs it so this tier runs there.
if ! command -v dpkg-parsechangelog >/dev/null 2>&1; then
    echo "dpkg-parsechangelog not found; text gate passed, parser tier skipped"
    if [ -n "${GITHUB_ACTIONS:-}" ]; then
        echo "::warning file=tests/deb-package.sh::dpkg-parsechangelog not found; debian/changelog was not parsed"
    fi
    exit 0
fi

source_name="$(dpkg-parsechangelog -l "$CHANGELOG" -S Source)"
if [ "$source_name" != buoy-wm ]; then
    fail "$(rel "$CHANGELOG"): dpkg-parsechangelog reads Source '$source_name', not 'buoy-wm'"
fi
# The deb form release-version.sh maps Cargo.toml's version to, so this
# test and `--check` agree on one mapping (ADR-012).
cargo_version="$(grep -m1 '^version = ' "$CARGO" | cut -d'"' -f2 || true)"
deb_version="$("$RELEASE_VERSION" "v$cargo_version" | sed -n 's/^deb=//p')"
parsed_version="$(dpkg-parsechangelog -l "$CHANGELOG" -S Version)"
if [ "$parsed_version" != "$deb_version" ]; then
    fail "$(rel "$CHANGELOG"): dpkg-parsechangelog reads Version '$parsed_version', release-version.sh maps v$cargo_version to '$deb_version'"
fi

if [ "$status" -ne 0 ]; then
    cat "$tmp/report" >&2
    exit 1
fi
