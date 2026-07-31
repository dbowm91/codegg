# Agent Runtime, Model Adaptation, and ACP Milestone 001 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/agent-runtime-model-adaptation-acp/001-prompt-compilation-and-agent-registry-correctness.md`

Source subsystem roadmap:

- `plans/subsystems/agent-runtime-model-adaptation-acp-roadmap.md#milestone-001--prompt-compilation-and-agent-registry-correctness`

Repository baseline reviewed: `f1af18ab` (plan registration head)

Implementation commit:

- `3cb6c0ec30a538483db121d89fa1028f5f06ad39` — converge prompt compilation and agent registry

## 1. Executive finding

Milestone 001 is closed. Production root and descendant requests now use the
same profile-aware `PromptCompiler`, with deterministic capability ordering,
versioned fingerprints, explicit execution/snapshot inputs on root turns, and
truthful omission of unresolved remote URLs. Agent file resolution preserves
field-level overlays, supports bounded native TOML inheritance, retains
fallback/runtime/safety fields, and fails closed for missing/self bases.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result |
|---|---|---|
| One compiler for root and child turns | `PromptCompiler`; `DefaultTurnRuntime`; `execute_agent_task` | pass |
| Profile-aware policy is production-reachable | Root compiler invokes `assemble_system_prompt_with_profile` | pass |
| Deterministic blocks/fingerprint | `PromptBlock`, compiler version, SHA-256 fingerprint, deterministic compiler test | pass |
| Immutable asset use at root boundary | `ProjectAssetSnapshot` selection and compiler snapshot input in `turn_runtime.rs` | pass |
| Overlay and replacement semantics | `AgentSpec::merge_overlay`, registry tests | pass |
| Safe built-in inheritance | TOML `extends`, missing/self-base rejection, project inheritance test | pass |
| Complete model/runtime field preservation | `fallback_model` and `runtime_kind` merge paths, wrapped TOML parsing, registry test | pass |
| Remote instruction behavior is bounded and truthful | compiler skips unresolved URL entries; no network fetch in compilation | pass |
| Generated built-ins remain authoritative | `check_builtin_agents.py`, `generate_builtin_agents.py --check` | pass |

## 3. Production implementation evidence

`PromptCompiler` sorts tools, skills, and agent metadata before invoking the
profile-aware composer and emits a stable flattened provider prompt plus a
structured block identity and fingerprint. Root turns select the resolved
snapshot agent where available and pass the explicit execution context and
snapshot. Descendants use the identical compiler contract after their safety
envelope and filtered tool registry are established.

`AgentSpec` now carries explicit inheritance metadata. Global and project
layers resolve the base before applying the overlay, preserving security-review
runtime and permission fields. Wrapped TOML now retains fallback model and
runtime kind instead of silently discarding them.

## 4. Verification executed

Passing local evidence:

```text
cargo fmt --all
cargo check --workspace
cargo test -p codegg --lib agent::prompt       # 21 passed
cargo test -p codegg --lib agent::registry     # 24 passed
cargo test -p codegg --test subagent           # 21 passed
cargo test -p codegg --lib agent::asset_snapshot # 4 passed
python3 scripts/check_daemon_cwd_usage.py
python3 scripts/check_project_agent_pwd_inference.py
python3 scripts/check_builtin_agents.py
python3 scripts/generate_builtin_agents.py --check
```

The required broad command `cargo test --workspace --lib` compiled and began
running successfully but aborted in the pre-existing macOS test
`core::transport::daemon_socket::daemon_socket_integration_tests::socket_f0_successful_production_write_is_observed`
with a stack overflow. This is recorded as unrelated operational evidence,
not as a passing broad-suite result.

## 5. Invariant review

No new production prompt/agent path reads cwd or `PWD`; the existing
compatibility loaders remain deprecated and guarded. Capability metadata is
descriptive only and cannot grant tools or permissions. Built-ins remain
generated assets, and custom agent permissions continue through the existing
safety envelope. Prompt fingerprints contain content digests, not absolute
paths or complete projection payloads.

## 6. Failure and recovery review

Compilation is synchronous and bounded after snapshot capture. Missing or
self-referential inheritance fails before publication. URL entries are not
pretended to be loaded during compilation. Concurrent turns do not share a
mutable prompt cache.

## 7. Migration and compatibility review

Existing flat and wrapped TOML plus Markdown prompt-first files remain
readable. `replace` retains full-replacement semantics. The additive
`extends` field is schema-version compatible and unknown keys continue to be
diagnosed by the existing loader.

## 8. Security review

The compiler does not alter tool registration or permission checks. Descendant
tools are filtered before capability names are compiled. Inherited agents
cannot bypass the runtime safety envelope, and unresolved remote content is
never inserted into a prompt.

## 9. Documentation and operations

`architecture/agent.md` documents compiler ownership, precedence,
inheritance, fingerprints, and remote-instruction behavior. The plan and
roadmap now point at this closure record.

## 10. Unresolved findings

- Low: broad workspace-library execution remains operationally blocked by the
  unrelated macOS socket fixture stack overflow described above. Focused
  milestone evidence and compilation are green.
- No unresolved high- or medium-severity milestone finding.

## 11. Roadmap disposition

M001 is closed. M002 is the next dependency-ready handoff. M003 through M011
remain blocked by their separately listed M002/M003/M004–M010 dependencies.

## 12. Registry updates

The registry moved M001 from dependency-ready work to recently closed work,
promoted M002 to `ready`, and left all later milestones blocked. No other
registered plan's complete dependency set was satisfied by this closure.
