# Deployment
Runs as a long-lived process launched from `~/.config/river/init` (chezmoi-
managed dotfile) under a `river` 0.4+ session on the Fedora atomic host
(`framework` image). Built and tested inside a devcontainer via devpod — the
Rust toolchain is never installed on the host. Targets both laptop-only and
docked (42" widescreen) monitor configurations.
