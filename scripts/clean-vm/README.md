# Clean-machine check

The tier-1 check the README defines — "install as the README says, pick
`buoy` at the display manager, get portals, bar and outputs, log out
cleanly" — run on one fresh VM per tier-1 release before a final tag
(`docs/PACKAGING.md`, release runbook step 4). It first ran for
v1.0.0-rc.3 (Story 5.7): Fedora 44, Fedora 45, Ubuntu 26.04 and Ubuntu
26.10 each passed every check it then had, before and after the tag.

Each VM is the release's own cloud image plus its desktop package set
(Fedora Workstation's environment group, `ubuntu-desktop-minimal`), not an
installer ISO. It is driven over ssh and over QMP (screenshots, clicks,
keys), so GDM is used the way a person uses it. The install script is
built from `README.md`'s fenced blocks by heading, so the VM runs what a
reader copies; `tests/clean-vm.sh` holds that generator, and `vms.py`, to
the README on every push.

Host needs: KVM, `qemu-system-x86_64`, `qemu-img`, `podman`, `python3`,
`git`, `ssh`, `scp`, `ssh-keygen`, `setsid`. State lives in `BUOY_VM_DIR`
(default `/var/tmp/buoy-vm`, about 30 GB with four desktop layers);
everything in it is rebuilt by these scripts, and Fedora's tmpfiles ages
`/var/tmp` after 30 days, so point it elsewhere to keep layers between
releases. QMP sockets go in `BUOY_VM_SOCKETS` (default
`$XDG_RUNTIME_DIR/buoy-vm`). The cloud images are checked against the sums
published beside them, not against the distributions' GPG signatures. The
seed server listens on loopback, so on a shared host another local user
could race for its port; run this on a single-user machine.

## Once per set of desktop layers

```sh
scripts/clean-vm/prepare.sh                      # ssh key, seeds (kept if present), images
scripts/clean-vm/layer.sh f44 desktop            # likewise f45, u2604, u2610
scripts/clean-vm/boot.sh f44 desktop 2244 8044 & # one ssh and one seed port per VM
scripts/clean-vm/desktop.sh f44 2244             # ~15 min, powers off when done
scripts/clean-vm/boot.sh f44 desktop 2244 8044 &
scripts/clean-vm/layercheck.sh f44 2244          # greeter shot; fails if anything of buoy's is installed
```

From here `<vm>.qcow2` is frozen: `boot.sh <vm> desktop` refuses once any
overlay sits on it, and every run is an overlay.

## Each run

Before the tag, on candidates built from the commit to be tagged (a dirty
checkout is refused, and `run_readme.sh` refuses candidates from any
commit but HEAD):

```sh
scripts/clean-vm/candidates.sh                   # one package per tier-1 release, each verify-replayed
scripts/clean-vm/layer.sh f44 pre
scripts/clean-vm/boot.sh f44 pre 2244 8044 &
scripts/clean-vm/run_readme.sh f44 2244 pre pre  # README script; exits with its status; reboots to GDM
scripts/clean-vm/session.sh f44 2244 pre         # login, logout, re-login, evidence
```

After the tag, with nothing substituted but the README's own 26.10 line:
the same with `post` (`run_readme.sh f44 2244 post post`), downloading the
published assets. Then:

```sh
python3 -I scripts/clean-vm/summary.py "${BUOY_VM_DIR:-/var/tmp/buoy-vm}" post
```

prints `== <vm>: passed/total` per VM and a `FAIL` line per failed check,
and exits 1 if there is any.

`session.sh` clicks `buoy` in the session menu and the password field
where `vms.py` says each release's greeter draws them on the 1280×800
screen (read from rc.3's screenshots). A new release's greeter may move
them: `session.sh <vm> <port> <label> greeter` stops after opening the
menu, so `shots/<vm>-<label>-gear.png` can be read and `vms.py` corrected
before a full pass, without logging in. Each VM proves one route to a
multiplexer (`vms.py`): f44 and u2604 the upstream zellij asset, f45
`cargo install` (8 GiB, it is OOM-killed in 4), u2610 tmux with the
`[defaults]` block and its `chmod 600`.

## What the logs hold

- `runs/<vm>-<label>.log`: the generator's `# SUBSTITUTED` lines, the
  clean check, the README script under `bash -x` with `set -e` and its
  `README-EXIT=` status, and each prompt `answer.py` typed `y` or Enter
  at. A rerun of the same label replaces it.
- `runs/<vm>-<label>-{check,logout,relogin}.log`: the evidence `check.sh`,
  `logout.sh` and `relogin.sh` print on the VM, which `summary.py` judges;
  `check.sh` also records the screenshot portal's access warning for a
  person to read.
- `shots/`: the greeter, session menu, session, logout and re-login
  screens.

A deviation found here is a README or package fix before the tag; one
found after the tag is the next release candidate, never a moved tag.
