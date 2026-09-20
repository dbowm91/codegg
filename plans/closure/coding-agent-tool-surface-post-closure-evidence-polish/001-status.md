# Coding-Agent Tool Surface Post-Closure Evidence Polish M001 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/coding-agent-tool-surface-post-closure-evidence-polish/001-lsp-preview-apply-concurrency-restart-evidence.md`

Source subsystem roadmap:

- `plans/subsystems/coding-agent-tool-surface-post-closure-evidence-polish-addendum.md#m001--lsp-preview-apply-concurrency-and-restart-evidence-polish`

Repository baseline reviewed: `444ef31b1f0b7ec4ce701f9bce2830281efa6046`

Implementation commits:

- `730b582b8763bec9b7bbec0a11650a8a8fdf4073` — deterministic same-preview contention and fresh-registry expiry fixtures.
- `5e7ff1c1533fdca7f97722374efd216ea76818fa` — milestone activation and registry handoff state.

Final implementation SHA: `730b582b8763bec9b7bbec0a11650a8a8fdf4073`

## 1. Executive finding

M001 is closed. The two previously composition-only M005 guarantees now have
dedicated adapter-level executable evidence, with no release-path production
behavior change. The contention fixture coordinates both calls after they have
exported the same host-owned preview, and the canonical repository lock plus
original-hash revalidation produce exactly one checked mutation and one durable
checkpoint. The lifetime fixture constructs registries through the production
session factory and proves that a fresh registry cannot resolve the old opaque
ID.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Same preview has exactly one successful adapter call | `same_preview_concurrent_apply_commits_once` | pass | Two direct calls were released from a shared test-only barrier after both exported the candidate; one succeeded and one returned stale/original-hash rejection. |
| Exactly one durable checkpoint is recorded | Same test, `edit_checkpoint` query | pass | Pre-count was zero; post-count was one; captured checkpoint ID was `b0460d40-4f6e-4bd7-9222-ad1566104f80`. |
| Losing caller has no committed file side effect | Same test | pass | Final file content was exactly `new\n`; only the winning checked write produced the checkpoint. |
| Shared preview is marked applied only after success | Same test | pass | Registry entry was `applied = true` after the single success. |
| Direct-only, non-idempotent, zero-retry contract remains intact | Same test plus existing adapter contract assertions | pass | `DirectOnly`, `NonIdempotent`, and `max_retries = 0` remained asserted. |
| Fresh production-style registry cannot resolve prior preview | `fresh_tool_registry_expires_prior_preview_id` | pass | Registry lifetimes were built by `build_session_tool_registry`; handle allocations compared distinct and B had no old entry. |
| Expired ID has zero side effects | Same lifetime test | pass | Canonical error was `LSP preview was not found or has expired`; file stayed `old\n`; checkpoint count stayed `0`. |
| Existing negative/compatibility behavior remains green | Focused M005/M006 regression set and quick gate | pass | Schema, stale/already-applied, supported kinds, registry isolation, checkpoint, contract, execution, and guard suites passed. |

## 3. Production implementation evidence

No production runtime, storage, protocol, authorization, or scheduler behavior
was changed. The only synchronization seam is a `#[cfg(test)]` barrier field and
builder on `LspPreviewApplyTool`; release builds do not contain it. The tests
exercise the existing `LspPreviewApplyTool`, `WorkspaceLockTable`,
`apply_preview()` mutation owner, SQLite checkpoint store, and
`build_session_tool_registry` factory. No preview persistence, global registry,
claim protocol, or second mutation owner was introduced.

## 4. Verification executed

### Commands run

