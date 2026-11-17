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
# Holds docs/audit-2026-08.md to what it claims. Everywhere (CI included):
# the table is one well-formed table with the number of in-scope and High
# rows its own `In scope:` sentence states, every cited commit is in this
# branch's history, every roadmap item and acceptance it names exists, no
# High finding is merely roadmapped, every `audit finding X-NN` comment in
# the tree names a row, and every Low row is cited by one.
#
# The audit's plan is private and gitignored (/audits/), so it exists only
# in the maintainer's main checkout. When it is there, the in-scope set,
# both counts and each severity are derived from it rather than trusted,
# and each plan close-out line must read exactly as the table's cell once
# each `accepted #n` becomes the `§6` number its entry is tagged with.
# Anywhere else that tier prints one `skipped:` line. BUOY_AUDIT_PLAN names
# another copy of the plan, for running a worktree's gate against the main
# checkout's plan; set but unreadable is a failure, not a skip.
#
# Blind spots, kept on purpose: a citation is found only in the form
# `audit finding(s) X-NN[, X-NN][ and X-NN]` (also `or` and `/`), after
# joining each file's lines with comment markers stripped, so an id cited
# any other way (a bare `(T-01)`) is not checked. Without the plan, two
# rows that swap their acceptance numbers pass; the plan tier catches it.
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
# Field separator for the records the readers print: not IFS whitespace,
# so `read` keeps an empty field instead of shifting the next one left.
readonly US=$'\037'

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

# stray_table_lines <file>: every line that renders as part of a table
# (a `|` line, indented or not) outside the close-out section, so a
# second table cannot hold rows the gate never reads.
stray_table_lines() {
    HEADING=$TABLE_HEADING awk '
        $0 == ENVIRON["HEADING"] { inside = 1; next }
        inside && /^## / { inside = 0 }
        !inside && /^[ \t]*\|/ { print }' "$1"
}

# table_rows <section-file>: one `id US severity US finding US close-out`
# record per table row. The first `|` line must be the header and the
# second a separator; a `|` line that is indented, not a four-cell row, or
# after the table has ended prints as `! US <the line>`, so the caller
# names it rather than the parser skipping it.
table_rows() {
    awk -F'|' -v header="$TABLE_HEADER" -v us="$US" '
        function trim(s) { gsub(/^[ \t]+|[ \t]+$/, "", s); return s }
        function bad() { printf "!%s%s\n", us, $0 }
        !/^[ \t]*\|/ { if (n > 0) ended = 1; next }
        ended || /^[ \t]/ { bad(); next }
        n++ == 0 { if ($0 != header) bad(); next }
        n == 2 {
            if (NF != 6 || $0 !~ /^\|( *:?-+:? *\|)+$/) bad()
            next
        }
        NF != 6 || $1 != "" || $6 != "" { bad(); next }
        { printf "%s%s%s%s%s%s%s\n", trim($2), us, trim($3), us, trim($4), us, trim($5) }' "$1"
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
            '; '*) rest=${rest#'; '} ;;
            ' — '?*)
                printf 'note\t%s\n' "${rest#' — '}"
                return 0
                ;;
            *)
                printf 'error\t%s\n' "$rest"
                return 1
                ;;
        esac
    done
}

