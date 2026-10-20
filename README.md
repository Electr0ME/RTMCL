# RTMCL

**Rust TUI Minecraft Launcher** is an independent, open-source launcher for
Minecraft: Java Edition, built with Rust and LightyLauncherLib.

[中文说明](README.zh-CN.md)

RTMCL aims to provide a terminal interface for managing game instances, accounts,
Java runtimes, downloads, and game processes. Target platforms are Windows x64
and Linux x64; current local validation focuses on Windows.

## Current capabilities

- Isolated Vanilla and Fabric instances, installation, and game launch.
- Official and BMCLAPI download sources, ordered fallback, resumable downloads,
  and size/hash verification.
- Java detection and selection, plus managed runtime installation.
- Independent game process supervision and per-session game logs.
- A working CLI and a minimal TUI instance browser.

## Development status

The project is an early development prototype. P0 installation and launch
infrastructure is implemented, and P1 application services and CLI work are in
progress. The complete TUI and packaged releases are planned.

Windows development-account launches have been verified. Microsoft authorization,
Xbox Live, and XSTS succeeded in a real check, but Minecraft Services token
exchange returned HTTP 403. Minecraft API access and full authenticated gameplay,
ownership, and credential refresh acceptance remain pending. This project does
not claim Microsoft or Mojang approval.

Clean-cache installation through real mirrors with official endpoints unavailable
also remains unverified. New WSL/WSLg acceptance checks are temporarily suspended;
Windows and Ubuntu CI are configured.

## Accounts

Normal builds require a bound Microsoft account with freshly checked Java
ownership and play entitlements when creating or launching a local profile.
Removing the owner or failing authentication blocks access. Local profiles do
not provide an authenticated Microsoft multiplayer session.

An explicit `dev-local-accounts` feature permits unbound profiles for local debug
development. It is off by default, rejected by standard release builds, and its
development-only profiles cannot be used by normal binaries. This exception is
part of the disclosed source behavior and does not imply API approval.

## Build requirements

The project pins Rust **1.98.0** and LightyLauncherLib **26.9.3**. Building requires
the platform C/C++ toolchain. Run `cargo build --workspace --locked` to build and
`cargo run --locked -- --help` for the CLI. Settings are illustrated in
[config.example.toml](config.example.toml).

Minecraft files, Java archives, account credentials, and local development
records are excluded from this source repository.

## License

RTMCL's own code is licensed under the [MIT License](LICENSE). Lighty retains
its separate license and copyright notice; see
[third-party notices](THIRD_PARTY_NOTICES.md) and the
[local patch boundary](vendor/lighty/RTMCL-PATCHES.md).

RTMCL is not an official Minecraft product and is not affiliated with or endorsed
by Mojang or Microsoft.
