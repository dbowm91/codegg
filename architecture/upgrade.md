# Upgrade Module

Self-upgrade version check via GitHub releases (check-only).

## Purpose

Checks the GitHub releases API for a newer version and prints manual
fresh-install instructions. Automatic in-place binary replacement is
retired: CodeGG never downloads and executes a network-fetched shell
script, never shells out to external `curl`, acquires no candidate
bytes, and attempts no executable replacement. Verified binary
replacement awaits the blocked external generic updater interface
(M005); it is intentionally not reimplemented locally.

## Where It Lives

- `src/upgrade/mod.rs` — version check and fail-closed disposition logic
- `src/main.rs:1015` — `cmd_upgrade()` CLI handler

## How It Works

### CLI Command (`codegg upgrade`)

`cmd_upgrade()` in `src/main.rs:1015` calls `upgrade::check_for_updates()`,
compares with `CARGO_PKG_VERSION`, and prints manual install instructions:

```
curl -fsSL https://raw.githubusercontent.com/dbowm91/codegg/main/install.sh | sh
```

It does **not** call `upgrade()`.

### Version Check

`check_for_updates()` sends a GET to
`https://api.github.com/repos/dbowm91/codegg/releases/latest`
with a 10-second timeout. Parses `tag_name`, strips leading `v`,
and compares with the compiled `VERSION`.

### Install Function (retired, fail-closed)

`upgrade()` checks for updates via `check_for_updates()`, then delegates
to the pure `describe_upgrade()` disposition:

- already-current returns `Ok("Already on latest version ...")` without
  touching the executable;
- missing latest tag or invalid semver returns `Err` (fail-closed);
- a valid newer tag returns `Err` with manual fresh-install guidance
  (`CODEGG_VERSION=v{latest}` plus the `install.sh` URL).

It never spawns `curl`, never fetches or executes a shell script, and
never replaces the running binary. The script URL and pin env are built
by the pure `installer_invocation()` constructor so the fresh-install
pin contract remains unit-tested without spawning a subprocess.

## Key Types & APIs

### VersionInfo (`src/upgrade/mod.rs:8`)

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
| `check_for_updates()` | `async fn() -> Result<VersionInfo, AppError>` | GitHub API query via Eggfetch |
| `upgrade()` | `async fn() -> Result<String, AppError>` | Check + fail-closed `describe_upgrade()`; never replaces binary |
| `describe_upgrade()` | `fn(&VersionInfo) -> Result<String, AppError>` | Pure fail-closed disposition; deterministic without network |

## Configuration Surface

### autoupdate (`opencode.json`)

```rust
pub enum AutoupdateConfig {
    Bool(bool),
    Notify(String),
}
```

Default: `Bool(true)`. Defined in `codegg-config` schema
(`crates/codegg-config/src/schema.rs:204`), loaded into
`Config.autoupdate` (`schema.rs:229`). **Not wired to the
upgrade module** — the config is loaded and stored but never
read by `check_for_updates()` or `upgrade()`.

## Invariants & Gotchas

- **CLI is check-only**: `codegg upgrade` never modifies the binary.
- **`upgrade()` is fail-closed**: it returns manual fresh-install guidance
  for any newer version instead of downloading, verifying, or replacing
  bytes. No `std::process::Command`, no external `curl`, no shell
  execution anywhere in `src/upgrade/`.
- **`autoupdate` config is inert**: exists in schema with default `true`
  but the upgrade module does not read it. Background auto-upgrade is
  not implemented.
- **No candidate acquisition**: because no binary bytes are fetched,
  checksum / program-identity / version-identity / unwritable-destination
  / interrupted-download / replacement-failure / unsupported-target cases
  all reduce to "existing executable left intact" by construction.
  Verified binary replacement awaits the blocked M005 external updater
  interface and is not duplicated locally.
- **Version comparison is exact string match**: `l != VERSION` — does
  not use semver ordering. Two different strings always trigger
  `needs_update: true`.
- **Version-pin env mismatch (fixed)**: `upgrade()` once exported the
  target as `INSTALL_VERSION`, which `install.sh` ignores. It now exports
  `CODEGG_VERSION` via the `installer_invocation()` constructor, matching
  the installer's supported surface. `tests/upgrade.rs` pins the env name;
  renaming it requires updating `install.sh` first.

## Related Docs

- [config.md](config.md) — `autoupdate` field (defined but not wired)
