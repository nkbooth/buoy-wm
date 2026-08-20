# Technology stack
- Language: Rust (2024 edition, MSRV 1.85)
- Key libraries/frameworks: `wayland-client` (river-window-management-v1
  binding, starting from the `tinyrwm` Rust reference implementation),
  `serde` + `serde_json` (IPC messages), `toml` (config file), `bitflags`
  (protocol enums). No async runtime: the WM is a single-threaded Wayland
  event loop with a blocking IPC accept, so `tokio`/`smol` — floated here
  during planning — were never needed.
- Build tooling: `cargo`, built inside a devcontainer via devpod
