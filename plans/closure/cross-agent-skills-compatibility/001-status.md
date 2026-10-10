# Cross-Agent Skill Compatibility M001 — Closure Status

Status: closed

Source plan: `plans/implementation/cross-agent-skills-compatibility/001-activation-parser-contract.md`

Source roadmap: `plans/subsystems/cross-agent-skills-compatibility-roadmap.md`

Repository baseline reviewed: `578b62bd9580382e00fb19e096fc1794e1f5618b`.

Implementation commits: `d0164e95` and `d7a146e1`.

## 1. Finding

M001 is closed. Both skill prompt paths now describe the actual `skill` model tool and its JSON `name` argument. The portable parser has a strict name/directory/description boundary, while CodeGG-owned native packages retain their prior permissive description behavior. No skill content is eagerly added to the prompt and no metadata grants execution permissions.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result |
|---|---|---|
| Truthful activation prompt | `skills::` tests and `tests/skills.rs`; prompt names `skill` + `{"name": ...}` and rejects `/skill:` guidance | pass |
| Portable name and package identity validation | `skills::` parser fixtures and `tests/skills_registry.rs` | pass |
| Required non-empty portable description without native regression | `portable_description_must_not_be_empty_but_codegg_remains_compatible` | pass |
| Actual activation remains snapshot/tool based | `tests/asset_snapshot.rs`, `tests/plugin_contributions.rs` | pass |
| Tool metadata cannot add authority | `allowed_tools_cannot_grant_permissions`; execution ownership guard | pass |

## 3. Implementation evidence

`AssetRegistry::build_system_prompt` and `SkillIndex::build_system_prompt` agree with `src/tool/skill.rs`. `validate_portable_document` validates portable proposals and foreign packages. CodeGG source kinds use the compatibility allowance for previously accepted blank descriptions; direct Markdown and native metadata behavior remain intact. Unknown vendor extensions are retained as inert metadata with bounded warnings.

## 4. Verification executed

- `rtk cargo test -p codegg --lib skills::` — 49 passed.
- `rtk cargo test --test skills_registry` — 24 passed.
- `rtk cargo test --test plugin_contributions` — 8 passed.
- `rtk cargo test --test asset_snapshot` — 8 passed.
- `rtk scripts/verify.sh quick` — passed.
- `rtk cargo clippy --workspace --all-targets --locked -- -D warnings` — passed.

An earlier registry run exposed two issues (symlink entries were omitted and a compat assertion depended on ambient global skills); both were corrected, and the final registry suite passed. A fixture name collision found during the first skills unit run was also corrected before the final 49-test pass.

## 5. Adversarial and compatibility fixtures

Fixtures cover uppercase/underscore/path-like names, directory mismatch, Claude directory-name fallback, unknown metadata, `allowed-tools`, empty portable description, legacy CodeGG packages, symlink escapes, oversized/malformed frontmatter, and native direct Markdown. Plugin namespacing and snapshot activation regressions pass.

## 6. Security and authorization

Discovery and activation remain read-only. Foreign metadata and resource scripts are data only. `allowed-tools` is reported as inert metadata. No tool, permission, scheduler, protocol, or filesystem-write authority changed; execution-ownership and core/client boundary guards passed.

## 7. Runtime and concurrency

Bodies remain lazy until the existing skill tool selects a skill. Existing immutable snapshot pinning remains the authority. No second registry or publication path was introduced.

## 8. Platform findings

Linux focused tests passed. Windows/macOS execution was not available in this local qualification; path handling continues to use standard `Path` APIs. No hosted CI result is claimed.

## 9. Documentation

Updated `architecture/skills.md`, `docs/agents-skills.md`, `.opencode/skills/skills/SKILL.md`, the `AGENTS.md` skills index, and the implementation handoff. User-facing guidance no longer advertises `/skill:<name>` as an activation command.

## 10. Deviations and remaining findings

The plan template mentions all-features Clippy. Repository instructions prohibit `--all-features` because it enables real-server tests; the supported locked workspace/all-targets Clippy command passed instead. No unresolved M001 correctness findings remain.

## 11. Dependency and registry disposition

M001 closed first in the plan sequence and unblocked M002. M002–M005 have since closed in order; no downstream plan remains unblocked by this line. Registry and roadmap now record the whole line as closed.

## 12. Final disposition

M001 acceptance criteria are met. Closure accepted.
