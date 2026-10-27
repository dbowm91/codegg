# Context Continuity Post-Closure Corrective — Milestone C001 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/context-continuity-compaction-post-closure-corrective/001-provider-visible-sequence-survives-the-turn-boundary.md`

Source subsystem roadmap:

- `plans/subsystems/context-continuity-compaction-post-closure-corrective-addendum.md`

Applicable ADR: `plans/adrs/ADR-0054-daemon-owned-provider-visible-history-across-turn-boundaries.md` (accepted 2026-10-26)

Repository baseline reviewed: `957fcbf00e17bf108429f0d75dcbf760736ae511` (`origin/main`)

Implementation commits or pull requests:

- `07c53d9ee` — `fix(context): C001 — the daemon owns the provider-visible
  sequence across the turn boundary`. The C001 production change, its
  two-turn regression, the base-drift repairs, and the comment-budget
  re-bank.
- `06916912f` — `docs(plans): record ADR-0054 and the C001 implementation
  plan`. ADR-0054, plan 001, and the subsystem addendum.
- This closure record, the registry rows and the M003 receipt drift note are
  in the following `docs(closure)` commit.

Branch: `feat/auto-20261026-7c94ea67`, opened as a pull request against
`main`. The hosted `CI / verify` job remains the merge gate.

## 1. Executive finding

C001 is **closed**. The daemon now owns the provider-visible sequence across a
turn boundary: when a turn's history was compacted into an installed
continuation checkpoint, the daemon removes the **exact** superseded frontend
prefix from the next turn's provider-visible history and re-installs the
checkpoint's single continuation projection ahead of the current user message.
The removal is exact and fail-closed — a retained history that is not a
contiguous match is left unchanged, and a bounded structured diagnostic
records the mismatch.

Every acceptance criterion of the implementation plan §10 is met, and every
command of §9 is green. The regression is proven to **fail on the pre-change
baseline** and to pass on the change; strict workspace Clippy is clean;
`scripts/verify.sh quick` passes; and the branch-wide scoped suite passes.

Closing C001 required repairing the pre-existing failures that the scoped gate
surfaced. `origin/main` at `957fcbf00` fails **30** workspace tests of its own
(measured by stashing this branch's changes and running the same suite at the
baseline commit). This branch already repaired 7 of them; the remaining 23 were
repaired here as a prerequisite, and the full workspace suite now passes. Two
of those repairs were real product defects — the `repo_search` contract
declared an output shape its own default projection did not produce, and the
read tool's symlink-escape guard disagreed with the repository's path
validator about the macOS system aliases. Each is recorded in §3.

## 2. Requirement-to-evidence matrix

| # | Acceptance criterion (§10) | Evidence | Result |
|---|---|---|---|
| 1 | After a turn that compacts, the next turn's provider-visible sequence does not contain the pre-compaction body | `c001_agent_loop_replaces_exact_frontend_prefix_across_turns` — the second provider request asserts the superseded prompt and answer are absent | pass |
| 2 | The compaction is effective across the turn boundary | The same regression: the first turn's request is compacted (a continuation frame is present), and the second request carries the compacted sequence | pass |
| 3 | Exactly one CodeGG-owned continuation frame is present | The same regression counts exactly one continuation frame in the second request | pass |
| 4 | The continuation projection precedes the current user message on every profile | `continuation_projection_precedes_current_user_with_or_without_system`; `crates/codegg-core/src/model_profile/policy.rs` placement matrix (30 tests) | pass |
| 5 | The "current user message" reads return the human's prompt | `human_prompt_snapshot_is_not_replaced_by_injected_rows` | pass |
| 6 | A prefix the daemon cannot identify exactly is kept, with a bounded diagnostic | `provider_visible_prefix_identity_is_exact_and_prefix_bounded`; `inject_installed_continuation_for_turn` mismatch arm (retention asserted); `src/agent/context_runtime.rs` emits a `tracing::debug!` carrying only ids, counts, and a version — no message body | pass (the emitted event itself is not captured by a test — §10) |
| 7 | The durable session transcript is unchanged and its user row is still the human's prompt | `two_turn_transcript_identity_covers_compacted_user_and_generated_rows`; the checkpoint carries identity metadata only | pass |
| 8 | The frontend's transcript is not truncated, rewritten or emptied | The change is confined to the daemon's provider-visible sequence; the frontend keeps its complete transcript and submits it unchanged | pass |
| 9 | `CoreRequest::TurnSubmit` and the compose contract are unchanged | No `TurnSubmit`/`AppEvent` variant and no `codegg-client` change in the diff | pass |
| 10 | The checkpoint payload stays within its 128 KiB bound and carries no secret or hidden-reasoning body | The regression measures the payload at 429 bytes against `CONTINUATION_CHECKPOINT_MAX_PAYLOAD_BYTES`; the identity holds a digest, a version, and a count | pass |
| 11 | The compaction engine's policy, thresholds, tracker and invariants are unchanged | No change under `src/context/compaction.rs` or the compaction policy | pass |
| 12 | The two-turn regression fails on the baseline and passes after the change | Recorded failing run at `957fcbf00` in an isolated worktree: the regression failed at the assertion that the superseded frontend prompt and answer must not be resent with the installed continuation. The worktree was removed after the run was recorded | pass |
| 13 | `scripts/verify.sh quick` is green, and fmt and workspace clippy with `-D warnings` are clean | §4 | pass |

