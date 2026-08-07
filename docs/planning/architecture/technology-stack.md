# Technology stack
- Language: Rust (2021 edition)
- Key libraries/frameworks: `wayland-client` (river-window-management-v1
  binding, starting from the `tinyrwm` Rust reference implementation),
  `serde` + `serde_json` (IPC messages), `tokio` or `smol` for the async
  event loop (TBD at implementation time — not a v1 architectural commitment)
- Build tooling: `cargo`, built inside a devcontainer via devpod
