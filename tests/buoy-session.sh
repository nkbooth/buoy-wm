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
# Runs packaging/buoy-session against stub `systemctl`,
# `dbus-update-activation-environment` and `buoy-wm` and asserts the
# supervisor contract: the exact argv of every systemd call in order, that
# the WM runs as a child of the supervisor (not exec'd), that TERM, INT and
# HUP sent to the supervisor all reach the WM as SIGTERM, that the session
# target is stopped only after the WM has exited (the stub logs its exit,
# so the order is observed rather than inferred), that a signal during the
# systemd steps still stops the target, that a failing systemctl never
# stops the WM from launching, and that the supervisor's exit status is
# always the WM's, including a WM killed by a signal.
#
# The stub log is the contract CI checks, so the five environment names
# and their order are compared byte for byte, and the PATH stubs reject an
# argument containing whitespace the way the real tools reject such a
# name: a `"$session_vars"` quoted by mistake is one argument, not five.
# A copy of the script is run from a directory that mirrors
# /usr/libexec/buoy-wm, with the stub WM beside it, because the script
# must find buoy-wm next to itself and never through PATH (the stub is
# deliberately not on PATH).
#
# Invoked directly, through its own shebang, so a `/bin/sh` that is bash
# (Fedora, this host) hides bashisms. The POSIX claim is exercised where
# `/bin/sh` is dash: the `hygiene` job on `ubuntu-latest` (Ubuntu 26.04)
# and, locally, a one-shot `ubuntu:26.04` or `debian:stable-slim`
# container (`bash /w/tests/buoy-session.sh`). Under GITHUB_ACTIONS the
# test warns if the runner's `/bin/sh` is ever not dash.
#
# A supervisor that outlives its case must be reaped before the stub
# directory goes away: once `$tmp/bin` is gone, its `systemctl --user
# stop buoy-session.target` resolves to the real systemctl and stops the
# live session's target on a developer's machine.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
readonly REPO_ROOT
readonly SCRIPT="$REPO_ROOT/packaging/buoy-session"

if [ ! -f "$SCRIPT" ]; then
    echo "$SCRIPT: missing" >&2
    exit 1
fi
if [ ! -r "$SCRIPT" ]; then
    echo "$SCRIPT: unreadable" >&2
    exit 1
fi
if [ ! -x "$SCRIPT" ]; then
    echo "$SCRIPT: not executable (river runs it through its shebang)" >&2
    exit 1
fi
# The POSIX claim, the dash coverage below and shellcheck's dialect all
# hang on this one line: a shebang quietly changed to bash would leave
# every case green and the /bin/sh report meaningless.
if [ "$(head -n 1 "$SCRIPT")" != '#!/bin/sh' ]; then
    echo "$SCRIPT: first line must be #!/bin/sh (the supervisor is POSIX sh, ADR-010)" >&2
    exit 1
fi

# A runner image whose /bin/sh is not dash would pass this test while
# proving nothing about the supervisor's POSIX claim, so say which shell
# ran it and annotate the run; bash-as-sh still proves the contract, dash
# is the extra coverage, so this is a warning, not a failure (same
# reasoning as the systemd-analyze warning in buoy-session-target.sh).
# Resolved only where it is reported, so a host run never depends on it.
if [ -n "${GITHUB_ACTIONS:-}" ]; then
    sh_path="$(readlink -f /bin/sh)"
    echo "/bin/sh is $sh_path"
    if [ "$(basename "$sh_path")" != dash ]; then
        echo "::warning file=tests/buoy-session.sh::/bin/sh is $sh_path, not dash; the POSIX claim of packaging/buoy-session is not exercised by this run"
    fi
fi

tmp="$(mktemp -d)"
readonly tmp
readonly STUB_LOG="$tmp/stub.log"
export STUB_LOG

# Set by run_case while a supervisor is running and cleared once it has
# been waited for, so neither a failing case nor an interrupted run leaves
# the supervisor, the stub WM or the stub's watchdog sleep behind. The stub
# records its own pid in $STUB_LOG.pid for the same reason.
supervisor_pid=""

# Terminates a supervisor and waits for it while the stubs are still on
# PATH, so its final `systemctl --user stop` hits the stub, never the real
# tool.
reap_supervisor() {
    if [ -n "$supervisor_pid" ]; then
        kill -s TERM "$supervisor_pid" 2>/dev/null || :
        wait "$supervisor_pid" 2>/dev/null || :
        supervisor_pid=""
    fi
    if [ -r "$STUB_LOG.pid" ]; then
        kill -s TERM "$(cat "$STUB_LOG.pid")" 2>/dev/null || :
        rm -f "$STUB_LOG.pid"
    fi
}

