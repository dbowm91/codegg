# Agent Convergence Milestone 001 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/agent-convergence/001-durable-convergence-cycle-foundation.md`

Source subsystem roadmap:

- `plans/subsystems/agent-convergence-roadmap.md#10-milestones`

Repository baseline reviewed: `1bee32578566cc6cdf4025002af781309d8f29f4`

Implementation commits:

- `18397ab1266c72aae45a2f6b3bef64163166c7ec` — durable convergence domain, stores, migration, evidence assembler, reconciliation classifier, bounded summary seam, and architecture documentation.
- `ffc3847c711a3ce7b410a1a59c205da8356dc645` — previous-layout migration regression coverage.

## 1. Executive finding

M001 is complete and strictly closed. CodeGG now has a core-only, host-owned
durable convergence foundation. It accepts and persists one exact turn/run
owner, bounded objective and criteria text with SHA-256 fingerprints, a
revision-checked lifecycle, bounded cycle references, semantic verdicts, and
owner decisions. SQLite and in-memory stores implement the same restricted
operations. The implementation does not schedule, spawn, authorize, mutate a
worktree, integrate Git, or complete a goal.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Exact turn/run owner and bounded idempotency identity | `AgentOrchestrationOwner`, `ConvergenceId`, `NewConvergence::validate`; in-memory/SQLite idempotency tests | pass | Owner shape is turn or existing run; no synthetic root run. |
| Exact objective/criteria survive restart | `ConvergenceSpec`; SQLite get/reload test compares exact spec and digest | pass | No transcript or model prose is consulted. |
| Cycle references runs/groups without copying run state | `ConvergenceCycleRecord` and `set_producer_references`/`set_verifier_run` | pass | SQLite stores typed reference IDs and bounded JSON ID list only. |
| Illegal, stale, and terminal-reopening transitions fail closed | `validate_transition`; revision checks; terminal and invalid graph tests | pass | Terminal statuses have no outgoing transitions. |
| Hard cycle ceiling of four | `MAX_CYCLES = 4`, SQL checks, ordinal and repair/replan budget checks | pass | Requested `max_cycles` is validated in both stores. |
| Typed bounded verdict and owner decision persistence | `SemanticVerificationVerdict`, `ConvergenceDecision`, SQLite cycle columns/JSON, first-valid tests | pass | Decisions require an existing matching verdict. |
| Semantic `Pass` remains advisory, not goal completion | Type documentation, `decision_allowed`, evidence tests, no goal-service call/import | pass | No convergence code can produce `GoalVerificationVerdict::Met`. |
| Bounded evidence assembled from authoritative result fields | `assemble_verifier_evidence`, `AgentRunResult::bounded`, redaction-by-construction DTO | pass | Transcript, hidden reasoning, tool arguments, environment, credentials, and raw output are not inputs. |
| In-memory and SQLite stores plus migration | Six `agent_convergence` tests, including SQLite round trip and v48→v49 rerun; schema migration 49 | pass | Existing root migration target was separately attempted but could not link on this host; see verification notes. |
| Deterministic restart reconciliation without scheduling | `classify_reconciliation` table cases and pure function | pass | Missing work yields a resume/attention classification only. |
| No new scheduler/worker/worktree/permission/team authority | Core boundary guard, implementation imports only existing core run/group/result types, no execution calls | pass | `agent_convergence.rs` is a state/store/evidence module. |
| Architecture/storage documentation current | `architecture/agent.md`, `architecture/storage.md`, `architecture/overview.md` | pass | M001 ownership, migration 49, and summary seam documented. |
| Focused and quick verification | Commands in §4 | pass | Quick verification passed. |

## 3. Production implementation evidence

- `crates/codegg-core/src/agent_convergence.rs` defines the typed durable
  identity, owner, bounded specification, lifecycle graph, cycle records,
  verdict/decision types, pure evidence packet, reconciliation classifier,
  and bounded projection summary.
- `InMemoryConvergenceStore` and `SqliteConvergenceStore` expose only bounded
  creation, exact-owner listing, CAS lifecycle transitions, cycle creation,
  first-valid references/verdicts/decisions, and bounded recovery listing.
- Migration 49 adds `agent_convergence` and
  `agent_convergence_cycle` with owner/status/recovery indexes and a unique
  idempotency key. Existing agent-run/group/goal/worktree rows are not
  rewritten.
- The core module does not import scheduler, worker, worktree, permission,
  goal, prompt, or protocol authority. The summary remains an internal seam;
  no unused wire-protocol version change was made.

## 4. Verification executed

### Commands run

