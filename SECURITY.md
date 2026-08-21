# Security

## Reporting

Report vulnerabilities through GitHub's private advisory form on
[the repository's Security tab](https://github.com/nkbooth/buoy-wm/security/advisories/new),
not as a public issue. `buoy` is a personal project with no service-level
commitment, so expect a best-effort response rather than a fixed timeline.

## Threat model

`buoy` is a single-user desktop session component. It holds no credentials,
opens no network sockets, and parses no untrusted input from outside the
user's own session. The trust boundary is the user account: anything already
running as you is inside it.

Three surfaces are worth naming explicitly.

**The IPC socket authenticates the user, not the process.** `buoy-wm` listens
on a Unix domain socket at `$XDG_RUNTIME_DIR/buoy-wm.sock`. Three controls
apply, and the first is the one that matters: every connection's credentials
are read with `SO_PEERCRED` and refused unless the peer runs as the same uid.
The kernel stamps those credentials at `connect(2)` and userspace cannot
forge them, so this holds independently of any filesystem race. Both clients
apply the same check in reverse and refuse to talk to a server that is not
running as you, so a socket squatted by another user is a failed connection
rather than a silent capture of your tag names and selections. Second, the WM
refuses to start its IPC server unless `$XDG_RUNTIME_DIR` is an absolute path
to a directory you own with no group or other permission bits — there is no
`/tmp` fallback, and a session without a usable runtime directory runs with
no IPC at all rather than with a socket somewhere shared. Third, the socket
itself is `0600`.

Within the user account there is deliberately no further authentication: any
process running as you can connect and drive it. That surface is narrow —
the entire request set is `get-state`, `create-tag`, `toggle-tag`, and
`switch-tag`.

One of those does spawn a process. A `switch-tag` for a tag whose pinned
terminal has not started yet runs the configured terminal and `zellij`
(`wm/src/ipc/server.rs:187`). The command and its arguments come entirely
from your own config, never from the request — the tag name reaches the child
only as a `zellij` session argument, passed as argv rather than through a
shell, so it cannot inject a command. The request set carries no command
string of its own, reads no file, and writes nowhere. A submitted tag name is
validated where it enters the registry — non-empty, at most 64 bytes, and
free of path separators and control characters — and one connection may
create at most eight tags before it is closed, so the 64-slot registry cannot
be spent in a single burst.

A second `buoy-wm` cannot quietly take over the endpoint either: a socket
path that answers a connection attempt belongs to a live instance and the
newcomer refuses to start rather than unlinking it, and an inode at that path
that is not a socket is left alone rather than deleted.

A hostile local process running as you could therefore read your tag names
and window app-ids, rearrange your workspaces, and cause up to 64 terminal
sessions to be spawned. It could not use `buoy` to run a command of its choosing, or to
gain anything it did not already have as your user.

**`exec` keybinds run arbitrary commands, from your own config file.** The
`{ exec = "..." }` action runs through `sh -c` with full shell semantics.
`~/.config/buoy/config.toml` is therefore as trusted as your shell profile.
Do not source one from an untrusted place. Since 0.1, that expectation is
also checked rather than merely stated: a config file that is group- or
world-writable, or owned by another user, is loaded but reported, the way
bash reports a world-writable profile. Only writability is checked — `644`
is a normal, accepted mode.

**A malformed config does not stop startup.** `buoy` reports the error on
stderr and falls back to built-in defaults. This is a deliberate
availability trade: refusing to start would leave you in a session with no
terminal from which to fix the file. It does mean a config error can silently
leave you on default bindings rather than your own — check stderr if
bindings are not what you expect.

## Supply chain

Release binaries are built by
[the release workflow](.github/workflows/release.yml) on GitHub-hosted
runners and carry a build-provenance attestation. Verify one with:

```sh
gh attestation verify buoy-wm-<version>-x86_64-unknown-linux-gnu.tar.gz \
  --repo nkbooth/buoy-wm
```

Dependencies are pinned in `Cargo.lock`, which CI enforces with `--locked`,
and reviewed monthly by Dependabot. The dependency tree is deliberately
small: Wayland bindings, `serde`, `toml`, and `bitflags`.
