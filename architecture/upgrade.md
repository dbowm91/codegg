# Upgrade Module

Self-upgrade functionality via GitHub releases.

## Purpose

Checks the GitHub releases API for a newer version and prints manual
install instructions. The `upgrade()` function can perform the install
but is not wired to the CLI.

## Where It Lives

- `src/upgrade/mod.rs` — version check and upgrade logic
- `src/main.rs:1016-1035` — `cmd_upgrade()` CLI handler

## How It Works

### CLI Command (`codegg upgrade`)

`cmd_upgrade()` in `src/main.rs:1016` calls `upgrade::check_for_updates()`,
compares with `CARGO_PKG_VERSION`, and prints manual install instructions:

```
curl -fsSL https://codegg.ai/install.sh
```

It does **not** call `upgrade()`.

### Version Check

`check_for_updates()` sends a GET to
`https://api.github.com/repos/dbowm91/codegg/releases/latest`
with a 10-second timeout. Parses `tag_name`, strips leading `v`,
and compares with the compiled `VERSION`.

### Install Function (defined, not called)

`upgrade()` validates semver, then runs:

```
curl -fsSL https://codegg.ai/install.sh
```

with `INSTALL_VERSION=v{latest}` in a sanitized environment
(`env_clear()` + only `PATH`). Uses `std::process::Command`
(blocking, not async).

## Key Types & APIs

### VersionInfo (`src/upgrade/mod.rs:7`)

```rust
pub struct VersionInfo {
    pub current: String,
    pub latest: Option<String>,
    pub needs_update: bool,
}
```

### Functions

| Function | Signature | Notes |
|----------|-----------|-------|
| `current_version()` | `fn() -> String` | Returns `CARGO_PKG_VERSION` |
| `check_for_updates()` | `async fn() -> Result<VersionInfo, AppError>` | GitHub API query |
| `upgrade()` | `async fn() -> Result<String, AppError>` | Defined but not called by CLI |

## Configuration Surface

### autoupdate (`opencode.json`)

```rust
pub enum AutoupdateConfig {
    Bool(bool),
    Notify(String),
}
```

Default: `Bool(true)`. Defined in `codegg-config` schema
(`crates/codegg-config/src/schema.rs:192`), loaded into
`Config.autoupdate` (`schema.rs:217`). **Not wired to the
upgrade module** — the config is loaded and stored but never
read by `check_for_updates()` or `upgrade()`.

## Invariants & Gotchas

- **CLI is check-only**: `codegg upgrade` never modifies the binary.
- **`upgrade()` is dead code**: defined in `src/upgrade/mod.rs:57` but
  not invoked from any CLI path.
- **`autoupdate` config is inert**: exists in schema with default `true`
  but the upgrade module does not read it. Background auto-upgrade is
  not implemented.
- **Blocking `std::process::Command` in async**: `upgrade()` uses
  blocking `curl` subprocess inside an `async fn`. This is acceptable
  because the function is not called.
- **Version comparison is exact string match**: `l != VERSION` — does
  not use semver ordering. Two different strings always trigger
  `needs_update: true`.

## Related Docs

- [config.md](config.md) — `autoupdate` field (defined but not wired)
