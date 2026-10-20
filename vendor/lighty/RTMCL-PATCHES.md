# RTMCL local Lighty patch boundary

Upstream: Lighty-Launcher/LightyLauncherLib, commit
`94bd0553beeb89fa7868eb0e0a9f446ad85af793`, version `26.9.3`.
The original MIT notice remains in LICENSE and source files. This is a vendored,
fixed snapshot; no upstream PR or remote fork has been published.

All nine crates.io archives were independently SHA-256 checked and their src/
files and Cargo.toml.orig compared with this commit before local modifications.
Evidence: [release audit](../../docs/lighty-release-audit.json); audit script:
[audit-lighty.ps1](../../scripts/audit-lighty.ps1). The event crate does not publish
a rust_version field; the integrated workspace requires Rust 1.98.

| File | Local change |
| --- | --- |
| crates/core/src/download_context.rs | Generic task-scoped metadata fetch injection. Default remains upstream HTTP; optional SHA1 checked before parsing. |
| crates/core/src/lib.rs | Export download context. |
| crates/core/src/app_state.rs | Explicit LauncherPaths initialization; one initialization per process remains a restriction. |
| crates/loaders/src/loaders/vanilla/vanilla.rs | Route manifest, version JSON and asset index; verify JSON SHA1; apply library OS rules. |
| crates/loaders/src/loaders/fabric/fabric.rs | Route Fabric profile JSON. |
| crates/loaders/src/utils/maven.rs | Route Fabric checksum sidecars; omit optional HEAD size probing rather than bypassing context. Forge-only base probing remains upstream and is not enabled. |
| crates/loaders/src/types/version/version_metadata.rs | Serialize normalized Version/MainClass/JavaVersion for atomic installation records. |
| crates/launch/src/arguments/arguments.rs | Log argument count rather than sensitive argv. RTMCL also leaves upstream tracing disabled. |
| crates/auth/src/microsoft.rs | Honor device-code interval/expiry and slow_down, following Microsoft documentation and RFC 8628. |

Some edited files were rustfmt-normalized; unmodified source files retain the
upstream bytes. Format only launcher-app and launcher-terminal from the outer
workspace to avoid reformatting this snapshot.

RTMCL reuses loader normalization and structured arguments. It owns download
execution, Java selection and process supervision, avoiding high-level run().
No BMCLAPI constants, mirror policy or RTMCL dependency is added to Lighty.

The former RTMCL local test login has been removed. RTMCL no longer constructs
offline profiles or invokes the upstream offline provider. Generic providers in
the audited upstream snapshot are retained for provenance; they are not exposed
by the application login/launch path. No-tracing builds emit upstream unused/dead-code warnings; project
Clippy uses --no-deps and denies warnings in the RTMCL crates.
