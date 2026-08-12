# Runtime Consolidation, Deletion, and Footprint M004 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/runtime-consolidation-deletion-footprint/004-prompt-provider-history-legacy-deletion.md`

Source subsystem roadmap:

- `plans/subsystems/runtime-consolidation-deletion-footprint-roadmap.md`

Repository baseline reviewed: `bd9b3b610af0fa72ce3fe5a8b8f59222659f006d`

Implementation commits or pull requests:

- `0363d8f1be2dca5b3f9fcc8124251abf76122f02` — consolidate prompt and provider history paths

## 1. Executive finding

M004 is complete. The production prompt path is compiler-only and consumes
explicit runtime assets and resolved adapter/profile identity. Process-CWD
loaders, prompt-time remote fetching, generic provider-template selection, and
post-compile prompt mutation were removed. Canonical history is no longer
rewritten by `AgentLoop`; provider serializers apply a wire-only projection for
interrupted tool calls.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| PromptCompiler is the sole production prompt compiler | Active callers in `turn_runtime`, `worker`, and `main`; legacy wrappers deleted | pass | No legacy prompt caller remains in Rust production code |
| No production prompt authority from process CWD | Deleted `find_*` helpers and CWD-based prompt assembly | pass | Explicit asset context/snapshot remains authoritative |
| No remote fetch in prompt assembly | Deleted `fetch_remote_instruction` and async compatibility loader | pass | URL-shaped config entries are skipped, not fetched or presented as bodies |
| Effective content is typed before fingerprinting | `PromptCompilerInput.runtime_blocks`, snapshot blocks, compiler fingerprint tests | pass | Post-compile custom append path deleted |
| Provider specialization is adapter/profile-owned | Deleted `select_provider_prompt` and eight bundled templates; architecture updated | pass | No generic model-prefix template branch remains |
| Canonical history is not mutated for provider grammar | `project_tool_call_history` in provider core; loop hardening deleted | pass | Projection is applied by all legacy chat serializers |
| Strict provider pairing remains valid | Provider-core projection regression test plus 93 provider tests | pass | Missing results exist only in serialized request projection |
| Root/descendant compiler and snapshot semantics remain | 40 `agent_loop_harness` tests and 4 asset-snapshot tests | pass | No snapshot/pin or child authority changes |

## 3. Production implementation evidence

`src/agent/prompt.rs` now retains the typed block compiler and its profile-aware
contracts only. The obsolete compatibility surface and prompt assets were
deleted. `ProjectInstructionResolver` and `ProjectAssetSnapshotBuilder` remain
the filesystem/asset owners, and prompt compilation remains pure.

`codegg-providers::project_tool_call_history` creates a bounded projected copy
of messages for provider serialization. It preserves orphan messages and adds
an interrupted-result placeholder only to the outgoing wire representation;
the session/turn vector is unchanged. All eight providers that serialize the
chat message model use the projection.

## 4. Verification executed

### Commands run

```bash
cargo fmt --all
cargo check -p codegg-providers
cargo check -p codegg
cargo test -p codegg-providers --lib -- --nocapture
cargo test -p codegg --lib agent::prompt -- --nocapture
cargo test -p codegg --lib agent::asset_snapshot -- --nocapture
cargo test --test agent_loop_harness -- --test-threads=1
scripts/verify.sh quick
git diff --check
```

### Results

All commands passed. Provider tests: 93; prompt tests: 23; asset snapshot
tests: 4; harness tests: 40. Quick verification passed, including generated
asset, core-boundary, sandbox, execution-ownership, formatting, and locked
workspace all-target checks. No hosted or external-server evidence was
required by this milestone.

## 5. Invariant review

Explicit execution/workspace context, immutable runtime-asset snapshots and
pins, deterministic compiler fingerprints, adapter/profile identity, and
root/descendant compiler contracts remain intact. Provider compatibility is
kept at serialization and is not stored as canonical durable history.

## 6. Failure and recovery review

Interrupted tool calls no longer create durable fabricated results. A provider
request receives a deterministic placeholder only when its wire grammar needs
the missing pairing. Existing cancellation, retry, compaction, and provider
streaming control flow remains unchanged; no durable storage or restart path
was modified.

## 7. Migration and compatibility review

No schema, protocol, or user migration is required. The removed Rust helpers
had no repository callers, examples, or supported embedding references beyond
historical planning/architecture text. The provider wire shape remains valid;
the only behavioral change is avoiding canonical-history mutation for a
provider-only requirement.

## 8. Security review

Removing prompt-time fetching eliminates an unbounded compatibility network
path. Instruction files continue through bounded explicit-context resolution;
no CWD fallback, new downloader, privilege change, or authority broadening was
introduced. Provider projection does not expose secrets or filesystem paths.

## 9. Documentation and operations

`architecture/agent.md` now documents compiler-only prompt construction,
snapshot-owned instruction resolution, and adapter-owned specialization. No
new guard, CI lane, or operational command was added.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| low | External consumers outside this repository cannot be enumerated | Public Rust API compatibility cannot be proven beyond repository evidence | Existing plan stop condition was satisfied by repository/package audit; any future supported embedding must use the typed compiler path |

No critical, high, or medium finding remains in M004 scope.

## 11. Roadmap disposition

M004 is closed. M005 remains dependency-ready. M006 is not unblocked: its
hard dependency set still includes M003 (corrective physical extraction) and
M005. M007 remains blocked on M006. M003's blocker is not resolved by this
closure, so no downstream plan changes to `ready` status are warranted.

## 12. Registry updates

- M004 implementation plan status changed to `implemented`.
- M004 moved from dependency-ready work to recently closed work.
- M004 was marked `closed` in the subsystem roadmap and registry.
- M003 remains `corrective pass required`; M005 remains `ready`.
- M006 and M007 remain blocked; the dependency audit found no newly unblocked
  registered plan.
