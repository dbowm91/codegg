# Runtime Safety, Resource Control, and Footprint Milestone 004 — Grep Concurrency and Context Efficiency

Status: implemented

Source subsystem roadmap:

- `plans/subsystems/runtime-safety-resource-footprint-roadmap.md`
- Milestone 004

Repository baseline reviewed: `4d540ce315c9ef2a1c07544cd42df0efc43708e1`

Dependencies:

- no hard dependency;
- may execute in parallel with M001 and M005;
- final footprint measurements in M007 should use the completed M004 tree when practical.

Primary class: resource correctness and bounded performance

Target closure record:

- `plans/closure/runtime-safety-resource-footprint/004-status.md`

## 1. Objective

Correct grep worker admission so the configured semaphore actually bounds blocking work, prevent large searches from queuing an excessive number of `spawn_blocking` tasks, and produce before/after match context without rereading an entire file for each match.

The accepted implementation must preserve:

- current search syntax and matcher behavior;
- ignore/hidden/file-filter behavior;
- cancellation;
- result limits and user-facing formatting;
- deterministic ordering where the current API promises it;
- bounded memory and blocking-thread use.

This is a focused search-execution repair, not a persistent indexing project.

## 2. Explicit non-goals

This milestone must not:

- introduce a search database, file watcher, vector index, daemon-wide cache, or persistent line index;
- replace the grep crates with a custom regex engine;
- add one task per file or one task per match;
- add long-running benchmark suites or large generated corpora to routine CI;
- change search result semantics merely to improve throughput;
- redesign the tool protocol or frontend search UI;
- expand filesystem traversal authority;
- follow symlinks or search ignored paths more broadly than current policy;
- perform dependency cleanup assigned to M005 except for source changes required to use the already selected grep crates correctly.

## 3. Current implementation evidence

Inspect at minimum:

- `src/tool/grep.rs`;
- helper modules used for filesystem traversal, matching, context rendering, and result limits;
- cancellation token/owner integration;
- tests for grep ordering, context, ignore rules, binary files, limits, and cancellation;
- Cargo grep dependencies and feature use, without changing them in this milestone unless source correctness requires it.

The reviewed baseline shows:

1. a semaphore permit is acquired before `spawn_blocking` is created but is dropped before the blocking closure performs the search, so the semaphore limits task creation briefly rather than actual blocking work;
2. batching derived from file count can still create many queued blocking tasks on large trees;
3. match-context extraction calls a helper that reads the entire file, and the surrounding code can call it separately for before and after context for each match;
4. multiple matches in one file therefore cause repeated whole-file reads and decoding;
5. cancellation and result-limit behavior must be preserved while correcting the resource model.

Confirm actual code flow before editing. Record any baseline change in the closure record.

## 4. Invariants that cannot regress

- the configured concurrency limit covers the entire blocking search operation;
- the number of queued blocking tasks remains bounded by a small function of configured concurrency, not total file count;
- cancellation releases permits and stops producing results promptly;
- result count and byte limits are enforced before unbounded accumulation;
- one slow or unreadable file does not abort unrelated valid results unless current API requires fail-fast behavior;
- ignore, hidden-file, symlink, binary-file, and size policies remain unchanged;
- context lines correspond to the correct file snapshot and line numbers;
- a file with many matches is read/decoded at most once for context during one search operation, unless the matching backend itself streams the file once and supplies context directly;
- result ordering is deterministic after parallel execution;
- no global cache retains file content after the search request ends.

## 5. Required concurrency design

Choose one simple bounded model.

Preferred model:

1. determine `worker_count = min(configured_limit, nonempty_work_units)` with a minimum of one;
2. partition candidate files into at most `worker_count` deterministic batches;
3. acquire an owned semaphore permit for each worker;
4. move the permit into the `spawn_blocking` closure so it remains live until the blocking batch completes;
5. each worker processes its batch serially, checks cancellation between files and at bounded intervals, and produces a bounded partial result;
6. join at most `worker_count` tasks;
7. merge and sort results deterministically;
8. stop scheduling/processing once global limits or cancellation require termination.

An async worker queue is acceptable if it creates only a bounded number of blocking workers. Do not enqueue one blocking closure per file.

The permit must not be released before the blocking operation finishes. Use an owned permit or move a guard into the closure; avoid lifetime workarounds that duplicate semaphore state.

## 6. Required context-extraction design

Use the simplest approach that avoids repeated whole-file reads.

Acceptable options:

### Option A — matcher-provided context

Configure the existing grep searcher to emit before/after context in one streaming pass when its API supports the required line numbers, formatting, binary policy, and result limits.

### Option B — one per-file context snapshot

For each file with matches:

1. read/decode the file once under the existing file-size and binary policy;
2. build line start offsets or a `Vec` of borrowed/owned line slices once;
3. derive all requested context windows for that file;
4. coalesce overlapping windows for rendering where current semantics allow;
5. release the file buffer when that file's results are finalized.

Do not call a whole-file `read_context_lines` helper independently for each match and direction.

Context memory must remain bounded by existing maximum file size or a newly explicit focused limit consistent with current behavior. Large/binary files must follow existing skip/truncation policy.

## 7. Global limits and cancellation

Inventory current limits for:

- candidate files;
- matches;
- rendered bytes;
- per-file matches;
- file size/context size;
- elapsed time if present.

Ensure parallel workers share a cancellation/limit state that prevents substantial overrun. Exact result count may exceed the threshold by at most a small documented in-flight bound, and the merged user-visible result must be capped exactly.

Use atomics or a small shared state only where needed. Do not add a complex distributed quota subsystem.

Cancellation must be checked:

- before starting a batch;
- between files;
- during long file processing at a bounded interval supported by the matcher/searcher;
- before expensive context rendering;
- before merging additional results.

