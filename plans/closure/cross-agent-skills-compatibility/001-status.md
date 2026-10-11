# Cross-Agent Skill Compatibility M001 — Closure Status

Status: closed

Source implementation plan: `plans/implementation/cross-agent-skills-compatibility/001-activation-parser-contract.md`

Source roadmap: `plans/subsystems/cross-agent-skills-compatibility-roadmap.md`

Repository baseline reviewed: `11e18b0797b2290f7e1d7c0f12c301466e98c03f`

Implementation commits:

- `fb63817439f9bd470c01e61f84cd49193f9bca86` — cross-agent skill compatibility
- `bfa5bcd001f297500e6cf6c36bcba984c0212f04` — isolate home discovery fixtures
- `9f55a80c5c15208a25e0705f67b2c4ceaaa331d2` — isolate conformance and user roots

## 1. Executive finding

M001 is closed. Both prompt paths now identify the real model-facing `skill`
tool and its `name` argument; no `/skill:<name>` capability is advertised.
Portable package names and directory identity are checked separately from
CodeGG-native compatibility, which retains existing permissive behavior.
Skill contents and vendor metadata remain data and do not grant permissions.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result |
|---|---|---|
| Truthful activation instructions | `src/skills/registry.rs`, `src/skills/mod.rs`, `tests/asset_snapshot.rs` | pass |
| Portable names and package identity | `src/skills/parser.rs`; portable/native parser fixtures | pass |
| Lazy body loading and captured digest | `tests/asset_snapshot.rs::pinned_skill_activation_records_the_captured_digest` | pass |
| Native compatibility retained | parser and registry suites; direct Markdown and CodeGG package fixtures | pass |
| Skill metadata cannot grant tool authority | `allowed_tools_cannot_grant_permissions`; inert metadata diagnostics | pass |

## 3. Verification executed

- `rtk cargo test -p codegg --lib skills:: --locked` — 48 passed.
- `rtk cargo test --test skills_registry --locked` — 29 passed.
- `rtk cargo test --test skills --locked` — 7 passed.
- `rtk cargo test --test asset_snapshot pinned_skill_activation_records_the_captured_digest --locked` — 1 passed.
- `rtk cargo fmt --all -- --check` — passed.
- `rtk cargo clippy --workspace --all-targets --locked -- -D warnings` — passed.
- `rtk bash scripts/verify.sh quick` — passed.

## 4. Security and compatibility review

No execution or permission boundary changed. Bodies remain activated through
the existing snapshot-bound tool path. Unknown vendor fields and `allowed-tools`
remain inert. The first parser run found three old test fixtures whose portable
package directory did not match the declared skill name; the fixtures were
corrected and the 48-test rerun passed.

## 5. Documentation and disposition

Updated the skill architecture, operator guide, activation snapshots, legacy
facade guidance, and changelog. No migration, protocol authority, or new
runtime was introduced. M002's hard dependency is discharged; its closure is
recorded separately at `002-status.md`.

## 6. Findings and limits

No blocking findings remain. Qualification ran on Linux; macOS and Windows
were not native test hosts. Vendor path references and platform-root behavior
are documented, with simulated/injected root fixtures where applicable.
