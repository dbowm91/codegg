# Runtime Safety, Resource Control, and Footprint Milestone 004 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/runtime-safety-resource-footprint/004-grep-concurrency-and-context-efficiency.md`

Source subsystem roadmap:

- `plans/subsystems/runtime-safety-resource-footprint-roadmap.md`

Repository baseline reviewed: `4d540ce315c9ef2a1c07544cd42df0efc43708e1`

Implementation commits:

- `7ffe198` — bound grep workers and context extraction
- `6b46655` — begin grep milestone closure review

## 1. Executive finding

M004 is implemented and strictly closed. Grep now creates one blocking task per
deterministic worker batch, holds an owned semaphore permit through the entire
blocking batch, applies shared match/output limits and cancellation, and reads
context source at most once per matched file. Results are merged by canonical
path order, and the existing tool syntax, traversal policy, matcher, and
human-readable output shape remain available.

No persistent index, cache, search database, new benchmark framework, or
filesystem-authority expansion was introduced.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Permits cover complete blocking work | `run_search_batches` acquires `acquire_owned()` and moves the permit into `spawn_blocking` | pass | The guard remains live until the worker closure returns. |
| Blocking-task admission is bounded | `partition_paths` creates at most `min(worker_limit, file_count)` batches; focused test uses 120 files and limit 4 | pass | Observed 4 blocking tasks, never one per file. |
| Active worker concurrency is bounded | `SearchMetrics::max_active_workers`; multi-thread focused test | pass | Observed maximum was at most the configured limit of 4. |
| Context is decoded once per matched file | `ContextSnapshot::from_path`; multiple-match focused test | pass | Two matches with before/after context recorded exactly one context read. |
| Match, rendered-byte, and context bounds are explicit | `MAX_GLOBAL_RESULTS`, `MAX_RENDERED_BYTES`, `MAX_CONTEXT_LINES`, and `MAX_CONTEXT_FILE_BYTES` | pass | Final output is capped exactly and reports truncation. |
| Cancellation and timeout stop work | shared `SearchControl`, cancellation checks between files/matches/context lines, and bounded `tokio::time::timeout` | pass | Focused cancellation test stops a batch before file processing; timeout cancels worker state without waiting indefinitely. |
| Deterministic result ordering | path sort in `GrepTool::execute`; end-to-end two-file ordering test | pass | Parallel completion order does not determine output order. |
| Existing search policy remains intact | unchanged `WalkBuilder` hidden, ignore, symlink, canonical-path, and walk-entry handling; 54-test tool integration target | pass | No persistent traversal or protocol change. |

## 3. Production implementation evidence

- `src/tool/grep.rs` replaces file-count-derived task creation with a bounded
  deterministic batch plan.
- Each blocking worker owns its semaphore permit for the complete search and
  context-rendering lifetime. Workers process their assigned files serially.
- `SearchControl` atomically limits global matches and rendered bytes and
  propagates cancellation to all workers.
- `ContextSnapshot` reads a matched file once, builds line storage once, and
  derives all before/after windows from that snapshot. The snapshot is dropped
  when that file's result is finalized.
- Results are sorted by canonical `PathBuf` before the exact global count and
  rendered-byte caps are applied.
- `architecture/tool.md` now documents the bounded worker-batch and
  single-context-read contract.

## 4. Verification executed

Commands run locally on the accepted implementation revision:

```text
cargo fmt --all -- --check                                      pass
cargo check -p codegg --lib --tests                             pass
cargo test -p codegg grep --lib -- --test-threads=1             8 passed
cargo test --test tool_execution -- --test-threads=1            54 passed
cargo clippy -p codegg --lib --tests -- -D warnings             pass
CARGO_BUILD_JOBS=1 cargo check --workspace --all-targets --locked pass
scripts/verify.sh quick                                        pass
```

The quick verification also passed the generated-agent checks, Tokio test
flavor guard, codegg-core boundary guard, and sandbox contract guard.

The feature branch push was accepted at `7ffe198`. The repository workflow is
configured for pull requests and `main` pushes, so no hosted `verify` run was
created for this direct feature-branch push. The available hosted runs were
for different commits and are not claimed as evidence for M004.

## 5. Invariant review

- Semaphore ownership matches the resource being bounded; permits cannot be
  released before a worker's blocking search and context work completes.
- At most the configured worker count of blocking tasks is admitted, and the
  focused 120-file test proves task count is independent of file count.
- Shared atomic limits prevent substantial result overrun; final merging caps
  both match count and rendered bytes exactly.
- Cancellation is checked before batches, between files, during match and
  context processing, and on the timeout path. Owned permits are released by
  closure drop on all normal, error, and cancellation paths.
- Ignore, hidden-file, symlink, binary, canonical-root, and file-walk behavior
  remains in the existing walker/searcher path.
- Context line numbers and context markers are derived from one file snapshot;
  no process-global content cache is retained.
- Final path ordering is stable after parallel execution.

## 6. Failure and recovery review

Unreadable, deleted, binary, and oversized context files continue to produce
the matched search lines without aborting unrelated results. Searcher errors
remain non-fatal as before. Regex construction errors remain reported as tool
execution errors. A timeout sets shared cancellation and returns a bounded
timeout error while dropped join handles allow the owned blocking workers to
finish and release their permits safely.

## 7. Migration and compatibility review

No database, durable state, daemon protocol, tool schema, dependency, or
configuration migration was required. Existing patterns, path filters, ignore
behavior, context input, result formatting, and read-only authority remain
supported. The explicit context/output limits only bound previously unbounded
buffering; truncation is surfaced in the existing human-readable result.

## 8. Security review

The change does not broaden filesystem traversal or follow symlinks. Existing
path validation and canonical-root checks remain before worker admission.
Atomic bounds prevent a large search from creating one task per file or
retaining unbounded rendered output. No secrets, credentials, or persistent
search data are introduced. No critical, high, or medium security finding
remains.

## 9. Documentation and operations

- Updated `architecture/tool.md` with the worker-batch and context-read model.
- Focused instrumentation is per-search and discarded; it is used by tests and
  debug logging only, not as a telemetry backend.
- Operational verification remains `scripts/verify.sh quick` plus the focused
  grep and tool-execution commands listed above.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| none | No unresolved M004 correctness, resource, compatibility, or security finding | none | none |

## 11. Roadmap disposition

M004 is closed. M005 remains ready and may proceed independently. M004 is only
a soft dependency for M007's final measurements; M007 remains blocked by its
hard dependencies on M002, M003, M005, and M006. The runtime-safety roadmap
remains active because those later milestones and the conditional M001/M002
evidence work remain outstanding.

## 12. Registry updates

- Marked the implementation plan `implemented` and accepted this closure record.
- Removed M004 from dependency-ready work and recorded it as closed.
- Updated the roadmap disposition to show M004 closed and M005 ready.
- Audited every blocked runtime-safety plan and each roadmap dependency graph:
  no registered plan became dependency-ready because M004 has no hard
  dependents; M007 lists M004 only as a soft final-measurement dependency.
- Left M003, M006, M007, and M008 blocked with their existing precise blockers.
