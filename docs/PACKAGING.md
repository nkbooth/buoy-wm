# Packaging plan

How `buoy` gets from a git tag to somebody's machine, which channels are
worth maintaining, and what each one costs.

## Release model

Semantic versioning, single tag per release, `v`-prefixed (`v0.1.0`). All
three crates share one version through `[workspace.package]` — they are one
product and are never installed apart, so independent versioning would only
create combinations nobody tests.

`.github/workflows/release.yml` fires on a `v*` tag and is the single source
of every published artifact. It refuses to build if the tag disagrees with
`Cargo.toml`, so a mistagged release fails loudly instead of shipping.

Each release produces one tarball,
`buoy-wm-<version>-x86_64-unknown-linux-gnu.tar.gz`, containing the three
binaries, `install.sh`, `README.md`, `LICENSE.md`, `NOTICE.md`, and
`config.example.toml`; plus a `.sha256` and a GitHub build-provenance
attestation.

Two constraints shape everything downstream:

1. **The three binaries must land in one directory.** The WM resolves
   `buoy-tag-picker` through `current_exe()`'s parent. Any packaging that splits
   them breaks the pickers at runtime, with no build-time signal.
2. **Only `buoy-wm` goes on `PATH`.** `buoy-tag-picker` and `buoy-status-bar` are too
   generic to own those names system-wide, and nothing resolves them through
   `PATH`.

Binaries are built on `ubuntu-22.04`, not `ubuntu-latest`. glibc is forward-
but not backward-compatible, so the runner's glibc becomes the floor imposed
on every user — 22.04 sets that floor at 2.35 rather than 2.39.

## Channels

Ranked by value per unit of maintenance.

### 1. GitHub release tarball — primary

Zero infrastructure, works on every distro, and the thing every other
channel is derived from. `install.sh` is shared verbatim with the source
checkout (it detects binaries sitting beside it), so there is no second copy
to drift.

Installs to `~/.local/lib/buoy-wm` with a `~/.local/bin/buoy-wm` symlink —
no root, and nothing that a broken build can take out of the system.

### 2. Fedora COPR (RPM) — recommended primary for Fedora/atomic hosts

`packaging/rpm/buoy-wm.spec`. The only channel that layers cleanly onto
`rpm-ostree`, which matters for the atomic host this was developed on. Also
the only channel that can ship a `wayland-sessions` entry, so the display
manager offers "buoy" as a login option instead of requiring a hand-written
`~/.config/river/init`.

COPR builds have no network, so crates must be vendored:

```sh
cargo vendor --versioned-dirs vendor
tar -czf buoy-wm-0.1.0-vendor.tar.gz vendor
```

Upload that alongside the source tarball and `buoy.desktop` as the spec's
`Source1`/`Source2`.

**Caveat, stated plainly:** RPL 1.5 does not appear on Fedora's reviewed
allowed-licenses list. COPR is a personal build service and is not governed
by that list, so this works — but Fedora proper is closed to `buoy` unless
RPL 1.5 is submitted for and passes legal review. Do not plan around Fedora
inclusion.

### 3. Homebrew tap — `nkbooth/homebrew-buoy`

`packaging/homebrew/buoy-wm.rb`, built from source against Homebrew's own
`wayland` formula.

**This will not go into homebrew-core**, for two independent reasons worth
knowing before spending effort on it: homebrew-core requires notability
(roughly 30+ stars/forks/watchers) that a new personal project does not
have, and Homebrew on Linux is a poor fit for login-session software — a
display-manager entry pointing into `/home/linuxbrew/.linuxbrew` is
unconventional at best. A custom tap is the realistic route, and it exists
mainly so `brew`-centric users are not locked out:

```sh
brew tap nkbooth/buoy
brew install buoy-wm
```

Per release, the tap's copy of the formula needs exactly two lines changed
(`url`, `sha256`). `brew bump-formula-pr` automates it.

### 4. crates.io — `cargo install` fallback

Zero-infrastructure coverage for any distro the other channels miss. All
three crates install into `~/.cargo/bin`, which satisfies the
one-directory constraint for free.

**Blocked until the helper crates are renamed.** `buoy-tag-picker` and
`buoy-status-bar` are far too generic to claim on crates.io and are very likely
already taken. Rename the *packages* to `buoy-tag-picker` and
`buoy-status-bar` while pinning `[[bin]] name = "buoy-tag-picker"` /
`"buoy-status-bar"` so the binary names — and therefore the runtime sibling
lookup, `install.sh`, the spec, and the formula — are untouched. This needs
a `cargo` run to regenerate `Cargo.lock`, so it is a deliberate step rather
than a side effect.

Then, in dependency order:

```sh
cargo publish -p buoy-tag-picker
cargo publish -p buoy-status-bar
cargo publish -p buoy-wm
```

Users install all three:

```sh
cargo install buoy-wm buoy-tag-picker buoy-status-bar
```

## Considered and not planned

**AUR — the strongest case for adding a channel.** river's user base skews
heavily toward Arch, so an AUR `PKGBUILD` would plausibly reach more actual
users than Homebrew and COPR combined. It was left out of this round only
because nothing here is validated on Arch yet. If one more channel gets
added, this is the one.

**Nix flake.** Overlaps the same audience as AUR and would give reproducible
builds and a `nix run` demo path. Real value, real ongoing cost; revisit if
demand appears.

**Debian/Ubuntu packaging.** Not viable. RPL 1.5's obligations — source
disclosure triggered by internal deployment, plus an affirmative duty to
notify the community of source availability — are very unlikely to clear
DFSG. This is a direct consequence of the license choice, not of packaging
effort, and no amount of packaging work changes it.

**Flatpak / Snap.** Category mismatch. Both sandbox applications; a window
manager is the session, needs the compositor's private protocol socket, and
spawns arbitrary user programs.

**Static musl builds.** Would remove the glibc floor, but `wayland-client`
links the system libwayland, so a musl target needs a musl libwayland
sysroot. The `ubuntu-22.04` glibc floor solves the same problem for
materially less work.

**AppImage.** Solves distribution for GUI applications launched by a user.
A WM is `exec`'d by a compositor at session start; the extra indirection
buys nothing.

## Release runbook

1. Bump `version` in the root `[workspace.package]`.
2. Update `packaging/rpm/buoy-wm.spec` (`Version:` and `%changelog`).
3. `cargo build --workspace --release && cargo test --workspace` — green.
4. Commit, `git tag -a v<x.y.z>`, push the tag. CI builds and publishes.
5. Verify the tarball on a clean machine: extract, run `install.sh`, start a
   nested river session before trusting it as a login session.
6. `brew bump-formula-pr` against the tap, or edit `url`/`sha256` by hand.
7. `cargo vendor` and submit the COPR build.
8. Once the crates are renamed: `cargo publish` in dependency order.

Step 5 is not optional. A `buoy-wm` binary that fails to start means a black
screen and a bounce back to the display manager, with no shell to recover
from.
