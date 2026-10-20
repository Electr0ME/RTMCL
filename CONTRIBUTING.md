# Contributing

RTMCL is a P0 prototype. Read the [project status](README.md) and
[architecture](docs/architecture.zh-CN.md) before changing the installation,
authentication, or process boundaries.

## Development

Use the pinned Rust 1.98.0 toolchain and a platform C/C++ build toolchain.
Keep local game data under `.rtmcl/`, which is ignored by Git.

```sh
cargo fmt --package launcher-app --package launcher-terminal -- --check
cargo clippy --workspace --all-targets --no-deps --locked -- -D warnings
cargo test --workspace --locked
cargo build --workspace --locked
```

Current local acceptance work targets Windows. WSL / WSLg acceptance is
temporarily suspended; existing Linux results are historical evidence.
Windows and Ubuntu CI remain configured.

Format only the application crates. Do not reformat the entire vendored Lighty
snapshot with `cargo fmt --all`. Document upstream modifications in
`vendor/lighty/RTMCL-PATCHES.md` and preserve its license notices.

## Changes and reports

- Describe the observed problem, resulting behavior, and relevant validation.
- Keep source selection in the application; do not hardcode BMCLAPI into Lighty.
- Never commit account files, credentials, device codes, raw authentication
  responses, or complete game command lines. Use synthetic values in tests.
- Report authentication failures with the stage and HTTP status, without tokens.
- Keep real external-service checks separate from deterministic local tests.

Contributions to RTMCL's own code are provided under its [MIT license](LICENSE).
