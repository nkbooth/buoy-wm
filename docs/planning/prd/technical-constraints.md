# Technical constraints
- Language: Rust, built from `codeberg.org/river/tinyrwm`'s Rust reference
  implementation as the starting skeleton
- Compositor: `river` 0.4+ (non-monolithic) — already correctly targeted in
  `~/Documents/code/framework/Containerfile` (commit `0d632c1`,
  `rpm-ostree install river`), no change needed there
- Protocol: `river-window-management-v1` only — no `river-layout-v3`, no
  `riverctl` tag/float-filter commands (those are river-classic-only, N/A here)
- No `wlr-layer-shell` (output-scoped, not tag-scoped — explicitly rejected).
  Resolved via UX design pass: `buoy-status-bar` does not render its own
  surface — it drives a `waybar` custom module instead (see
  architecture/components.md and architecture/external-integrations.md)
- Terminal: `foot`; multiplexer: `zellij`; picker: `fuzzel`
- Build/test: devcontainer via devpod — never install toolchains on the host
- Deployment: Fedora atomic host. The eventual `~/.config/river/init` launcher
  script is chezmoi-managed; the WM binary's own source is not
