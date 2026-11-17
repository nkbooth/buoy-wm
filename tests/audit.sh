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
# Holds docs/audit-2026-08.md to what it claims: every High and Medium
# finding of the August 2026 audit has exactly one close-out, each cited
# commit is in this branch's history, each roadmap item and acceptance it
# names exists, no High finding is merely roadmapped, and every `audit
# finding X-NN` comment in the tree names a row a reader can open.
#
# The audit's plan is private and gitignored (/audits/), so it exists only
# in the maintainer's main checkout. When it is there, the in-scope set
# and each severity are derived from it rather than restated here, and
# the plan's own close-out lines are held to the table. Anywhere else (a
# worktree, CI) that tier prints one `skipped:` line. BUOY_AUDIT_PLAN
# names another copy of the plan, for running a worktree's gate against
# the main checkout's plan.
#
# Blind spot, kept on purpose: a citation is found only in the form
# `audit finding(s) X-NN[, X-NN][ and X-NN]`, after joining comment lines,
# so an id cited any other way (a bare `(T-01)`) is not checked.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
readonly REPO_ROOT
# shellcheck source=tests/lib.sh
source "$REPO_ROOT/tests/lib.sh"
readonly RECORD_PATH=docs/audit-2026-08.md
readonly RECORD="$REPO_ROOT/$RECORD_PATH"
readonly ROADMAP="$REPO_ROOT/docs/ROADMAP.md"
readonly PLAN="${BUOY_AUDIT_PLAN:-$REPO_ROOT/audits/IMPLEMENTATION_PLAN.md}"
readonly TABLE_HEADING='## Close-out'
readonly ACCEPTED_HEADING='## Accepted'
readonly TABLE_HEADER='| ID | Severity | Finding | Close-out |'

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

for file in "$RECORD" "$ROADMAP"; do
    if [ ! -r "$file" ]; then
        fail "${file#"$REPO_ROOT/"}: missing or unreadable"
    fi
done
if [ "$status" -ne 0 ]; then
    cat "$tmp/report" >&2
    exit "$status"
fi

# section_lines <file> <heading>: the lines under the exact heading line
# up to the next `## ` heading, so a row or entry elsewhere in the file
# cannot satisfy a check about this section.
section_lines() {
    HEADING=$2 awk '
        $0 == ENVIRON["HEADING"] { inside = 1; next }
        inside && /^## / { exit }
        inside' "$1"
}

# table_rows <section-file>: one `id<TAB>severity<TAB>finding<TAB>close-out`
# line per table row, the header and separator dropped. A `|` line that
# is not a four-cell row prints as `!<TAB><the line>`, so the caller names
# it rather than the parser skipping it.
table_rows() {
    awk -F'|' -v header="$TABLE_HEADER" '
        function trim(s) { gsub(/^[ \t]+|[ \t]+$/, "", s); return s }
        !/^\|/ { next }
        n++ == 0 { if ($0 != header) printf "!\t%s\n", $0; next }
        n == 2 { next }
        NF != 6 || $1 != "" || $6 != "" { printf "!\t%s\n", $0; next }
        { printf "%s\t%s\t%s\t%s\n", trim($2), trim($3), trim($4), trim($5) }' "$1"
}