## 8. Deterministic result merging

Parallel completion order must not leak into output order.

Define or preserve a stable sort key such as:

```text
normalized relative path
line number
column/start offset
match ordinal
```

Do not sort by lossy display text if a path/position key exists. Preserve platform path behavior and current case sensitivity.

When the global result limit is reached, apply it after deterministic ordering unless the existing API explicitly promises traversal-order truncation. If changing truncation ordering would be user-visible, preserve current semantics and document the bounded in-flight behavior.

## 9. Expected production-code changes

Expected areas:

- `src/tool/grep.rs` worker planning, semaphore ownership, cancellation, result merging, and context extraction;
- small helper types for per-file matches/context if needed;
- focused tests and test-only instrumentation;
- architecture/tool documentation if current concurrency claims are inaccurate.

Avoid broad filesystem abstraction changes. Keep the implementation local to the grep tool unless an existing shared bounded-blocking helper clearly owns the behavior.

## 10. Storage, protocol, migration, and compatibility effects

Storage:

- no database or durable-state change;
- no persistent cache;
- temporary per-search buffers are released at request completion/cancellation.

Protocol:

- no breaking tool result change expected;
- optional diagnostics such as truncated/cancelled may be preserved or made more explicit through existing fields;
- ordering and context text should remain compatible.

Compatibility:

- current patterns, file filters, ignore rules, context count options, and output formatting remain supported;
- unreadable files follow current error/skip behavior;
- binary/large-file behavior remains current unless a concrete unsafe unbounded case requires an explicit limit and documentation.

## 11. Ordered work packages

### Work package A — Baseline resource tests

Add focused tests or test-only hooks that can observe:

- maximum simultaneous blocking workers;
- number of blocking tasks created;
- number of context reads per file;
- cancellation completion;
- deterministic result order.

The instrumentation must be test-only and must not add runtime telemetry infrastructure.

### Work package B — Bounded batching and permit ownership

1. replace file-count-derived unbounded task queuing with at most configured worker count;
2. move owned permits into blocking worker lifetime;
3. process deterministic batches serially per worker;
4. release permits on success, error, panic propagation, and cancellation.

### Work package C — Single-pass context extraction

1. group matches by file;
2. use matcher-provided context or one per-file snapshot;
3. derive all windows from the single pass;
4. preserve line numbers and formatting;
5. remove repeated whole-file helper calls.

### Work package D — Limit/cancellation reconciliation

1. centralize global result/byte termination state;
2. check cancellation at bounded points;
3. cap final merged output exactly;
4. ensure task join does not wait indefinitely after cancellation.

### Work package E — Deterministic merge and cleanup

1. sort by stable path/position key;
2. preserve current duplicate/coalescing behavior;
3. delete obsolete batching/context helpers;
4. update comments/docs.

## 12. Focused verification

Required test scenarios:

- more candidate files than worker limit;
- configured worker limit of one;
- multiple matches in one file with before/after context;
- overlapping context windows;
- multiple files completing in different worker order but producing stable output;
- cancellation during a large file batch;
- result-limit termination with several workers in flight;
- unreadable/deleted file race;
- binary or oversized file according to current policy;
- zero matches and empty candidate set;
- test proof that max simultaneous workers never exceeds the configured bound;
- test proof that context source is read once per matched file, not per match.

Expected command shape:

```bash
cargo fmt --all -- --check
cargo check --workspace --all-targets
cargo test <grep unit target> -- --test-threads=1
cargo test <grep integration target> -- --test-threads=1
scripts/verify.sh quick
```

Use current target names. Do not add wall-clock performance assertions that are flaky on shared runners.

A local diagnostic run on a moderately large repository may be recorded, but it is not a CI requirement and is not closure evidence by itself.

## 13. Static guards

A permanent source guard is optional. Prefer behavioral tests.

If a cheap guard is added, it may reject the specific pattern of acquiring a permit and dropping it before `spawn_blocking` in `src/tool/grep.rs`. Do not create a general concurrency linter.

Document the intended maximum task count in a code comment adjacent to worker creation and assert it in a test.

## 14. Acceptance criteria

M004 is complete only when:

- semaphore permits remain held for the full blocking worker lifetime;
- the number of blocking tasks is at most the configured worker count or another equally small documented bound;
- large file sets cannot enqueue one blocking task per file;
- context extraction does not reread a file for every match/direction;
- focused instrumentation proves bounded concurrency and one context source read per matched file;
- cancellation, result limits, ignore rules, binary policy, and formatting remain correct;
- parallel completion produces deterministic output;
- no persistent index/cache or new CI benchmark infrastructure is introduced;
- focused tests and `scripts/verify.sh quick` pass;
- no unresolved high/medium resource or correctness finding remains.

## 15. Stop conditions

Stop and report blocked when:

- the current grep API cannot provide cancellation or context without changing user-visible semantics and the alternative requires a new search engine;
- result ordering is undocumented and multiple existing tests conflict;
- file-size/binary policy is unbounded and requires a separate product decision;
- fixing worker admission exposes a global blocking-pool ownership problem outside this tool.

Prefer one narrow follow-up plan over persistent indexing or scheduler redesign.

## 16. Required closure evidence

`plans/closure/runtime-safety-resource-footprint/004-status.md` must include:

- accepted commit/PR;
- old and new worker/task model;
- configured concurrency and observed maximum in focused tests;
- task-count proof for a file set larger than the limit;
- context-read-count proof for multiple matches in one file;
- cancellation, limit, and deterministic-order results;
- focused commands, quick verification, and hosted run reference;
- unresolved findings by severity;
- confirmation that no persistent index or broad benchmark framework was added.
