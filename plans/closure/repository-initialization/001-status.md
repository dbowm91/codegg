# Repository Initialization Milestone 001 — Closure Status

Status: closed
Source implementation plan: `plans/implementation/repository-initialization/001-bounded-evidence-and-draft.md`
Source subsystem roadmap: `plans/subsystems/repository-initialization-roadmap.md#7-milestones`
Repository baseline reviewed: `3809aa17c6397a69ffe27a2aa8d55e09c276fa88`
Implementation commits: `b4e28c5 — add bounded, read-only repository evidence and AGENTS.md draft generation`

## 1. Executive finding

M001 is complete as provider-independent infrastructure. The explicit-root
analyzer returns a deterministic candidate, source-attributed facts,
diagnostics, and an exact target digest or absent marker. It does not write,
execute commands, call a provider, access global instructions, or copy source
file bodies into the candidate. No `/init` command is claimed by this closure.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence |
|---|---|
| Explicit selected root and fixed root `AGENTS.md` target | `ProjectBootstrapContext`, canonical root in `InstructionDraft`, and selected nested-root test |
| Bounded allowlisted scan | 64-entry/file cap, depth 4, 64 KiB per file, 512 KiB total, 2 second cooperative elapsed budget; budget fixture |
| Observed/unknown classification and source paths | `BootstrapEvidence`; empty and multi-manifest fixtures |
| No command execution or private/global content | Static analyzer has no process/provider/global-instruction calls; hostile README and `.env` decoys are absent from candidate |
| Conservative existing-file update | Only the CodeGG-delimited section is replaced; human text and CRLF are preserved; idempotent rerun test |
| No-op and stale-write material | No-op operation on repeat and exact `sha256:` digest; absent target represented by `None` |
| Malformed, binary, oversized, and symlink input | Ambiguous marker, binary file, oversized target, and Unix symlink tests |
| Ignore and nested-workspace behavior | Root `.gitignore` and workflow fixture; selected nested root ignores ancestor manifests and `.git` indirection |

## 3. Production implementation evidence

`src/agent/bootstrap.rs` defines the inert draft/evidence/diagnostic values,
bounded candidate inventory, deterministic manifest facts, managed-section
merge, and target digest. `analyze_project_context` carries authoritative
project and workspace identity into the proposal. The implementation is
read-only and uses an allowlist; README and instruction file bodies are never
copied into evidence or generated Markdown.

## 4. Verification executed

- `rtk cargo test --lib agent::bootstrap --locked` — **9 passed**, 0 failed.
- `rtk cargo fmt --all -- --check` — passed.
- `rtk git diff --check` — passed before implementation commit.
- `rtk bash scripts/verify.sh quick` — passed, including workspace all-target check and repository guards.
- `rtk cargo clippy --workspace --all-targets --locked -- -D warnings` — passed after a minor nested-condition cleanup.

The focused test process compiled against the final analyzer source and ran
locally. The quick pass preceded the lint-only nested-condition cleanup; the
final source then passed workspace Clippy and is being rerun through the
focused analyzer test. No hosted or native non-Linux qualification is claimed
here.

## 5. Invariant review

The analyzer receives an explicit root and canonicalizes it; it does not
consult process CWD or walk ancestor/sibling projects. Candidate evidence
contains only generic facts and relative source paths. The target remains
exactly root `AGENTS.md`. No state becomes active runtime instructions until a
separate authorized publication path exists.

## 6. Failure and recovery review

Unreadable, oversized, binary, symlinked, or out-of-root evidence is skipped
with bounded diagnostics or an explicit refusal for an unsafe root target.
Malformed managed markers leave existing text unchanged and report an
ambiguity. The operation is ephemeral; cancellation or process restart loses
only the candidate because no disk mutation occurs.

## 7. Migration and compatibility review

No protocol, storage, schema, permission, or existing instruction resolver
behavior changed. Existing text and line endings outside the generated
section are preserved. The new module is additive.

## 8. Security review

Repository files are treated as untrusted data. Commands and README prose are
not executed or reproduced. Global/private instructions, `.env`, credentials,
and unlisted paths are not inspected. Symlink entries are skipped, resolved
candidate paths are checked against the canonical root, and the output carries
no raw file excerpts. The two-second deadline is cooperative between bounded
filesystem operations; individual synchronous filesystem calls cannot be
preempted.

## 9. Documentation and operations

Updated `architecture/agent.md`, added `docs/repository-init.md`, and indexed
the guide from `README.md`. The documentation describes the API as a draft
only and does not claim `/init` is implemented.

## 10. Unresolved findings

- **Low:** synchronous filesystem syscalls cannot be interrupted at the
  cooperative elapsed-time boundary. File count, depth, and byte caps remain
  enforced; a slow filesystem may exceed the nominal two-second target.

No medium or high findings remain for this read-only milestone.

## 11. Roadmap disposition

M001 is closed. The new `ProjectBootstrapContext` and `InstructionDraft`
provide the hard interface dependency for M002. ADR-0014 records the scoped
daemon publication contract because existing document save cannot create an
absent root target.

## 12. Registry updates

The registry now records M001 closed and M002 **ready**. The blocked-work and
roadmap dependency audit found no other registered plan unblocked by M001.