# closeout_parts <text> <accepted-prefix>: one line per part of a
# close-out — `fixed<TAB><sha>` per commit, `roadmap<TAB><heading>`,
# `accepted<TAB><n>` — and `note<TAB><text>` for the note. Status 1, with
# `error<TAB><what is left>` last, when the text does not follow
# `<part>[; <part>]…[ — <note>]`. The prefix is `§6 ` in the plan and
# empty in the tracked table.
closeout_parts() {
    local rest=$1 prefix=$2
    # shellcheck disable=SC2016 # literal Markdown backticks around a SHA
    local fixed_re='^fixed `([0-9a-f]{7,12})`'
    # shellcheck disable=SC2016 # literal Markdown backticks around a SHA
    local more_re='^, `([0-9a-f]{7,12})`'
    local roadmap_re='^roadmap "([^"]+)"'
    local accepted_re="^accepted ${prefix}#([0-9]+)"
    while :; do
        if [[ $rest =~ $fixed_re ]]; then
            printf 'fixed\t%s\n' "${BASH_REMATCH[1]}"
            rest=${rest:${#BASH_REMATCH[0]}}
            while [[ $rest =~ $more_re ]]; do
                printf 'fixed\t%s\n' "${BASH_REMATCH[1]}"
                rest=${rest:${#BASH_REMATCH[0]}}
            done
        elif [[ $rest =~ $roadmap_re ]]; then
            printf 'roadmap\t%s\n' "${BASH_REMATCH[1]}"
            rest=${rest:${#BASH_REMATCH[0]}}
        elif [[ $rest =~ $accepted_re ]]; then
            printf 'accepted\t%s\n' "${BASH_REMATCH[1]}"
            rest=${rest:${#BASH_REMATCH[0]}}
        else
            printf 'error\t%s\n' "$rest"
            return 1
        fi
        case $rest in
            '') return 0 ;;
            '; '*) rest=${rest:2} ;;
            ' — '?*)
                printf 'note\t%s\n' "${rest:3}"
                return 0
                ;;
            *)
                printf 'error\t%s\n' "$rest"
                return 1
                ;;
        esac
    done
}

# without_numbers <close-out>: the close-out with every acceptance number
# dropped, so the plan's line (which cites its own §6 numbers) and the
# table's cell (which cites this file's) compare as one text.
without_numbers() {
    printf '%s\n' "$1" | sed -E 's/accepted (§6 )?#[0-9]+/accepted/g'
}

# plan_items <plan>: one line per `#### X-NN — ` item of the plan as
# `id<TAB>label<TAB>in-scope<TAB>close-out count<TAB>first close-out`.
# The label is the bold text after `**Severity: `, its full stop dropped;
# a `Low` whose severity line also says Medium (one rating in the repo,
# another elsewhere) is `Low–Medium`. In scope is the plan's own rule:
# the severity line, cut before `**Effort`, says High or Medium. A block
# runs from its heading to the next `##`–`####` heading outside a fence.
plan_items() {
    awk '
        function flush() {
            if (id == "") return
            printf "%s\t%s\t%s\t%d\t%s\n", id, label, scope, closeouts, first
            id = ""
        }
        /^```/ { fence = !fence }
        fence { next }
        /^####? / || /^## / { flush() }
        /^#### [A-Z]-[0-9][0-9] — / {
            id = substr($0, 6, 4); label = "-"; scope = "no"
            closeouts = 0; first = ""; seen_severity = 0
            next
        }
        id == "" { next }
        !seen_severity && /^\*\*Severity/ {
            seen_severity = 1
            cut = $0
            at = index(cut, "**Effort")
            if (at > 0) cut = substr(cut, 1, at - 1)
            if (cut ~ /High|Medium/) scope = "yes"
            text = cut
            sub(/^\*\*Severity: /, "", text)
            at = index(text, "**")
            if (at > 0) {
                label = substr(text, 1, at - 1)
                sub(/\.$/, "", label)
                if (label == "Low" && cut ~ /Medium/) label = "Low–Medium"
            }
        }
        /^\*\*Close-out:\*\* / {
            closeouts++
            if (closeouts == 1) { first = $0; sub(/^\*\*Close-out:\*\* /, "", first) }
        }
        END { flush() }' "$1"
}

# numbered_entries <section-file>: the number of each `N. ` entry.
numbered_entries() {
    sed -nE 's/^([0-9]+)\. .*/\1/p' "$1"
}

# cited_ids: every audit id cited in tracked source outside the record,
# as `id<TAB>file`. Each file's lines are joined with their comment
# markers stripped first, so a citation a line break splits is still one.
cited_ids() {
    local files
    files="$(git -C "$REPO_ROOT" ls-files)" || return 1
    : >"$tmp/citing"
    while IFS= read -r file; do
        [ "$file" != "$RECORD_PATH" ] || continue
        [ -f "$REPO_ROOT/$file" ] || continue
        if grep -qI 'udit finding' "$REPO_ROOT/$file"; then
            printf '%s\n' "$REPO_ROOT/$file" >>"$tmp/citing"
        fi
    done <<<"$files"
    [ -s "$tmp/citing" ] || return 0
    local list
    mapfile -t list <"$tmp/citing"
    awk -v root="$REPO_ROOT/" '
        function scan(   s, hit, ids) {
            s = buf
            gsub(/[ \t]+/, " ", s)
            while (match(s, /[Aa]udit findings? [A-Z]-[0-9][0-9]((,| and|, and) [A-Z]-[0-9][0-9])*/)) {
                hit = substr(s, RSTART, RLENGTH)
                s = substr(s, RSTART + RLENGTH)
                while (match(hit, /[A-Z]-[0-9][0-9]/)) {
                    printf "%s\t%s\n", substr(hit, RSTART, RLENGTH), name
                    hit = substr(hit, RSTART + RLENGTH)
                }
            }
            buf = ""
        }
        FNR == 1 { if (NR > 1) scan(); name = substr(FILENAME, length(root) + 1) }
        {
            line = $0
            sub(/^[ \t]*(\/\/[\/!]?|#+|\*|--)?[ \t]*/, "", line)
            buf = buf " " line
        }
        END { if (NR > 0) scan() }' "${list[@]}"
}

section_lines "$RECORD" "$TABLE_HEADING" >"$tmp/table-section"
section_lines "$RECORD" "$ACCEPTED_HEADING" >"$tmp/accepted-section"
table_rows "$tmp/table-section" >"$tmp/rows"

# (i) The table: rows exist, at least one is High, every row is well
# formed and no id appears twice. A parser that matched nothing would
# otherwise pass every check below.
while IFS=$'\t' read -r id line; do
    [ "$id" = '!' ] || continue
    fail "$RECORD_PATH: '$line' under '$TABLE_HEADING' is not a four-cell row of a table headed '$TABLE_HEADER'"
done <"$tmp/rows"
grep -v '^!' "$tmp/rows" >"$tmp/good-rows" || true
row_count="$(grep -c . "$tmp/good-rows" || true)"
if [ "$row_count" -eq 0 ]; then
    fail "$RECORD_PATH: no close-out rows under '$TABLE_HEADING' (a parser that matches nothing must not pass)"
fi
cut -f2 "$tmp/good-rows" >"$tmp/severities"
if ! grep -qx 'High' "$tmp/severities"; then
    fail "$RECORD_PATH: no High row (the audit had four)"
fi
cut -f1 "$tmp/good-rows" | sort | uniq -d >"$tmp/dups"
while IFS= read -r id; do
    fail "$RECORD_PATH: $id has more than one row (each finding has exactly one close-out)"
done <"$tmp/dups"
if [ ! -s "$tmp/accepted-section" ]; then
    fail "$RECORD_PATH: no '$ACCEPTED_HEADING' section, or it is empty"
fi
numbered_entries "$tmp/accepted-section" >"$tmp/entries"
sort "$tmp/entries" | uniq -d >"$tmp/dup-entries"
while IFS= read -r n; do
    fail "$RECORD_PATH: '$ACCEPTED_HEADING' numbers entry $n more than once"
done <"$tmp/dup-entries"

: >"$tmp/cited-acceptances"
while IFS=$'\t' read -r id severity finding closeout; do
    if [[ ! $id =~ ^[A-Z]-[0-9]{2}$ ]]; then
        fail "$RECORD_PATH: row id '$id' is not an audit id (X-NN)"
        continue
    fi
    case $severity in
        High | Medium | Low–Medium | Low) ;;
        *) fail "$RECORD_PATH: $id has severity '$severity' (High, Medium, Low–Medium or Low)" ;;
    esac
    if [ -z "$finding" ]; then
        fail "$RECORD_PATH: $id has no finding summary"
    fi
    if ! closeout_parts "$closeout" '' >"$tmp/parts"; then
        fail "$RECORD_PATH: $id close-out does not parse at '$(tail -n1 "$tmp/parts" | cut -f2-)' (grammar: fixed \`sha\`[, \`sha\`]… | roadmap \"heading\" | accepted #n, joined by '; ', then ' — note')"
        continue
    fi

    # (iii) Each fixed commit is in this branch's history.
    while IFS=$'\t' read -r kind value; do
        case $kind in
            fixed)
                if ! git -C "$REPO_ROOT" merge-base --is-ancestor "$value" HEAD 2>/dev/null; then
                    fail "$RECORD_PATH: $id cites fixed \`$value\`, which is not an ancestor of HEAD"
                fi
                ;;
            # (iv) Each roadmap heading exists, once.
            roadmap)
                count="$(grep -cxF -- "## $value" "$ROADMAP" || true)"
                if [ "$count" -ne 1 ]; then
                    fail "$RECORD_PATH: $id cites roadmap \"$value\"; docs/ROADMAP.md has $count '## $value' lines (exactly one)"
                fi
                ;;
            # (v) Each acceptance number names an entry.
            accepted)
                printf '%s\n' "$value" >>"$tmp/cited-acceptances"
                if ! grep -qxF -- "$value" "$tmp/entries"; then
                    fail "$RECORD_PATH: $id cites accepted #$value; '$ACCEPTED_HEADING' has no entry $value"
                fi
                ;;
        esac
    done <"$tmp/parts"

    # (vi) No High finding is closed by a roadmap item alone.
    if [ "$severity" = High ] && ! grep -qE '^(fixed|accepted)'$'\t' "$tmp/parts"; then
        fail "$RECORD_PATH: $id is High and its close-out has no 'fixed' or 'accepted' part (a High item cannot be roadmap-only)"
    fi
done <"$tmp/good-rows"

# (v) Every acceptance is cited by at least one row.
while IFS= read -r n; do
    if ! grep -qxF -- "$n" "$tmp/cited-acceptances"; then
        fail "$RECORD_PATH: '$ACCEPTED_HEADING' entry $n is cited by no row"
    fi
done <"$tmp/entries"

# (vii) Every audit id cited in tracked source has a row.
if ! cited_ids >"$tmp/cited"; then
    fail "git ls-files failed; cannot list the citing files"
fi
if [ ! -s "$tmp/cited" ]; then
    fail "no 'audit finding X-NN' citation found in tracked source (the reader found nothing; about 200 exist)"
fi
cut -f1 "$tmp/good-rows" >"$tmp/row-ids"
sort -u "$tmp/cited" >"$tmp/cited-unique"
while IFS=$'\t' read -r id file; do
    if ! grep -qxF -- "$id" "$tmp/row-ids"; then
        fail "$file: cites audit finding $id, which has no row in $RECORD_PATH"
    fi
done <"$tmp/cited-unique"

# (i-b) Against the private plan, when it is here: the table's ids are the
# plan's in-scope set plus any Low rows, each severity is the plan's, and
# each in-scope item (and nothing else) carries one close-out line that
# reads as the table's cell does.
if [ ! -r "$PLAN" ]; then
    echo "skipped: private plan not present (${PLAN#"$REPO_ROOT/"}); the in-scope set is checked in the maintainer's checkout"
else
    plan_items "$PLAN" >"$tmp/plan-items"
    cut -f1 "$tmp/plan-items" >"$tmp/plan-ids"
    section_lines "$PLAN" '## 6. Won'"'"'t fix / accept risk' >"$tmp/plan-6"
    numbered_entries "$tmp/plan-6" >"$tmp/plan-6-entries"
    if [ ! -s "$tmp/plan-items" ]; then
        fail "$PLAN: no '#### X-NN — ' items found"
    fi
    if [ ! -s "$tmp/plan-6-entries" ]; then
        fail "$PLAN: no numbered entries under its §6 heading"
    fi
    while IFS=$'\t' read -r id label scope closeouts plan_closeout; do
        mapped=yes
        case $label in
            High | Medium | Low–Medium) [ "$scope" = yes ] || mapped=no ;;
            Low) [ "$scope" = no ] || mapped=no ;;
            *) mapped=no ;;
        esac
        if [ "$mapped" = no ]; then
            fail "$PLAN: $id's severity line reads as '$label' but in scope '$scope' (cannot derive one severity)"
            continue
        fi
        row="$(awk -F'\t' -v id="$id" '$1 == id' "$tmp/good-rows")"
        if [ "$scope" = no ]; then
            if [ "$closeouts" -ne 0 ]; then
                fail "$PLAN: $id is $label (out of scope) and has $closeouts close-out line(s) (none)"
            fi
            if [ -n "$row" ] && [ "$(printf '%s' "$row" | cut -f2)" != Low ]; then
                fail "$RECORD_PATH: $id is '$(printf '%s' "$row" | cut -f2)', the plan says Low"
            fi
            continue
        fi
        if [ "$closeouts" -ne 1 ]; then
            fail "$PLAN: $id is $label and has $closeouts '**Close-out:**' lines (exactly one)"
        fi
        if [ -z "$row" ]; then
            fail "$RECORD_PATH: $id is $label in the plan and has no row"
            continue
        fi
        if [ "$(printf '%s' "$row" | cut -f2)" != "$label" ]; then
            fail "$RECORD_PATH: $id is '$(printf '%s' "$row" | cut -f2)', the plan says '$label'"
        fi
        [ "$closeouts" -ge 1 ] || continue
        if ! closeout_parts "$plan_closeout" '§6 ' >"$tmp/plan-parts"; then
            fail "$PLAN: $id close-out does not parse at '$(tail -n1 "$tmp/plan-parts" | cut -f2-)' (accepted parts cite '§6 #n')"
            continue
        fi
        while IFS=$'\t' read -r kind value; do
            if [ "$kind" = accepted ] && ! grep -qxF -- "$value" "$tmp/plan-6-entries"; then
                fail "$PLAN: $id cites accepted §6 #$value; §6 has no entry $value"
            fi
        done <"$tmp/plan-parts"
        table_closeout="$(printf '%s' "$row" | cut -f4)"
        if [ "$(without_numbers "$plan_closeout")" != "$(without_numbers "$table_closeout")" ]; then
            fail "$id: the plan's close-out and the table's differ beyond acceptance numbers"
        fi
    done <"$tmp/plan-items"
    while IFS= read -r id; do
        if ! grep -qxF -- "$id" "$tmp/plan-ids"; then
            fail "$RECORD_PATH: $id has a row but no '#### $id — ' item in the plan"
        fi
    done <"$tmp/row-ids"
fi

if [ "$status" -ne 0 ]; then
    cat "$tmp/report" >&2
fi
exit "$status"
