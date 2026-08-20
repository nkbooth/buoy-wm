# Third-party notices

`buoy-wm` as a whole is licensed under the Reciprocal Public License 1.5
(see [LICENSE.md](LICENSE.md)). The material listed here arrived under
different terms, which continue to apply to it.

## river Wayland protocol definitions

`wm/protocol/*.xml` are unmodified copies of protocol definitions from the
[river compositor](https://codeberg.org/river/river):

| File | Copyright | License |
| --- | --- | --- |
| `river-window-management-v1.xml` | © 2024 Isaac Freund | MIT |
| `river-input-management-v1.xml` | © 2025 Isaac Freund | MIT |
| `river-layer-shell-v1.xml` | © 2025 Isaac Freund | MIT |
| `river-libinput-config-v1.xml` | © 2025 Isaac Freund | MIT |
| `river-xkb-bindings-v1.xml` | © 2025 Isaac Freund | MIT |

Each file carries its own full MIT text in a `<copyright>` element. They are
vendored so `wayland-scanner` can generate bindings at build time; they are
not modified, and RPL 1.5 is not applied to them.

## tinyrwm

`wm/src/main.rs` and `wm/Cargo.toml` began as the Rust example from
[tinyrwm](https://codeberg.org/river/tinyrwm) by Julian Andrews, distributed
under 0BSD. Both files have since been substantially rewritten and extended;
the surviving tinyrwm structure is the Wayland registry-binding and
`Dispatch` scaffolding.

0BSD places no conditions on use, modification, or redistribution, so the
derived work is distributed under RPL 1.5. The original remains available
under 0BSD from tinyrwm upstream. Attribution is retained in the header of
each affected file as a courtesy, not an obligation.

## Rust dependencies

Build-time and runtime crate dependencies (`wayland-client`,
`wayland-backend`, `wayland-scanner`, `serde`, `serde_json`, `toml`,
`bitflags`) carry their own licenses — predominantly MIT and Apache-2.0.
`Cargo.lock` pins the exact versions; run `cargo license` or
`cargo about generate` against a checkout for the authoritative list.

## Runtime programs

`river`, `foot`, `fuzzel`, `zellij`, and `waybar` are external programs that
`buoy-wm` executes or cooperates with. None are bundled or redistributed
here, and none are linked into any `buoy-wm` binary.
