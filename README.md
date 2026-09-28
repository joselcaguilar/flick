# Flick

Flick turns a camera into a gesture remote for Home Assistant. The engine is Rust, the desktop shell is Tauri, and the UI will be React. Status: **pre-alpha foundation**.

## Build and test

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo deny check licenses
```

Developer automation is exposed through `cargo xtask`.

## Specs and design

- Engineering source of truth: [`docs/spec/README.md`](docs/spec/README.md)
- Product context: [`PRODUCT.md`](PRODUCT.md)
- Visual system: [`DESIGN.md`](DESIGN.md)