cleanup() {
    reap_supervisor
    rm -rf "$tmp"
}
trap cleanup EXIT

mkdir -p "$tmp/bin" "$tmp/libexec"

# Both PATH stubs log their name and argv. STUB_SYSTEMCTL_MODE selects:
#   ok          every call succeeds
#   fail        every call fails, the "systemd is broken at login" case
#   slow-start  `start` touches $STUB_LOG.starting and then takes a while,
#               so a signal can be delivered during the systemd steps
# An argument containing whitespace is rejected and logged as such: the
# real tools reject a name with a space in it, so one quoted word standing
# in for five names must not pass as the same log line.
cat > "$tmp/bin/systemctl" <<'EOF'
#!/bin/sh
for arg in "$@"; do
    case $arg in
        *[[:space:]]*)
            printf '%s rejected: argument with whitespace: %s\n' "$(basename "$0")" "$arg" >> "$STUB_LOG"
            exit 1
            ;;
    esac
done
printf '%s %s\n' "$(basename "$0")" "$*" >> "$STUB_LOG"
case "${STUB_SYSTEMCTL_MODE:-ok}" in
    fail)
        exit 1
        ;;
    slow-start)
        if [ "${2:-}" = start ]; then
            touch "$STUB_LOG.starting"
            sleep 0.5
        fi
        ;;
esac
exit 0
EOF
cp "$tmp/bin/systemctl" "$tmp/bin/dbus-update-activation-environment"
chmod 0755 "$tmp/bin/systemctl" "$tmp/bin/dbus-update-activation-environment"

# The stub WM logs the pid of its parent, which must be the supervisor,
# and logs `buoy-wm exit` on its way out so the test can see that the
# target is stopped after the WM has gone, not merely after it started.
# In `signal` mode it waits on a background sleep with a different exit
# status trapped for each signal, so the test can tell "forwarded as TERM"
# (7) from "forwarded as HUP" (11), "never forwarded" (99, after the 10 s
# watchdog sleep ends) and "the supervisor died instead" (143). INT has no
# such code: the stub is an asynchronous child of a non-interactive sh, so
# it starts with SIGINT ignored and the trap cannot take, and INT forwarded
# as INT shows up as 99 too. The sleep has to be a background child that
# the stub waits on: a foreground sleep would defer the trap until it
# finished. `selfterm` dies from SIGTERM, the status a crashed or killed
# WM reports, with no trap and no exit line.
cat > "$tmp/libexec/buoy-wm" <<'EOF'
#!/bin/sh
printf 'buoy-wm ppid=%s WAYLAND_DISPLAY=%s\n' "$PPID" "${WAYLAND_DISPLAY:-unset}" >> "$STUB_LOG"
echo "$$" > "$STUB_LOG.pid"
leave() {
    echo 'buoy-wm exit' >> "$STUB_LOG"
    exit "$1"
}
case "${STUB_WM_MODE:-exit0}" in
    exit0)
        leave 0
        ;;
    exit3)
        leave 3
        ;;
    selfterm)
        kill -s TERM "$$"
        sleep 10
        leave 99
        ;;
    signal)
        sleep 10 &
        watchdog=$!
        trap 'kill "$watchdog" 2>/dev/null; leave 7' TERM
        trap 'kill "$watchdog" 2>/dev/null; leave 11' HUP
        touch "$STUB_LOG.ready"
        wait "$watchdog"
        leave 99
        ;;
    *)
        echo "buoy-wm stub: unknown STUB_WM_MODE=$STUB_WM_MODE" >&2
        exit 98
        ;;
esac
EOF
chmod 0755 "$tmp/libexec/buoy-wm"

# Plain cp keeps the mode; the executable check above is what rejects a
# script committed without 0755, this just keeps the copy honest.
cp "$SCRIPT" "$tmp/libexec/buoy-session"

export PATH="$tmp/bin:$PATH"
export WAYLAND_DISPLAY=wayland-test
export XDG_CURRENT_DESKTOP=buoy
export XDG_SESSION_TYPE=wayland
export XDG_SESSION_DESKTOP=buoy
# The script passes all five names regardless; an unset one must not break
# it (systemctl and dbus-update-activation-environment both ignore unset
# names).
unset DISPLAY

