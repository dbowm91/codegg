# Cross-Agent Skill Compatibility M002 — Closure Status

Status: closed

Source implementation plan: `plans/implementation/cross-agent-skills-compatibility/002-source-roots-and-deduplication.md`

Source roadmap: `plans/subsystems/cross-agent-skills-compatibility-roadmap.md`

Repository baseline reviewed: `11e18b0797b2290f7e1d7c0f12c301466e98c03f`

Implementation commits: `fb63817439f9bd470c01e61f84cd49193f9bca86`,
`bfa5bcd001f297500e6cf6c36bcba984c0212f04`, and
`9f55a80c5c15208a25e0705f67b2c4ceaaa331d2`.

## 1. Executive finding

M002 is closed. The registry recognizes the qualified project and home roots
listed in `architecture/skills.md` and `docs/agents-skills.md`, preserves
existing serialized source values, and deterministically deduplicates canonical
roots and physical skill files. Home discovery is injectable so tests and
embedders do not depend on the process user's private skill installation.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result |
|---|---|---|
| Project and home locations | `resolve_source_roots_with_home`; project-root and injected-home fixtures | pass |
| Stable classifications and backwards compatibility | `source_kind_precedence_order`; historical serde-name fixture | pass |
| Canonical alias root and physical-file deduplication | `canonical_source_aliases_are_scanned_once_with_stable_precedence`; physical-target set | pass |
| Explicit global/configured roots and bounded candidates | 16-root cap; home/config/project root fixtures | pass |
| Vendor path evidence and exclusions | official references linked from `docs/agents-skills.md`; no mode-specific or descendant scan | pass |

## 3. Verification executed

- `rtk cargo test -p codegg --lib skills:: --locked` — 48 passed.
- `rtk cargo test --test skills_registry --locked` — 29 passed.
- `rtk cargo clippy --workspace --all-targets --locked -- -D warnings` — passed.
- `rtk bash scripts/verify.sh quick` — passed.

The first integration run picked up real user-home skills and exposed a
non-isolated test setup; the suite was changed to inject `None` for the home
root, and all 29 tests passed on rerun. A symlink fixture was also corrected to
create its parent directory before creating the link.

## 4. Precedence and security review

Project roots precede global roots, and configured roots remain last. Within
project roots, workspace depth precedes stable vendor rank. Canonical aliases
retain display aliases without manufacturing shadow candidates. Foreign roots
are read-only, canonical escapes are rejected, and no arbitrary home or
descendant recursion was introduced.

## 5. Documentation and findings

The source matrix, global-root semantics, and qualified vendor references are
documented in the architecture and operator guide. No blocking findings remain.
The host was Linux; Windows and macOS filesystem behavior was not exercised on
native runners. No hosted workflow run was available for this pushed branch.

M003's hard dependency is discharged; its closure is recorded separately at
`003-status.md`.
