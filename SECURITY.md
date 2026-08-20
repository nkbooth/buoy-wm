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

**The IPC socket is unauthenticated by design.** `buoy-wm` listens on a Unix
domain socket at `$XDG_RUNTIME_DIR/buoy-wm.sock`, protected only by
filesystem permissions. Any process running as the same user can connect and
drive it. That surface is deliberately narrow — the entire request set is
`get-state`, `create-tag`, `toggle-tag`, and `switch-tag`. There is no
request that executes a command, reads a file, or writes anywhere. A hostile
local process could read your tag names and window app-ids, and rearrange
your workspaces; it could not use `buoy` to gain anything it did not already
have as your user.

**`exec` keybinds run arbitrary commands, from your own config file.** The
`{ exec = "..." }` action runs through `sh -c` with full shell semantics.
`~/.config/buoy/config.toml` is therefore as trusted as your shell profile.
Do not source one from an untrusted place.

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