# accepted_entries <section-file>: one `n US plan-tags` record per
# numbered entry, where plan-tags is every `(plan §6 #m)` in the entry's
# text, space-separated. An entry runs from its `N. ` line to the next one.
accepted_entries() {
    awk -v us="$US" '
        function flush() {
            if (n == "") return
            tags = ""; s = text
            while (match(s, /\(plan §6 #[0-9]+\)/)) {
                t = substr(s, RSTART, RLENGTH); sub(/.*#/, "", t); sub(/\).*/, "", t)
                tags = tags (tags == "" ? "" : " ") t
                s = substr(s, RSTART + RLENGTH)
            }
            printf "%s%s%s\n", n, us, tags
        }
        /^[0-9]+\. / { flush(); n = $0; sub(/\..*/, "", n); text = $0; next }
        n != "" { text = text " " $0 }
        END { flush() }' "$1"
}

# plan_items <plan>: one record per `#### X-NN — ` item of the plan as
# `id US label US in-scope US close-out count US first close-out`. The
# label is the bold text after `**Severity: `, its full stop dropped; a
# `Low` whose severity line also says Medium (one rating in the repo,
# another elsewhere) is `Low–Medium`. In scope is the plan's own rule:
# the severity line, cut before `**Effort`, says High or Medium. A block
# runs from its heading to the next `##`–`####` heading outside a fence. A
# heading that names an id in any other shape prints as `! US <line>`.
plan_items() {
    awk -v us="$US" '
        function flush() {
            if (id == "") return
            printf "%s%s%s%s%s%s%d%s%s\n", id, us, label, us, scope, us, closeouts, us, first
            id = ""
        }
        /^```/ { fence = !fence }
        fence { next }
        /^##(##?)? / { flush() }
        /^#### [A-Z]-[0-9][0-9] — / {
            id = substr($0, 6, 4); label = "-"; scope = "no"
            closeouts = 0; first = ""; seen_severity = 0
            next
        }
        /^#+ *[A-Z]-[0-9]/ { printf "!%s%s\n", us, $0; next }
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

# roadmap_heading_count <heading>: how many `## <heading>` lines
# docs/ROADMAP.md has outside fenced code.
roadmap_heading_count() {
    WANT="## $1" awk '
        /^```/ { fence = !fence; next }
        !fence && $0 == ENVIRON["WANT"] { n++ }
        END { print n + 0 }' "$ROADMAP"
}

# with_plan_numbers <close-out>: the table's close-out with each
# `accepted #n` replaced by `accepted §6 #m`, m being entry n's plan tag
# (`?` when it has none), so it can be compared with the plan's line.
with_plan_numbers() {
    local rest=$1 out="" re='accepted #([0-9]+)' match tag
    while [[ $rest =~ $re ]]; do
        match=${BASH_REMATCH[0]}
        tag="$(awk -F"$US" -v n="${BASH_REMATCH[1]}" '$1 == n { print $2 }' "$tmp/entry-tags")"
        out+="${rest%%"$match"*}accepted §6 #${tag:-?}"
        rest=${rest#*"$match"}
    done
    printf '%s\n' "$out$rest"
}

# cited_ids: every audit id cited in tracked source outside the record,
# as `id<TAB>file`. Every line of each file is joined to the next with
# its leading comment or quote marker stripped, so a citation a line
# break splits is still one. An id is read with anything alphanumeric
# that follows it (`T-012` stays `T-012`, which no row matches). Status 1
# when `git ls-files` fails, 2 when awk does.
cited_ids() {
    git -C "$REPO_ROOT" ls-files -z >"$tmp/ls-files" || return 1
    : >"$tmp/citing"
    while IFS= read -r -d '' file; do
        [ "$file" != "$RECORD_PATH" ] || continue
        [ -f "$REPO_ROOT/$file" ] || continue
        if grep -qIi 'audit finding' "$REPO_ROOT/$file"; then
            printf '%s\n' "$REPO_ROOT/$file" >>"$tmp/citing"
        fi
    done <"$tmp/ls-files"
    [ -s "$tmp/citing" ] || return 0
    local list
    mapfile -t list <"$tmp/citing"
    awk -v root="$REPO_ROOT/" '
        function scan(   s, hit) {
            s = buf
            gsub(/[ \t]+/, " ", s)
            while (match(s, /[Aa]udit [Ff]indings? [A-Z]-[0-9][0-9A-Za-z]*((, and |, or |, | and | or |\/)[A-Z]-[0-9][0-9A-Za-z]*)*/)) {
                hit = substr(s, RSTART, RLENGTH)
                s = substr(s, RSTART + RLENGTH)
                sub(/^[Aa]udit [Ff]indings? /, "", hit)
                while (match(hit, /[A-Z]-[0-9][0-9A-Za-z]*/)) {
                    printf "%s\t%s\n", substr(hit, RSTART, RLENGTH), name
                    hit = substr(hit, RSTART + RLENGTH)
                }
            }
            buf = ""
        }
        FNR == 1 { if (NR > 1) scan(); name = substr(FILENAME, length(root) + 1) }
        {
            line = $0
            sub(/^[ \t]*(\/\/[\/!]?|#+|\*|--|>[ >]*)?[ \t]*/, "", line)
            buf = buf " " line
        }
        END { if (NR > 0) scan() }' "${list[@]}" || return 2
}

section_lines "$RECORD" "$TABLE_HEADING" >"$tmp/table-section"
section_lines "$RECORD" "$ACCEPTED_HEADING" >"$tmp/accepted-section"
table_rows "$tmp/table-section" >"$tmp/rows"

# (i) The table: one table, rows exist, every row is well formed, no id
# appears twice, and the in-scope and High row counts are the ones the
# record's `In scope:` sentence states. A parser that matched nothing, or
# a deleted row nothing cites, would otherwise pass every check below.
while IFS= read -r line; do
    fail "$RECORD_PATH: '$line' is a table line outside '$TABLE_HEADING' (one table, and the gate reads only that one)"
done < <(stray_table_lines "$RECORD")
while IFS="$US" read -r id line; do
    [ "$id" = '!' ] || continue
    fail "$RECORD_PATH: '$line' under '$TABLE_HEADING' is not a row of one unbroken, unindented four-cell table headed '$TABLE_HEADER' with a separator line"
done <"$tmp/rows"
grep -v '^!' "$tmp/rows" >"$tmp/good-rows" || true
row_count="$(grep -c . "$tmp/good-rows" || true)"
if [ "$row_count" -eq 0 ]; then
    fail "$RECORD_PATH: no close-out rows under '$TABLE_HEADING' (a parser that matches nothing must not pass)"
fi
cut -d "$US" -f1 "$tmp/good-rows" >"$tmp/row-ids"
cut -d "$US" -f2 "$tmp/good-rows" >"$tmp/severities"
in_scope_rows="$(grep -cvx 'Low' "$tmp/severities" || true)"
high_rows="$(grep -cx 'High' "$tmp/severities" || true)"
scope_line_count="$(grep -cE '^In scope: [0-9]+ findings, [0-9]+ of them High\.$' "$RECORD" || true)"
if [ "$scope_line_count" -ne 1 ]; then
    fail "$RECORD_PATH: expected exactly one line 'In scope: <n> findings, <h> of them High.', found $scope_line_count"
    stated_scope='' stated_high=''
else
    stated_scope="$(sed -nE 's/^In scope: ([0-9]+) findings, [0-9]+ of them High\.$/\1/p' "$RECORD")"
    stated_high="$(sed -nE 's/^In scope: [0-9]+ findings, ([0-9]+) of them High\.$/\1/p' "$RECORD")"
    if [ "$in_scope_rows" -ne "$stated_scope" ]; then
        fail "$RECORD_PATH: $in_scope_rows High or Medium rows, but its 'In scope:' line says $stated_scope"
    fi
    if [ "$high_rows" -ne "$stated_high" ]; then
        fail "$RECORD_PATH: $high_rows High rows, but its 'In scope:' line says $stated_high"
    fi
fi
sort "$tmp/row-ids" | uniq -d >"$tmp/dups"
while IFS= read -r id; do
    fail "$RECORD_PATH: $id has more than one row (each finding has exactly one close-out)"
done <"$tmp/dups"

# (v) The acceptances: numbered 1, 2, 3… in order (Markdown renders an
# ordered list from its first number, so any other numbering cites the
# wrong item on the page), each tagged with the plan §6 entry it is.
if [ ! -s "$tmp/accepted-section" ]; then
    fail "$RECORD_PATH: no '$ACCEPTED_HEADING' section, or it is empty"
fi
if grep -qE '^[0-9]+\) ' "$tmp/accepted-section"; then
    fail "$RECORD_PATH: '$ACCEPTED_HEADING' has an 'N) ' entry (entries are 'N. ', which the gate reads)"
fi
numbered_entries "$tmp/accepted-section" >"$tmp/entries"
expected=1
while IFS= read -r n; do
    if [ "$n" != "$expected" ]; then
        fail "$RECORD_PATH: '$ACCEPTED_HEADING' entry numbered $n where $expected is due (number them 1, 2, 3… in order)"
    fi
    expected=$((expected + 1))
done <"$tmp/entries"
accepted_entries "$tmp/accepted-section" >"$tmp/entry-tags"
while IFS="$US" read -r n tags; do
    if [[ ! $tags =~ ^[0-9]+$ ]]; then
        fail "$RECORD_PATH: '$ACCEPTED_HEADING' entry $n has tags '$tags'; it needs exactly one '(plan §6 #m)'"
    fi
done <"$tmp/entry-tags"

: >"$tmp/cited-acceptances"
while IFS="$US" read -r id severity finding closeout; do
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
    if grep -v '^note'$'\t' "$tmp/parts" | sort | uniq -d | grep -q .; then
        fail "$RECORD_PATH: $id close-out repeats a part"
    fi

    # (iii) Each fixed commit is in this branch's history.
    while IFS=$'\t' read -r kind value; do
        case $kind in
            fixed)
                if ! git -C "$REPO_ROOT" cat-file -e "$value^{commit}" 2>/dev/null; then
                    fail "$RECORD_PATH: $id cites fixed \`$value\`, which names no commit here (missing, ambiguous, or the clone is shallow)"
                elif ! git -C "$REPO_ROOT" merge-base --is-ancestor "$value" HEAD; then
                    fail "$RECORD_PATH: $id cites fixed \`$value\`, which is not an ancestor of HEAD"
                fi
                ;;
            # (iv) Each roadmap heading exists, once, outside fenced code.
            roadmap)
                count="$(roadmap_heading_count "$value")"
                if [ "$count" -ne 1 ]; then
                    fail "$RECORD_PATH: $id cites roadmap \"$value\"; docs/ROADMAP.md has $count '## $value' headings (exactly one)"
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

# (vii) Every audit id cited in tracked source has a row, and every Low
# row (there only because code cites it) is cited.
cited_status=0
cited_ids >"$tmp/cited" || cited_status=$?
case $cited_status in
    0) ;;
    1) fail "git ls-files failed; cannot list the citing files" ;;
    *) fail "the citation reader (awk) failed; citations were not checked" ;;
esac
if [ "$cited_status" -eq 0 ] && [ ! -s "$tmp/cited" ]; then
    fail "no 'audit finding X-NN' citation found in tracked source (the reader found nothing; about 200 exist)"
fi
sort -u "$tmp/cited" >"$tmp/cited-unique"
cut -f1 "$tmp/cited-unique" | sort -u >"$tmp/cited-ids"
while IFS=$'\t' read -r id file; do
    if ! grep -qxF -- "$id" "$tmp/row-ids"; then
        fail "$file: cites audit finding $id, which has no row in $RECORD_PATH"
    fi
done <"$tmp/cited-unique"
while IFS="$US" read -r id severity _; do
    if [ "$severity" = Low ] && ! grep -qxF -- "$id" "$tmp/cited-ids"; then
        fail "$RECORD_PATH: $id is a Low row nothing in the tree cites (Low rows exist only for citations)"
    fi
done <"$tmp/good-rows"

# (i-b) Against the private plan, when it is here: the table's ids are the
# plan's in-scope set plus Low rows, both stated counts and each severity
# are the plan's, each in-scope item (and nothing else) carries one
# close-out line, and that line is the table's cell in the plan's numbers.
if [ ! -r "$PLAN" ]; then
    if [ -n "${BUOY_AUDIT_PLAN:-}" ]; then
        fail "BUOY_AUDIT_PLAN names '$PLAN', which cannot be read (set means the plan tier was asked for)"
    else
        echo "skipped: private plan not present (${PLAN#"$REPO_ROOT/"}); the in-scope set is checked in the maintainer's checkout"
    fi
else
    plan_items "$PLAN" >"$tmp/plan-all"
    while IFS="$US" read -r id line; do
        [ "$id" = '!' ] || continue
        fail "$PLAN: heading '$line' names an id but is not '#### X-NN — title' (the item would drop out of the in-scope set)"
    done <"$tmp/plan-all"
    grep -v '^!' "$tmp/plan-all" >"$tmp/plan-items" || true
    cut -d "$US" -f1 "$tmp/plan-items" >"$tmp/plan-ids"
    sort "$tmp/plan-ids" | uniq -d >"$tmp/plan-dups"
    while IFS= read -r id; do
        fail "$PLAN: '#### $id — ' appears more than once"
    done <"$tmp/plan-dups"
    section_lines "$PLAN" '## 6. Won'"'"'t fix / accept risk' >"$tmp/plan-6"
    numbered_entries "$tmp/plan-6" >"$tmp/plan-6-entries"
    if [ ! -s "$tmp/plan-items" ]; then
        fail "$PLAN: no '#### X-NN — ' items found"
    fi
    if [ ! -s "$tmp/plan-6-entries" ]; then
        fail "$PLAN: no numbered entries under its §6 heading"
    fi
    while IFS="$US" read -r n tag; do
        if [[ $tag =~ ^[0-9]+$ ]] && ! grep -qxF -- "$tag" "$tmp/plan-6-entries"; then
            fail "$RECORD_PATH: '$ACCEPTED_HEADING' entry $n is tagged plan §6 #$tag; §6 has no entry $tag"
        fi
    done <"$tmp/entry-tags"
    plan_scope=0 plan_high=0
    while IFS="$US" read -r id label scope closeouts plan_closeout; do
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
        row="$(awk -F"$US" -v id="$id" '$1 == id' "$tmp/good-rows")"
        row_severity="$(printf '%s' "$row" | cut -d "$US" -f2)"
        if [ "$scope" = no ]; then
            if [ "$closeouts" -ne 0 ]; then
                fail "$PLAN: $id is $label (out of scope) and has $closeouts close-out line(s) (none)"
            fi
            if [ -n "$row" ] && [ "$row_severity" != Low ]; then
                fail "$RECORD_PATH: $id is '$row_severity', the plan says Low"
            fi
            continue
        fi
        plan_scope=$((plan_scope + 1))
        [ "$label" != High ] || plan_high=$((plan_high + 1))
        if [ "$closeouts" -ne 1 ]; then
            fail "$PLAN: $id is $label and has $closeouts '**Close-out:**' lines (exactly one)"
        fi
        if [ -z "$row" ]; then
            fail "$RECORD_PATH: $id is $label in the plan and has no row"
            continue
        fi
        if [ "$row_severity" != "$label" ]; then
            fail "$RECORD_PATH: $id is '$row_severity', the plan says '$label'"
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
        table_closeout="$(printf '%s' "$row" | cut -d "$US" -f4)"
        if [ "$plan_closeout" != "$(with_plan_numbers "$table_closeout")" ]; then
            fail "$id: the plan's close-out is not the table's cell with each acceptance in its plan §6 number"
        fi
    done <"$tmp/plan-items"
    if [ -n "$stated_scope" ] && [ "$plan_scope" -ne "$stated_scope" ]; then
        fail "$RECORD_PATH: its 'In scope:' line says $stated_scope; the plan's rule derives $plan_scope"
    fi
    if [ -n "$stated_high" ] && [ "$plan_high" -ne "$stated_high" ]; then
        fail "$RECORD_PATH: its 'In scope:' line says $stated_high High; the plan has $plan_high"
    fi
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
