# RTMCL

A Rust CLI/TUI launcher prototype for Minecraft: Java Edition, built on
LightyLauncherLib. The project targets Windows x64 and Linux x64; current local
acceptance work focuses on Windows.

[中文说明](README.zh-CN.md) · [Documentation](docs/README.md) ·
[Application review status](docs/app-review-status.md)

## Project status

RTMCL is an early P0 development snapshot, not a production release. The CLI
implements the current install and launch workflow; the TUI is a minimal shell.
Full account, task, settings, and log pages are planned.

- Earlier Vanilla 1.21.1 and Fabric Loader 0.16.10 smoke tests reached the game
  menu using a development login mode that has now been removed. Those screenshots
  do not validate the current Microsoft-only launch path.
- Windows has 36 automated tests covering accounts, downloads, installation,
  storage, process supervision, and CLI/TUI behavior.
- WSL / WSLg acceptance is temporarily suspended. Ubuntu CI remains configured;
  historical Linux checks are not a guarantee for later changes.
- Microsoft Device Code authorization, Xbox Live, and XSTS passed a real Windows
  check. Minecraft Services token exchange returned HTTP 403. API access review
  and end-to-end Microsoft account acceptance remain outstanding.
- Launch now requires a Microsoft account and successful online authentication.
  The local test login, offline identity generator, and offline launch option
  have been removed; old local test records cannot be selected.

See the [validation record](docs/p0-validation.zh-CN.md) for evidence and limits,
including the outstanding clean-cache mirror-only installation test.

## Implemented

- Isolated Vanilla and Fabric instances with persisted configuration.
- Official and BMCLAPI source routing, strict source selection, ordered fallback,
  resumable downloads, size/hash verification, and atomic file commits.
- Explicit Java executables, real version/architecture detection, and managed
  Adoptium runtimes verified with SHA-256.
- Separate launcher and game logs, instance operation locks, and an independent
  supervisor that lets the game keep running when the interactive CLI exits.
- Microsoft Device Code integration with system credential storage code and an
  explicit session-only fallback; real persistence/refresh acceptance is pending.

## Build and try

The repository pins **Rust 1.98.0**. Install the platform C/C++ build tools as
well. Game files and Java archives are downloaded separately and are not included
in this repository.

```sh
cargo build --workspace --locked
cargo run --locked -- doctor
cargo run --locked -- --data-dir .rtmcl/demo config init
cargo run --locked -- --data-dir .rtmcl/demo instance create "Vanilla test" --game-version 1.21.1
cargo run --locked -- --data-dir .rtmcl/demo instance list
```

Use the instance UUID returned by the create command:

```sh
cargo run --locked -- --data-dir .rtmcl/demo p0 --source mirror-first install <INSTANCE_ID>
cargo run --locked -- --data-dir .rtmcl/demo account login --client-id <CLIENT_ID>
cargo run --locked -- --data-dir .rtmcl/demo account list
cargo run --locked -- --data-dir .rtmcl/demo p0 launch <INSTANCE_ID> --account <ACCOUNT_ID>
```

The launch commands require successful Microsoft/Minecraft authentication;
they cannot complete while the current Minecraft Services HTTP 403 remains.
Authentication uses this project's own Client ID, supplied through `--client-id`
or `RTMCL_MICROSOFT_CLIENT_ID`; it does not require a redirect URI. See
[Microsoft login setup](docs/microsoft-login.zh-CN.md) and
[review status](docs/app-review-status.md).

Configure downloads, memory, Java, and the game window with
[config.example.toml](config.example.toml). Pass `--java /absolute/path/to/java`
to select Java explicitly, or `--wait` to wait for the game to exit. Run without a
subcommand to open the minimal TUI; interactive controls are WASD / arrow keys,
Enter / Space, and Esc.

## Repository layout

```text
crates/launcher-app/        Application services, domain types, and integration tests
crates/launcher-terminal/   CLI, minimal TUI, and process worker entry point
docs/                      Architecture, roadmap, validation, and screenshots
scripts/                   Upstream release auditing tools
vendor/lighty/             Fixed, audited Lighty snapshot and documented patches
config.example.toml        Example application configuration
```

Local build outputs and runtime data belong in `target/` and `.rtmcl/`; both are
excluded from Git. No account files, credentials, game archives, or raw runtime
logs are part of the source repository.

## Development

```sh
cargo fmt --package launcher-app --package launcher-terminal -- --check
cargo clippy --workspace --all-targets --no-deps --locked -- -D warnings
cargo test --workspace --locked
cargo build --workspace --locked
```

Format only the two application crates to preserve the audited upstream snapshot.
Lighty's no-tracing build emits some upstream unused-code warnings; the project's
own Clippy checks deny warnings. See [CONTRIBUTING.md](CONTRIBUTING.md).

## License and attribution

RTMCL's own code is licensed under the [MIT License](LICENSE). The vendored
Lighty snapshot retains its separate MIT copyright notice; provenance and other
notices are in [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md).

RTMCL is an independent project, not an official Minecraft product, and is not
affiliated with or endorsed by Mojang or Microsoft.
