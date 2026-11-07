# Packaging plan

How `buoy` gets from a git tag to somebody's machine, which channels are
worth maintaining, and what each one costs.

## Release model

Semantic versioning, one tag per release. A tag is `vX.Y.Z` or
`vX.Y.Z-rc.N` and nothing else (ADR-012). All three crates share one version
through `[workspace.package]` — they are one product and are never installed
apart, so independent versioning would only create combinations nobody
tests. One tag maps to one version form per packaging format; the table is
`scripts/release-version.sh`'s:

| key | `v1.0.0-rc.1` | `v1.0.0` | used by |
| --- | --- | --- | --- |
| `cargo` | `1.0.0-rc.1` | `1.0.0` | `Cargo.toml`, `Cargo.lock`, the tarball name |
| `rpm` | `1.0.0~rc.1` | `1.0.0` | the spec's `Version:` |
| `deb` | `1.0.0~rc.1-1` | `1.0.0-1` | `debian/changelog` (Story 5.5) |
| `formula` | `v1.0.0-rc.1` | `v1.0.0` | the Homebrew `url` tag |
| `prerelease` | `yes` | `no` | `gh release create --prerelease` |

rpm cannot carry a hyphen in `Version:` and dpkg cannot in the upstream
part of a version, and `~` is the one character both sort *before* nothing
at all, so `1.0.0~rc.1` upgrades to `1.0.0` where `1.0.0.rc.1` never would.

`.github/workflows/release.yml` fires on a `v*` tag and is the single source
of every published artifact. It refuses a tag that disagrees with any
manifest, and `ci.yml` runs the same check on every push with the tag
derived from `Cargo.toml`, so a half-bumped tree is red before anyone tags
it. An `rc.N` tag is published as a GitHub pre-release.

Each release carries six assets and one attestation:
`buoy-wm-<version>-x86_64-unknown-linux-gnu.tar.gz` (the three binaries,
`install.sh`, `README.md`, `LICENSE.md`, `NOTICE.md` and
`config.example.toml`), `buoy-wm-<version>-vendor.tar.gz` (the vendored
crates) and `buoy-wm-<version>-1.fc44.x86_64.rpm`, each beside a `.sha256`
whose line carries the bare basename, so `sha256sum -c` works wherever the
pair is downloaded. `<version>` is the tag without its `v`, so the RPM is
named with the hyphen while its header says `~`: GitHub rewrites `~` in
asset names, and `rpm -qp` on the file reports `1.0.0~rc.1-1.fc44`. The
build-provenance attestation names all three archives as subjects, and
`gh attestation verify <file> --repo nkbooth/buoy-wm` works for each.

Three jobs produce that. `build`, on the runner, builds and tests, stages
the tarball, runs `cargo vendor --locked --versioned-dirs` and tars the
result reproducibly, and makes a `git archive --prefix=buoy-wm-<version>/`
of the tag as the spec's `Source0` — relayed to the next job and never
published, because GitHub serves the tag archive itself. `rpm`, in a
`registry.fedoraproject.org/fedora:44` container, runs
`dnf builddep packaging/rpm/buoy-wm.spec` and then
`scripts/build-rpm.sh rpm-sources dist`: a full `rpmbuild -bb` with
`%check`, on Fedora's own rustc (ADR-014). `publish` makes the one
attestation and runs `gh release create` with the `CHANGELOG.md` entry for
the tag as the release notes (cut in `build` by `scripts/release-notes.sh`)
— `--prerelease` for an rc, and `--clobber` on a re-run so a transient
failure replaces the assets instead of failing on the existing release.

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

The GitHub release assets are the source of truth (ADR-011, which replaces
the ranking this section used to open with); COPR, a PPA and the Homebrew
tap are optional mirrors made after the tag, never something a release
waits on.

### 1. GitHub release tarball — primary

Zero infrastructure, works on every distro, and the thing every other
channel is derived from. `install.sh` is shared verbatim with the source
checkout (it detects binaries sitting beside it), so there is no second copy
to drift.

Installs to `~/.local/lib/buoy-wm` with a `~/.local/bin/buoy-wm` symlink —
no root, and nothing that a broken build can take out of the system.

### 2. GitHub release RPM — Fedora 44

`packaging/rpm/buoy-wm.spec`, built by `release.yml` in a `fedora:44`
container and attached to the release. The only channel that layers cleanly
onto `rpm-ostree`, which matters for the atomic host this was developed on,
and the only one that ships the session. Beyond the three binaries the
package installs:

- `/usr/share/wayland-sessions/buoy.desktop` — the login entry, so the
  display manager offers "buoy" instead of requiring a hand-written
  `~/.config/river/init`.