```bash
cargo fmt --all -- --check
git diff --check
cargo test -p codegg-core agent_convergence --locked
cargo test -p codegg-core run_result --locked
cargo clippy -p codegg-core --all-targets --locked
scripts/verify.sh quick
cargo test --test storage_migrations --locked
```

### Results

- `cargo test -p codegg-core agent_convergence --locked`: pass, 6 tests.
- `cargo test -p codegg-core run_result --locked`: pass, 1 test.
- `cargo clippy -p codegg-core --all-targets --locked`: pass, no issues.
- `scripts/verify.sh quick`: pass; formatting, generated-agent, core-boundary,
  sandbox, execution-ownership, workspace all-target check, and quick checks
  completed successfully.
- `cargo test --test storage_migrations --locked`: environmental block during
  root test-harness linking. The host target is `x86_64-apple-darwin` while
  `/opt/local/lib/liblzma.dylib` and `libiconv.dylib` are `arm64`, producing
  undefined x86_64 lzma symbols. The core SQLite test independently exercised
  fresh migration, restart-safe rerun, terminal persistence, and explicit
  v48→v49 migration recreation and passed.

## 5. Invariant review

- Convergence IDs are opaque, path-independent, validated on parse and
  deserialization, and distinct from run/group/job/turn/goal IDs.
- Objective and criteria are bounded, stored as text plus digests, and are
  never reconstructed from transcript content.
- The lifecycle graph is explicit; terminal records cannot reopen and all
  writes that change lifecycle state use a revision check.
- Cycle records retain handles and bounded structural evidence, not complete
  run results or transcripts.
- A semantic pass is explicitly advisory and has no host-goal authority.
- Normal summaries omit full specs, findings, diffs, and evidence details.

## 6. Failure and recovery review

- Duplicate creation with the same key and fingerprint returns the original;
  changed specifications fail with `IdempotencyConflict`.
- Duplicate cycle ordinals, changed producer/verifier references, changed
  verdicts, and second decisions fail closed.
- CAS transitions and owner decisions reject stale revisions. SQLite owner
  decisions update cycle and record state in one transaction.
- Terminal and partial/missing run/group states are classified as no-change,
  failure/cancellation, execution-resume, or attention; no classifier branch
  schedules work or invents success.
- All public text, IDs, collections, cycle counts, JSON fields, and evidence
  envelopes have code-level bounds. Artifact values remain references.

## 7. Migration and compatibility review

Migration 49 is additive and idempotent under the existing migration-version
transaction. It can upgrade a layout at version 48 without a manual data
migration, adds only the two new tables and demonstrated indexes, and does
not alter existing durable run/group/goal/worktree rows. `STORAGE_LAYOUT_VERSION`
is 49 and the storage architecture documentation is updated. No protocol
version or legacy team inbox/outbox behavior changed.

## 8. Security review

The convergence handle does not grant authority; this module has no
authorization or execution capability. Owner and identity values reject path,
control, and malformed identity input. Objective/criteria, idempotency keys,
digests, verdict text, findings, artifact labels/references, and commit
references are bounded before persistence. Evidence assembly has no API for
secrets, credentials, environment variables, hidden reasoning, tool
arguments, or raw transcript/output propagation. SQLite writes use bound
parameters and migration-owned transactions.

## 9. Documentation and operations

Updated `architecture/agent.md`, `architecture/storage.md`, and
`architecture/overview.md`. No new static guard, CI lane, scheduler loop, or
operator command was required. M002 is the next consumer and owns any future
execution/projection publication behavior.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| low | Root `storage_migrations` integration target cannot link in this local x86_64-on-arm64 MacPorts toolchain. | Local root migration executable evidence is unavailable; core SQLite migration evidence is green and quick workspace checking passes. | Re-run the root target on a matching arm64 toolchain or with x86_64-compatible lzma/iconv libraries. No M001 code change is required. |

No critical, high, or medium findings remain. The low environmental linker
condition does not leave a product correctness or security requirement
unimplemented, so it does not prevent strict closure.

## 11. Roadmap disposition

M001 is closed and the next dependency may proceed. M002 is unblocked and is
marked `ready`; M003 remains blocked on M002. The subsystem roadmap remains
active planning because later verifier and bounded repair capabilities are
not part of M001.

## 12. Registry updates

- Mark the M001 implementation plan `implemented`.
- Add this accepted closure record at
  `plans/closure/agent-convergence/001-status.md`.
- Mark M001 closed in the agent-convergence roadmap and move M002 to `ready`.
- Update `plans/registry.md` so the subsystem is `ready`, M002 is the sole
  dependency-ready convergence plan, and M003 remains blocked on M002.