| Command | Result |
|---|---|
| `rtk git pull --rebase` | pass; rebased cleanly, 3 files changed (`+560/-0`) |
| `rtk cargo test -p codegg --lib tool::lsp_preview_apply -- --test-threads=1 --nocapture` | pass; 5 tests |
| `rtk proxy cargo test -p codegg --lib same_preview_concurrent_apply_commits_once -- --test-threads=1 --nocapture` | pass; 1 test, one success, stale loser, pre-count 0, post-count 1 |
| `rtk proxy cargo test -p codegg --lib fresh_tool_registry_expires_prior_preview_id -- --test-threads=1 --nocapture` | pass; 1 test, distinct handles, expired-ID error, zero checkpoints |
| `rtk cargo test -p codegg --lib tool::lsp -- --test-threads=1` | pass; 183 tests |
| `rtk cargo test -p codegg --lib lsp::mutation -- --test-threads=1` | pass; 5 tests |
| `rtk cargo test --test tool_registry -- --test-threads=1` | pass; 14 tests |
| `rtk cargo test --test edit_checkpoint_integration -- --test-threads=1` | pass; 22 tests |
| `rtk cargo test --test tool_contract_guards -- --test-threads=1` | pass; 11 tests |
| `rtk cargo test --test tool_execution -- --test-threads=1` | pass; 55 tests |
| `rtk python3 scripts/check_execution_ownership.py` | pass |
| `rtk python3 scripts/check_scheduler_bypass.py` | pass |
| `rtk cargo fmt --all -- --check` | pass |
| `rtk cargo clippy --workspace --all-targets --all-features -- -D warnings` | pass; no issues |
| `rtk scripts/verify.sh quick` | pass; all quick checks and workspace check |
| `rtk git diff --check` | pass |

The focused concurrency output was:

```text
success=1
loser=tool execution failed: LSP preview is stale: main.rs changed since preview
pre_checkpoints=0
post_checkpoints=1
checkpoint_id=b0460d40-4f6e-4bd7-9222-ad1566104f80
```

The fresh-registry output was:

```text
handles_distinct=true
old_id_error=tool execution failed: LSP preview was not found or has expired
checkpoints=0
```

All local evidence above was executed after the clean pull/rebase. No hosted CI
claim is made by this record.

## 5. Invariant review

- At most one successful checked mutation occurred for the same preview.
- Exactly one checkpoint was persisted for the race.
- The final file contained one intended application and no second overwrite.
- The shared preview was marked applied after the successful mutation.
- The adapter contract remained direct-only, non-idempotent, and non-retryable.
- Preview state remained owned by the turn-local registry handle.
- A fresh production-style registry did not reconstruct the old preview from
  durable session, workspace, checkpoint, or LSP state.
- `apply_preview()` remained the sole digest/hash/write/rollback/checkpoint
  owner.
- Tests supplied only the opaque preview ID to the adapter.
- No read-only or Tool Program boundary changed.

## 6. Failure and recovery review

The race joined both direct calls cleanly and exercised the canonical workspace
lock and stale-hash rejection path. The losing call did not retry through the
broker and produced no additional checkpoint. Registry teardown invalidated
the old ID before the fresh adapter attempted mutation. Existing mutation tests
continue to own rollback and checkpoint-failure behavior; this milestone adds
no new cancellation, restart persistence, or fault-injection semantics.

## 7. Migration and compatibility review

No schema, wire, model-input, configuration, or migration changes were made.
The public schema remains preview-ID-only, and historical M005/M006 closure
records remain unchanged and immutable.

## 8. Security review

The fixtures use the existing host-bound session/workspace rows, canonical
workspace root, path containment validation, and daemon-owned lock/service
bindings. They introduce no model-supplied patch, digest, path, session, or
workspace authority fields, and no new secret, network, or privilege path.

## 9. Documentation and operations

Updated planning control points are this closure record, the addendum roadmap,
the implementation plan status, and `plans/registry.md`. No architecture
rewrite was necessary. The exact focused test names and runtime evidence are
recorded above.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| — | None | No known medium/high correctness or security finding remains. | None. |

## 11. Roadmap disposition

Milestone closed. The addendum roadmap is closed because its only milestone has
dedicated deterministic contention and lifetime evidence, and the bounded
M005/M006 regression and repository gates are green. No corrective plan is
required.

## 12. Registry updates

- Removed M001 from the active-roadmap and dependency-ready sections.
- Added M001 to recently closed work with implementation commit `730b582b`.
- Marked the implementation plan `implemented` and the addendum roadmap
  `closed`.
- Audited the registry Blocked work section and searched registered dependency
  references. No future registered plan lists this M001 as a hard or interface
  dependency, so no plan could be promoted to `ready`.
- No new corrective or deferred registered work was created.
- Historical coding-agent tool-surface M005/M006 closure records were not
  modified.