## 3. Production implementation evidence

C001's own production change:

- `src/agent/turn_runtime.rs` captures the protocol-message prefix identities
  before converting the incoming DTOs.
- `src/context/rollover.rs` owns the versioned canonical projection, SHA-256
  identity, covered count, compaction-boundary calculation, and the fail-closed
  prefix trim.
- `src/agent/loop.rs` carries the incoming identity and compaction-chain state
  through the turn.
- `src/agent/context_runtime.rs` applies the exact match at turn start,
  preserves any unmatched suffix, and places the installed continuation before
  the current user message.
- `src/agent/follow_up.rs` carries the required turn-boundary state through
  follow-up execution.
- The checkpoint payload adds serde-defaulted identity metadata. **No database
  schema change and no public wire-protocol change was introduced.**

Prerequisite repairs to pre-existing baseline failures (all independent of
C001's production change):

- **`src/tool/repo_search.rs` — the contract's declared output shape was
  wrong.** `detail='summary'` is the default and its projection emits the
  compact card *array*, but the contract declared `{"type": "object"}`, so
  every default `repo_search` call through the broker failed output
  validation with "expected JSON type object". The declared shape now admits
  both the full envelope and the summary card list, which is what the schema's
  own doc comment always intended.
- **`src/tool/broker.rs` — the bounded schema validator could not express
  that.** `validate_json_schema` read `type` as a single string; it now
  accepts a list of accepted type names, and the type match moved into a
  named helper.
- **`src/tool/read.rs` — the read tool's escape guard disagreed with the
  repository's path validator.** `validate_path` admits the macOS system
  aliases (`/var`, `/tmp`, `/etc`), but the read tool's own
  `symlink_escape_message` did not, so every path under a temporary
  directory was refused as a symlink escape on macOS. The guard now honours
  the same alias list (`src/tool/util.rs`).
- **`src/mcp/mod.rs` — the test-only MCP mock could not stand in for a
  capable backend.** The mock is constructed only by the test-only
  `register_mock_server` helper, and it refused the host-scoped call and was
  held to an advertised protocol version that a stub never publishes. Both
  restrictions now apply to real servers only; the mock answers the call.
- **The frozen M003 receipt drifted with the tool schemas.** The receipt's
  effect-catalog fingerprint is bound to the M003-era input schemas, and the
  regression reconstructs them from the live registry. The agentfindings
  tool-contract workstream added post-M003 deltas the reconstruction did not
  cover (`task.run_id`/`task.timeout_ms` descriptions, the `include_ignored`
  discovery property on `glob`/`grep`/`list`, and two `repo_search`
  descriptions). The reconstruction was extended; the receipt asset itself
  was **not** modified. Recorded in
  `plans/closure/tool-advisor-m003-receipt-ci-corrective/001-status.md`.
- **`tests/tool_programs/tool_program_search_palette.inc`** — the palette
  asserted the upstream envelope as the default structured value; the default
  is now the summary projection, so the test asserts the card list and adds
  an explicit `detail='full'` call for the envelope.
- **`tests/tool_programs/tool_program_{language,static_bounds,ir,property}.inc`**
  — the `parallel()` fixtures used descriptors without the `tool` key the
  language now requires.
- **`tests/tool_programs/writer_disposition_guards.inc`** — `math_eval` and
  `unit_convert` declare a read-only effect class and are now in the reviewed
  read-only palette.
- **Platform-qualified tests.** The Git object-view cache's watcher is Linux
  only, so a repeat read is cache-served there and rebuilt elsewhere; the
  scratch allocator follows the platform cache convention, so the explicit
  `XDG_CACHE_HOME` qualification is Linux-only; and the contained-worktree
  grant is built from the Linux protected-mount view, so its qualifications
  are Linux-only. Composing the contained child's launch refuses on any host
  but Linux, so the whole `contained_worktree_hooks_path` target is a Linux
  qualification. Each gate asserts the platform-appropriate outcome; none
  weakens the security invariant it guards.

## 4. Verification executed

All commands below were run **locally** in the worktree. No hosted CI run was
obtained; the repository's hosted `CI / verify` job remains the merge gate.

```bash
# the new two-turn regression, by name
cargo test --locked --test agent_loop -- c001_agent_loop_replaces_exact_frontend_prefix_across_turns

# the continuity families
cargo test --locked --test agent_loop -- context_continuity
cargo test --locked --test agent_loop -- context_epoch_handoff
cargo test --locked --test agent_loop -- long_horizon_trajectory

# the in-crate placement tests
cargo test --locked -p codegg --lib -- context_runtime

# the projection placement matrix
cargo test --locked -p codegg-core --lib -- model_profile

# the workspace and the guards
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
scripts/verify.sh quick

# the branch-wide gate
bash scripts/verify.sh scoped origin/main

# supporting checks
cargo nextest run --workspace --locked --profile ci --no-fail-fast
python3 scripts/rust_test_scan.py --validate
python3 scripts/check_adr_integrity.py
python3 scripts/check_comment_budget.py
python3 scripts/check_comment_budget.py --self-test
git diff --check
```

### Results

- C001 two-turn regression: **pass**, 1/1. The first turn's provider request
  is compacted, and the second request carries exactly one continuation frame
  ahead of the current prompt, with the superseded prompt and answer removed.
- Continuity, epoch-handoff, long-horizon, context-runtime (14/14) and
  model-profile (30/30) suites: pass.
- `cargo clippy --workspace --all-targets --locked -- -D warnings`: **pass**.
  The 12 diagnostics that had blocked this gate were in files outside C001's
  scope; they were repaired, and the gate now reports none.
- `scripts/verify.sh quick`: **pass** — formatting, the generated-asset
  checks, the guard scripts, the harness-lab suite (329 tests, one skipped),
  and the workspace all-target check.
- `bash scripts/verify.sh scoped origin/main`: **pass**. The gate widened to
  the whole workspace for a root-package change, and its own run of that suite
  reported **16,539 passed, 0 failed, 9 skipped**.
- Full workspace suite under the `ci` nextest profile: **pass** — the scoped
  gate's run reports 16,539 passed, 0 failed, 9 skipped. A separate standalone
  run of the same suite failed at exactly one test, the known pre-existing
  daemon-recovery race recorded in §10; that test passed in the scoped run and
  in 8 of 8 isolated re-runs.
- `rust_test_scan --validate`, ADR integrity, comment budget (and its
  self-test), and diff-whitespace checks: pass.

### Baseline comparison

`origin/main` at `957fcbf00` was measured by stashing this branch's changes
and running the same workspace suite at the baseline commit: **30 failures**.
This branch repairs all 30 — 7 of them already repaired by the C001 work, and
23 by the prerequisite repairs in §3. The scoped gate is therefore green for
the first time on this line.

## 5. Invariant review

- The digest is domain-separated and derived from typed, ordered message
  fields; the legacy `source_history_digest` remains diagnostic-only.
- A covered count is stored with the hash. Unsupported versions, absent or
  invalid identities, count errors, conversion ambiguity, peer/control
  provenance, or mismatches do not authorize deletion.
- Only an exactly identified prefix is removed. Any incoming suffix remains in
  order, and a non-contiguous retained history leaves the whole history
  untouched.
- The installed continuation is injected before the current human turn; the
  current request prompt is retained.
- Checkpoint identity contains no message bodies; the frontend transcript and
  the public turn-submit contract are unchanged.
- The repository's path validator and the read tool's escape guard now agree
  on the macOS system aliases, so the platform convention is applied once.

## 6. Failure and recovery review

An absent, corrupt, unsupported, or mismatched checkpoint identity leaves the
submitted history intact and emits a bounded diagnostic; continuation
injection remains separate from prefix deletion. The existing M004 storage,
restart, transaction, cancellation, and contention suites pass in the focused
run. The two-turn regression now drives a real automatic compaction inside the
first `AgentLoop::run` and asserts the installed continuation across the
boundary, so the compaction-to-next-turn lifecycle is covered end to end by
one test.

The `repo_search` output-schema repair is the one place where a *product*
defect was found rather than a stale expectation: the broker's output
validation rejected the tool's own default projection. The repair makes the
declared contract match the two shapes the tool can produce, which is what the
schema's doc comment already intended.

## 7. Migration and compatibility review

The identity fields are additive serde-defaulted checkpoint data. Old
checkpoints without the new identity are treated as non-matches, preserving
history rather than risking a drop. No SQLite migration, `TurnSubmit` wire
change, or frontend transcript migration was made. The compatibility fallback
may temporarily send duplicate context for old or ambiguous checkpoints; that
is the deliberate fail-closed behavior.

The `repo_search` contract change widens a declared output schema; it changes
no value a caller receives, only the schema that describes it.

## 8. Security review

Only a cryptographic digest, version, and count are persisted for the source
prefix; message content is not copied into the identity. The comparison is
exact and fail-closed, preventing a stale or ambiguous checkpoint from
authorizing deletion. The checkpoint remains under the pre-existing 128 KiB
bound; the regression payload measured 429 bytes. No new authority, secret,
path, or filesystem surface was introduced.

The repair to the read tool's escape guard **narrows** a refusal rather than
widening one: a macOS system alias is no longer treated as an escape, and the
path validator's own root comparison still decides the outcome. The contained
worktree, writer-permit, and checkout-mediation authorities are untouched.

## 9. Documentation and operations

- `architecture/context-compaction-ownership.md` — C001's daemon ownership and
  prefix-matching contract, plus source-verification evidence.
- `architecture/compaction.md` — the turn-boundary replacement rule and the
  contiguous-opening-anchor rule.
- `plans/adrs/ADR-0054-daemon-owned-provider-visible-history-across-turn-boundaries.md`
  — the ownership decision.
- The C001 implementation plan, the corrective addendum roadmap, and this
  closure record.
- `plans/closure/tool-advisor-m003-receipt-ci-corrective/001-status.md` — the
  frozen-receipt drift repair that was a prerequisite for the scoped gate.
- No new static guard was added; the exact-identity unit tests, the two-run
  AgentLoop regression, and the existing guard scripts provide the regression
  protection.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| low | The bounded mismatch diagnostic is emitted through `tracing::debug!` and is not captured by a test; the *retention* behaviour it accompanies is asserted | The diagnostic's own shape is qualified by inspection rather than by assertion | Capture the event with a tracing subscriber in a follow-up if the observable form must be pinned |
| low | No hosted CI run was obtained | The repository's hosted `CI / verify` job remains the merge gate | Run the hosted gate on the branch before merge |
| low | `scripts/verify.sh scoped origin/main` widens to the whole workspace for a root-package change, so it also selects unrelated suites | The scoped gate's signal is coarser than the plan's own §9 list | None — the full workspace suite is green |
| low | `tool_programs::tool_program_m015_daemon_failpoints::recursive_descendants_and_capacity_converge_after_cancel_crash` is a **pre-existing** load-dependent flake, not a product defect of this change. The test's `first.kill()` waits for the daemon's exit but not for its descendants, and the replacement daemon then opens the same SQLite job store while those descendants still hold it; when the lock has not yet been released the daemon's job recovery produces no report and the two jobs stay `running`. The line above the failure is the daemon's own `job recovery failed … (code: 5) database is locked` | One test in the full-suite run can fail under load and pass on a re-run; it says nothing about C001 | The owning line is the tool-programs line. The same failure and mechanism are already recorded in `plans/closure/ci-test-throughput-recovery/008-merge-and-absorption-status.md` §"the four lanes that were red on `main`", where the identical tree's re-run passed. This branch's evidence: the test source is byte-identical to the base branch's, the test passed in the scoped gate's own full-suite run and in 8 of 8 isolated re-runs, and it failed only in one earlier standalone run | Close the race in the test — wait for the descendants to release the store before the replacement daemon opens it |

No high or medium finding remains open.

## 11. Roadmap disposition

C001 is **closed**. The corrective addendum roadmap has no further milestone:
its sole milestone is closed, and the predecessor `context-continuity-compaction`
M001–M004 line stays closed and unchanged. The prerequisite repairs in §3 are
recorded here and, for the frozen receipt, in the tool-advisor closure record;
they do not open a new corrective line, because each was a stale expectation
or a narrow contract defect rather than an unclosed milestone requirement.

## 12. Registry updates

- The C001 implementation plan moves to `implemented`, and this record is its
  closure.
- The corrective addendum roadmap moves to `closed`, with C001 closed.
- The C001 rows leave the active and dependency-ready sections and are
  recorded under recently closed work.
- The blocked-work row for C001 is removed: its three blockers are discharged —
  the baseline-red run is recorded, strict workspace Clippy is clean, and the
  scoped suite passes.
- Dependency audit: no registered plan lists C001 as a hard or interface
  dependency, so nothing else is moved to `ready`. The closure unblocked
  nothing, and that is recorded here rather than left implicit.