readonly setup='systemctl --user import-environment WAYLAND_DISPLAY DISPLAY XDG_CURRENT_DESKTOP XDG_SESSION_TYPE XDG_SESSION_DESKTOP
dbus-update-activation-environment --systemd WAYLAND_DISPLAY DISPLAY XDG_CURRENT_DESKTOP XDG_SESSION_TYPE XDG_SESSION_DESKTOP
systemctl --user start buoy-session.target'
readonly stop='systemctl --user stop buoy-session.target'
readonly wm_start='buoy-wm ppid=SUPERVISOR WAYLAND_DISPLAY=wayland-test'

# The WM ran and left on its own terms (or via a forwarded signal).
readonly expected_run="$setup
$wm_start
buoy-wm exit
$stop"
# The WM was killed: no exit line, the stop still follows.
readonly expected_killed="$setup
$wm_start
$stop"
# A signal landed before the launch: no WM at all, the target is still
# stopped because the start may already have gone through.
readonly expected_no_wm="$setup
$stop"

failures=0

fail() {
    echo "case $1: $2" >&2
    failures=$((failures + 1))
}

# run_case NAME WM_MODE SYSTEMCTL_MODE SIGNAL EXPECTED_STATUS EXPECTED_LOG
#
# The supervisor is started through `env --default-signal` because a `&`
# child of this non-interactive bash inherits SIGINT ignored (and under
# nohup, SIGHUP too), so the INT and HUP cases could never reach the
# supervisor otherwise. env execs the script, so $! is the supervisor's
# own pid, which is also what the stub WM logs as its $PPID.
#
# SIGNAL, when given, is sent once the stub WM is ready (mode `signal`),
# or once the stub's `start` call is in progress (`slow-start`).
run_case() {
    local name="$1" mode="$2" systemctl_mode="$3" signal="$4"
    local expected_status="$5" expected_log="$6"
    local status=0 pid tries=0 marker

    rm -f "$STUB_LOG" "$STUB_LOG.ready" "$STUB_LOG.starting" "$STUB_LOG.pid"
    : > "$STUB_LOG"

    STUB_WM_MODE="$mode" STUB_SYSTEMCTL_MODE="$systemctl_mode" \
        env --default-signal "$tmp/libexec/buoy-session" 2> "$tmp/stderr" &
    pid=$!
    supervisor_pid="$pid"

    if [ -n "$signal" ]; then
        case "$systemctl_mode" in
            slow-start) marker="$STUB_LOG.starting" ;;
            *) marker="$STUB_LOG.ready" ;;
        esac
        # The markers are touched only once the thing being signalled is
        # in the state the case needs; signalling earlier races it.
        while [ ! -e "$marker" ]; do
            tries=$((tries + 1))
            if [ "$tries" -gt 50 ]; then
                fail "$name" "supervisor never reached the signal point"
                cat "$tmp/stderr" >&2
                reap_supervisor
                return
            fi
            sleep 0.1
        done
        if ! kill -s "$signal" "$pid" 2>/dev/null; then
            fail "$name" "supervisor exited before $signal could be sent"
            cat "$tmp/stderr" >&2
            reap_supervisor
            return
        fi
    fi

    wait "$pid" || status=$?
    supervisor_pid=""
    rm -f "$STUB_LOG.pid"

    if [ "$status" -ne "$expected_status" ]; then
        fail "$name" "expected status $expected_status, got $status"
        cat "$tmp/stderr" >&2
    fi

    # A wrong ppid shows up in the diff rather than being filtered out:
    # only the supervisor's own pid is rewritten to the placeholder.
    if ! diff -u --label expected --label actual \
        <(printf '%s\n' "$expected_log") \
        <(sed "s/^buoy-wm ppid=$pid /buoy-wm ppid=SUPERVISOR /" "$STUB_LOG") >&2; then
        fail "$name" "stub log differs from the expected sequence"
        cat "$tmp/stderr" >&2
    fi

    if [ "$systemctl_mode" = fail ] && ! grep -q '^buoy-session: ' "$tmp/stderr"; then
        fail "$name" "a failing systemctl was not reported on stderr"
    fi
}

run_case exit0 exit0 ok "" 0 "$expected_run"
run_case exit3 exit3 ok "" 3 "$expected_run"
run_case killed selfterm ok "" 143 "$expected_killed"
run_case TERM signal ok TERM 7 "$expected_run"
run_case INT signal ok INT 7 "$expected_run"
run_case HUP signal ok HUP 7 "$expected_run"
run_case TERM-during-start exit0 slow-start TERM 143 "$expected_no_wm"
run_case systemctl-fails exit3 fail "" 3 "$expected_run"

if [ "$failures" -ne 0 ]; then
    echo "$failures case(s) failed" >&2
    exit 1
fi
