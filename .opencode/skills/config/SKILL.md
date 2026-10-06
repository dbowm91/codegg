---
name: config
description: Configuration schema, layered discovery order, JSON/JSONC parsing, and YAML frontmatter compatibility in codegg
version: 1.0.0
tags:
  - config
  - schema
  - paths
  - jsonc
---

# Configuration Module Guide

Operational guide for changing `crates/codegg-config/`. The full contract
lives in `architecture/config.md`; this skill covers discovery precedence,
format handling, and the backward-compatibility rules that keep existing user
config files working.

## Layout

| Path | Purpose |
|------|---------|
| `crates/codegg-config/src/schema.rs` | The `Config` tree — the single source of truth for every accepted key |
| `crates/codegg-config/src/paths.rs` | Layered config discovery and platform paths |
| `crates/codegg-config/src/document.rs` | `DocumentFormat`, `DocumentErrorClass`, `parse_yaml` — the markdown-frontmatter codec |
| `crates/codegg-config/src/encryption.rs` | Master-key resolution for the credential store |
| `crates/codegg-config/src/watcher.rs` | `ConfigWatcher` / config reload plumbing (`WatcherConfig` itself lives in `schema.rs`) |
| `crates/codegg-config/src/error.rs` | Aggregated config error type (`ConfigError` + `AppError`) |

## Discovery Order

`resolve_config_paths()` (`crates/codegg-config/src/paths.rs:12`) returns
candidate files in this order, later entries layered over earlier ones:

1. `CODEGG_TUI_CONFIG` — explicit file, only if it exists (`paths.rs:15-20`).
2. **System** — `/Library/Application Support/codegg/codegg.json` on macOS,
   `/etc/codegg/codegg.json` on unix, `%ProgramData%\codegg\codegg.json` on
   Windows (`paths.rs:75-89`).
3. **Global** — `dirs::config_dir()/codegg/`, probing `codegg.jsonc`, then
   `codegg.json`, then `config.json`, falling back to `codegg.jsonc`
   (`paths.rs:64-73`).
4. **Project** — first match walking *upward* from the cwd, checking
   `.codegg/codegg.{jsonc,json}` then `codegg/codegg.{jsonc,json}` at each
   level, stopping at the filesystem root (`paths.rs:46-62`).

Note the project search calls `std::env::current_dir()`. Daemon code that is
workspace-bound must NOT do that — thread an `ExecutionContext` instead
(`scripts/check_daemon_cwd_usage.py`).

## Invariants

- **`schema.rs` is the contract.** A key that is not in `schema.rs` does not
  exist, regardless of what a doc or example file claims. When you add a
  field, add it to `codegg.example.jsonc` and `architecture/config.md` too.
- **Every config struct is `#[serde(default)]`.** Existing user files must keep
  loading when you add a field. Do not make a new field required.
- **`CONFIG_VERSION` is `"1"`** (`schema.rs:5`) and `Config::migrate()`
  (`schema.rs:2739`) upgrades `"0"` → `"1"`. Do not bump it without a migration
  branch and a test.
- **A new `Config` field is invisible until `merge_configs` handles it.**
  `merge_configs` (`paths.rs:164`) is an explicit whitelist, not a derived
  merge: `merge_option!` for scalar/`Option` fields, then hand-written arms
  for maps and nested structs. Thirteen fields are currently **dropped on
  every multi-layer load** — `approval_reviewer`, `command_intent`, `daemon`,
  `deterministic_tools`, `human_shell`, `preflight`, `provider_connections`,
  `research`, `scheduler`, `security`, `shell`, `tool_backends`,
  `tool_deferral` (`theme` is the one exception: whole-value replace).
  Verified empirically: a parsed `[security]` block survives `load_config`
  but is `None` after `merge_configs`. Adding a field means adding a merge
  arm in the same change, plus a `tests/` merge assertion.
- **Inert-but-retained fields stay.** `[autoupdate]` (`schema.rs:236`) is
  accepted and preserved but **nothing reads it**; `codegg upgrade` only runs
  on explicit invocation (see `architecture/upgrade.md`). It is deliberately
  not removed because existing user config files contain it. Document a
  retained field rather than deleting it.
- **`encrypt_provider_keys` / `decrypt_provider_keys` are no-ops**
  (`encryption.rs:544`/`:549`). The config crate owns master-key *resolution*
  only; credential encryption/decryption lives in the providers crate and
  the MCP auth store. `Config::load()` explicitly does not decrypt.
- **YAML is read-only compatibility.** `parse_yaml` (`document.rs:92`) exists
  only to read markdown frontmatter in agents, commands, and skills, via
  `serde_norway` 0.9.42. New config and generated assets use TOML or
  JSON/JSONC. Existing YAML is never rewritten automatically.
- **JSONC is the canonical user format.** JSON5 plus comments.

## Testing

```bash
cargo test -p codegg-config
```

Storage-adjacent config tests belong with storage; see
`.opencode/skills/session-storage/SKILL.md`.

## See Also

- `architecture/config.md` — authoritative configuration contract
- `docs/dependency-maintenance.md` — `serde_norway` pinning and feature policy
- `docs/security-semantics.md` — the `security` config block in full
- `.opencode/skills/provider-auth/SKILL.md` — `provider` / credential config
- `.opencode/skills/session-storage/SKILL.md` — `STORAGE_LAYOUT_VERSION`

## Source verification

Verified 2026-10-06 against `crates/codegg-config/src/{paths,schema,document,encryption,watcher,error}.rs`
and `crates/codegg-config/Cargo.toml`. Pinned the discovery order and its
exact lines, the system-config platform paths, `CONFIG_VERSION`, the inert
`autoupdate` field, and the `serde_norway` 0.9.42 pin. Claims without a
traceable source were removed rather than guessed.

Second pass: corrected `autoupdate` `schema.rs:231` → `:236`; corrected the
watcher row (the file owns `ConfigWatcher`, not `WatcherConfig`); added the
`merge_configs` whitelist gap — 13 fields silently dropped on multi-layer
load, verified by building against the crate and comparing
`load_config(path)` with `merge_configs(&[config])` — and the
`encrypt_provider_keys`/`decrypt_provider_keys` no-op contract
(`encryption.rs:544`/`:549`).