- `/usr/libexec/buoy-wm/buoy-session` — the supervisor the entry runs
  (ADR-010). `/usr/libexec/buoy-wm` holds four files, not three, because
  the supervisor resolves `buoy-wm` as its own sibling.
- `/usr/lib/systemd/user/buoy-session.target` — the user target the
  supervisor keeps up exactly as long as the WM runs.
- `/usr/share/xdg-desktop-portal/buoy-portals.conf` — the portal routing;
  the entry's `DesktopNames=buoy` selects it.

Install by URL: the README's "Fedora 44: the RPM" block downloads, verifies
and installs it, and `rpm-ostree install` accepts the `https://` URL
directly on an atomic host, which is how the author's image will take it
(Story 4.10). The spec `Requires: river >= 0.4`, `systemd` and `dbus-tools`,
`Recommends` `foot`, `fuzzel`, `zellij` and the two portal backends, and
`Suggests` `waybar`. `zellij` is not in Fedora's archive, so that
`Recommends` is inert there: dnf ignores an unavailable weak dependency,
the line records what the default config expects, and the README says where
to get it.

**Offline rebuild.** The spec's `%build` runs `cargo --offline`, so the
crates arrive as `Source1`, the vendor tarball, named exactly
`%{name}-%{upstream_version}-vendor.tar.gz` —
`buoy-wm-1.0.0-rc.1-vendor.tar.gz` for rc.1, the tag's hyphen and not the
spec's `Version:`. To rebuild:

1. Download the release's vendor tarball, or make one from the tag with
   `cargo vendor --locked --versioned-dirs vendor` and
   `tar -czf buoy-wm-<upstream_version>-vendor.tar.gz vendor`.
2. Download `Source0` from the spec's own URL, the tag archive GitHub
   serves.
3. Put both in one directory. In a `fedora:44` container run
   `dnf install rpm-build dnf5-plugins` (the image has neither `rpmbuild`
   nor the `builddep` subcommand), `dnf builddep packaging/rpm/buoy-wm.spec`,
   then `scripts/build-rpm.sh <that directory> <a fresh out-dir>`; the
   script does not clear a non-empty out-dir.

`rpmlint` on the published file reports `W: non-coherent-filename`, because
the asset carries the tag's hyphen while the header carries the tilde, and
`W: invalid-license RPL-1.5`, because RPL is not on Fedora's list. Both are
expected.

**Caveat, stated plainly:** RPL 1.5 does not appear on Fedora's reviewed
allowed-licenses list. COPR is a personal build service and is not governed
by that list, so a mirror there works — but Fedora proper is closed to
`buoy` unless RPL 1.5 is submitted for and passes legal review. Do not plan
around Fedora inclusion.

### 3. Homebrew tap — `nkbooth/homebrew-buoy`

`packaging/homebrew/buoy-wm.rb`, built from source against Homebrew's own
`wayland` formula. The tap carries no session entry, supervisor, target or
portal config; a Homebrew install starts the WM from `~/.config/river/init`
as the README's from-source section shows, and its supervisor recipe needs
a checkout, since the tap installs the binaries only.

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

1. Bump: `version` in `[workspace.package]` and `cargo update -w`; the
   spec's `Version:`, `%global upstream_version` and a `%changelog` entry;
   the formula's `url`. Run `scripts/release-version.sh --check v<x.y.z>`
   until it is quiet — it names every file that disagrees. `ci.yml` runs
   the tagless form on every push, so a half-bump is red before the tag.
2. `cargo build --workspace --release && cargo test --workspace` — green.
3. Commit, `git tag -s v<x.y.z> -m 'buoy-wm <x.y.z>'` (signed, as every
   commit in this repository is; `-rc.N` for a release candidate), push
   the tag. The release notes are the `CHANGELOG.md` entry for the tag,
   so a tag whose entry is missing fails in `build`'s first minute.
   `release.yml` builds and publishes: watch `build`, `rpm` (two to four
   minutes in the container) and `publish` (one attestation with three
   subjects in the step summary, six assets, the pre-release flag on an
   rc).
4. Verify. Download the RPM and the tarball; `sha256sum -c` and
   `gh attestation verify` each. `dnf install` the RPM in a fresh
   `fedora:44` container to prove the dependency resolution, then on a real
   Fedora host pick **buoy** at the display manager and check the portal
   (ScreenCast, Screenshot), the bar and logout. Extract the tarball on a
   clean machine, run `install.sh` and start a nested river before trusting
   it. This step is not optional: a `buoy-wm` binary that fails to start
   means a black screen and a bounce back to the display manager, with no
   shell to recover from.
5. Mirrors, after the tag (Story 6.5): `brew bump-formula-pr` against the
   tap, or edit `url`/`sha256` by hand; COPR from `Source0`'s URL plus the
   release's vendor tarball; once the crates are renamed, `cargo publish`
   in dependency order.
