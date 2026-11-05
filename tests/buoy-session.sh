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
# target is stopped only after the WM has exited, that a failing systemctl
# never stops the WM from launching, and that the supervisor's exit status
# is always the WM's.
#
# The stub log is the contract CI checks, so the five environment names
# and their order are compared byte for byte. A copy of the script is run
# from a directory that mirrors /usr/libexec/buoy-wm, with the stub WM
# beside it, because the script must find buoy-wm next to itself and never
# through PATH (the stub is deliberately not on PATH).
#
# Invoked directly, through its own shebang, so a `/bin/sh` that is bash
# (Fedora) hides bashisms; CI also runs this file in a Debian container
# where `/bin/sh` is dash.
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

tmp="$(mktemp -d)"
readonly tmp
readonly STUB_LOG="$tmp/stub.log"
export STUB_LOG

# Set by run_case while a supervisor is running, so a failing case does not
# leave the supervisor, the stub WM or the stub's watchdog sleep behind on
# the runner. The stub records its own pid in $STUB_LOG.pid for the same
# reason.
supervisor_pid=""

cleanup() {
    if [ -n "$supervisor_pid" ]; then
        kill -s TERM "$supervisor_pid" 2>/dev/null || :
    fi
    if [ -r "$STUB_LOG.pid" ]; then
        kill -s TERM "$(cat "$STUB_LOG.pid")" 2>/dev/null || :
    fi
    rm -rf "$tmp"
}
trap cleanup EXIT

mkdir -p "$tmp/bin" "$tmp/libexec"

# Both PATH stubs log their name and argv. STUB_SYSTEMCTL_FAIL=1 makes
# every call fail, which is the "systemd is broken at login" case.
cat > "$tmp/bin/systemctl" <<'EOF'
#!/bin/sh
printf '%s %s\n' "$(basename "$0")" "$*" >> "$STUB_LOG"
if [ "${STUB_SYSTEMCTL_FAIL:-0}" = 1 ]; then
    exit 1
fi
exit 0
EOF
cp "$tmp/bin/systemctl" "$tmp/bin/dbus-update-activation-environment"
chmod 0755 "$tmp/bin/systemctl" "$tmp/bin/dbus-update-activation-environment"

# The stub WM logs the pid of its parent, which must be the supervisor.
# In `signal` mode it waits on a background sleep with a different exit
# status trapped for each signal, so the test can tell "forwarded as TERM"
# (7) from "forwarded unmapped" (9, 11), "never forwarded" (99, after the
# 10 s watchdog sleep ends) and "the supervisor died instead" (143). The
# sleep has to be a background child that the stub waits on: a foreground
# sleep would defer the trap until it finished.
cat > "$tmp/libexec/buoy-wm" <<'EOF'
#!/bin/sh
printf 'buoy-wm ppid=%s WAYLAND_DISPLAY=%s\n' "$PPID" "${WAYLAND_DISPLAY:-unset}" >> "$STUB_LOG"
echo "$$" > "$STUB_LOG.pid"
case "${STUB_WM_MODE:-exit0}" in
    exit0)
        exit 0
        ;;
    exit3)
        exit 3
        ;;
    signal)
        sleep 10 &
        watchdog=$!
        trap 'kill "$watchdog" 2>/dev/null; exit 7' TERM
        trap 'kill "$watchdog" 2>/dev/null; exit 9' INT
        trap 'kill "$watchdog" 2>/dev/null; exit 11' HUP
        touch "$STUB_LOG.ready"
        wait "$watchdog"
        exit 99
        ;;
    *)
        echo "buoy-wm stub: unknown STUB_WM_MODE=$STUB_WM_MODE" >&2
        exit 98
        ;;
esac
EOF
chmod 0755 "$tmp/libexec/buoy-wm"

# Plain cp keeps the mode, so a script committed without 0755 fails here
# with 126 rather than passing because the test fixed it up.
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

readonly expected='systemctl --user import-environment WAYLAND_DISPLAY DISPLAY XDG_CURRENT_DESKTOP XDG_SESSION_TYPE XDG_SESSION_DESKTOP
dbus-update-activation-environment --systemd WAYLAND_DISPLAY DISPLAY XDG_CURRENT_DESKTOP XDG_SESSION_TYPE XDG_SESSION_DESKTOP
systemctl --user start buoy-session.target
buoy-wm ppid=SUPERVISOR WAYLAND_DISPLAY=wayland-test
systemctl --user stop buoy-session.target'

failures=0

fail() {
    echo "case $1: $2" >&2
    failures=$((failures + 1))
}

# run_case NAME WM_MODE SYSTEMCTL_FAIL SIGNAL EXPECTED_STATUS
#
# The supervisor is started through `env --default-signal=INT` because a
# `&` child of this non-interactive bash inherits SIGINT ignored, so the
# INT case could never reach the supervisor otherwise. env execs the
# script, so $! is the supervisor's own pid, which is also what the stub
# WM logs as its $PPID.
run_case() {
    local name="$1" mode="$2" systemctl_fail="$3" signal="$4" expected_status="$5"
    local status=0 pid tries=0

    rm -f "$STUB_LOG" "$STUB_LOG.ready" "$STUB_LOG.pid"
    : > "$STUB_LOG"

    STUB_WM_MODE="$mode" STUB_SYSTEMCTL_FAIL="$systemctl_fail" \
        env --default-signal=INT "$tmp/libexec/buoy-session" 2> "$tmp/stderr" &
    pid=$!
    supervisor_pid="$pid"

    if [ -n "$signal" ]; then
        # The stub touches .ready only after its traps are installed and
        # its watchdog is running; signalling earlier races the
        # supervisor's own trap install.
        while [ ! -e "$STUB_LOG.ready" ]; do
            tries=$((tries + 1))
            if [ "$tries" -gt 50 ]; then
                fail "$name" "stub WM never became ready"
                cat "$tmp/stderr" >&2
                return
            fi
            sleep 0.1
        done
        kill -s "$signal" "$pid"
    fi

    wait "$pid" || status=$?
    supervisor_pid=""

    if [ "$status" -ne "$expected_status" ]; then
        fail "$name" "expected status $expected_status, got $status"
        cat "$tmp/stderr" >&2
    fi

    # A wrong ppid shows up in the diff rather than being filtered out:
    # only the supervisor's own pid is rewritten to the placeholder.
    if ! diff -u --label expected --label actual \
        <(printf '%s\n' "$expected") \
        <(sed "s/^buoy-wm ppid=$pid /buoy-wm ppid=SUPERVISOR /" "$STUB_LOG") >&2; then
        fail "$name" "stub log differs from the expected sequence"
        cat "$tmp/stderr" >&2
    fi

    if [ "$systemctl_fail" = 1 ] && ! grep -q '^buoy-session: ' "$tmp/stderr"; then
        fail "$name" "a failing systemctl was not reported on stderr"
    fi
}

run_case exit0 exit0 0 "" 0
run_case exit3 exit3 0 "" 3
run_case TERM signal 0 TERM 7
run_case INT signal 0 INT 7
run_case HUP signal 0 HUP 7
run_case systemctl-fails exit3 1 "" 3

if [ "$failures" -ne 0 ]; then
    echo "$failures case(s) failed" >&2
    exit 1
fi
