# Memory-to-Skill Promotion Milestone 004 — Habit Lock OpenOptions and Hosted Closure Corrective Pass

Status: implemented

Repository baseline: `4ea4eaa000ecf65b0e70ed7278cf071a57cf2197`

Source corrective roadmap/addendum:

- `plans/subsystems/memory-skill-promotion-hosted-verification-corrective-addendum.md`

Original milestone and closure evidence corrected by this pass:

- M001 plan: `plans/implementation/memory-skill-promotion/001-habit-observation-and-candidate-store.md`
- M001 implementation: `2f029d8dd7de49876cf6527c835e586bd3d46e3c`
- M001 closure: `plans/closure/memory-skill-promotion/001-status.md`
- M003 plan: `plans/implementation/memory-skill-promotion/003-approved-skill-publication-and-refresh.md`
- M003 implementation: `081ae511a456b3892079a6da3e7e08fe56f6e0b0`
- M003 closure: `plans/closure/memory-skill-promotion/003-status.md`

Long-term requirements:

- `plans/000-long-term-specification.md#26-reliability-and-recovery`
- `plans/000-long-term-specification.md#27-security-requirements`
- `plans/000-long-term-specification.md#29-system-invariants`
- `plans/001-terminology-and-domain-model.md`
- `plans/003-planning-process.md#7-corrective-passes`

Applicable architecture:

- `architecture/memory.md`
- `architecture/skills.md`
- `architecture/tool.md`

Primary class: corrective reliability / verification / closure

## 1. Objective

Correct the two M001-owned `clippy::suspicious_open_options` failures in the habit-store advisory lock path without changing persistence or locking semantics, then restore truthful strict subsystem closure with one green exact-head hosted CI run that reaches and passes Workspace tests.

This is intentionally a very small corrective pass. The memory-to-skill feature behavior is already implemented. Do not use this milestone to redesign habit observation, proposal drafting, publication, asset refresh, or CI.

## 2. Discovered defects

### 2.1 Ambiguous advisory-lock file open semantics

Exact `main` revision `4ea4eaa000ecf65b0e70ed7278cf071a57cf2197` failed GitHub Actions run `33813852632`, job `100841494152`, at:

```bash
cargo clippy --workspace --all-targets --locked -- -D warnings
```

The hosted runner used Rust `1.98.1`. Clippy reported two `suspicious_open_options` errors in `crates/codegg-core/src/memory/habit.rs`, approximately lines 408 and 552.

Both sites construct the project habit lock file as:

```rust
OpenOptions::new()
    .create(true)
    .write(true)
    .open(lock_path)?
```

The code's intended behavior is to create the lock inode when absent and otherwise open the existing synchronization file without treating its contents as payload. The truncate policy is currently implicit, which Rust 1.98.1 rejects under repository `-D warnings`.

### 2.2 Closure attribution was too narrow

M003 closure recorded the same standalone Clippy findings but categorized them as unrelated pre-existing repository findings because M003 itself did not introduce `habit.rs`.

That classification is too narrow for subsystem closure. `habit.rs` was introduced by the memory-to-skill M001 implementation, so these warnings are attributable to this workstream even though they predate M003.

Historical closure records are evidence and MUST NOT be rewritten. M004 supersedes only the current strict disposition.

## 3. Why original verification did not catch or close the defect

- M001 focused tests and `cargo check` did not exercise the workspace/all-target Clippy lint set that later failed.
- M001 recorded `scripts/verify.sh quick` as passing on its local environment.
- M003's local focused/quick verification also passed, while a standalone Clippy invocation exposed the warnings.
- the exact hosted Linux push lane subsequently installed Rust 1.98.1 and failed the repository's canonical workspace Clippy command before Workspace tests.
- M003 closure preserved the warning evidence but treated it outside subsystem ownership, allowing the registry to say `closed` while current `main` remained red.

M004 closes that evidence gap. It does not justify a larger verification framework.

## 4. Explicit non-goals

M004 MUST NOT:

- alter habit action vocabulary, effect classification, fingerprints, privacy bounds, readiness thresholds, dismissal/promotion lifecycle, or project scoping;
- persist raw shell commands, tool arguments/results, paths, prompts, environment data, or hidden reasoning;
- alter model-assisted skill proposal initiation or proposal schema;
- add model publication authority;
- change project/global skill roots, parser rules, collision behavior, symlink/path handling, provenance, reconciliation, or asset-refresh semantics;
- change the habit JSON format or migrate existing records;
- rewrite M001/M002/M003 closure records;
- suppress `clippy::suspicious_open_options` globally or locally merely to make CI green;
- weaken `-D warnings`, remove workspace/all-target Clippy, pin an older Rust toolchain solely to avoid the lint, or add another CI workflow;
- fix unrelated static-guard findings such as the project-catalog layout expectation or the existing review-tool broker boundary unless new evidence proves they are directly caused by this corrective change.

## 5. Invariants that must not regress

- habit candidates remain a separate host-owned structural evidence store, not ambient text memory;
- only successful logical operations increase habit confidence;
- automatic habit evidence remains bounded and excludes raw command/tool payloads;
- readiness remains at least three successful occurrences across at least two distinct sessions by default, with a hard multi-session floor;
- per-project habit mutation remains protected by advisory locking;
- each mutation reads the current complete file while holding the project lock, writes a complete bounded replacement through the existing temp-file/durability path, and atomically renames it;
- lock-file handling must not truncate or rewrite the synchronization file before advisory ownership is acquired;
- skill publication remains explicit-user-only and model-inaccessible;
- no storage/protocol/public compatibility change is introduced;
- verification remains the repository's existing local commands plus existing hosted CI lane.

## 6. Required production changes

### 6.1 Make the habit lock-file open policy explicit

Inspect both advisory lock creation sites in `crates/codegg-core/src/memory/habit.rs` before editing. They currently serve the `observe` path and host-only candidate transition path.

The intended policy is:

```text
absent lock file -> create synchronization inode
existing lock file -> open it without truncation
then -> acquire flock
```

Express that explicitly with `.truncate(false)` or a small private helper equivalent to:

```rust
fn open_lock_file(path: &Path) -> io::Result<File> {
    OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(false)
        .open(path)
}
```

Preferred implementation:

- use one private helper if it eliminates duplicated lock-open policy without obscuring the call sites;
- otherwise add `.truncate(false)` at both sites directly.

Do not use `.truncate(true)` as the default fix. The lock file's content is not application payload and truncating it is unnecessary; more importantly, truncation is a filesystem mutation performed before `flock_lock` establishes advisory ownership.

Do not change the temp-file write path used for actual habit JSON persistence unless an independent defect is discovered.

### 6.2 Preserve existing lock lifecycle

Keep current lock acquisition/release and error behavior:

```text
open/create lock inode
-> flock_lock
-> load current bounded state
-> mutate candidate state
-> save complete state atomically
-> flock_unlock / RAII close
```

If implementation introduces a helper, it must not silently move locking, broaden lock scope, change lock paths, or change file permissions unless required by existing repository convention and documented as a material deviation.

### 6.3 Do not paper over the lint

Forbidden fixes include:

- `#[allow(clippy::suspicious_open_options)]` on these paths without a demonstrated semantic reason;
- workspace lint-level changes;
- CI exclusions;
- toolchain downgrades/pins whose purpose is only to hide the lint.

The code should state its actual intended open semantics and compile cleanly under the repository's current stable toolchain policy.

## 7. Storage, protocol, migration, and compatibility

No storage migration is expected or permitted for the normal corrective path.

Existing habit files remain byte-compatible. Existing lock-file paths remain unchanged. Existing proposal and publication files remain unchanged.

No protocol/TUI command behavior changes are required. Documentation updates should be limited to the implementation plan/closure evidence unless production behavior materially deviates from the architecture already documented.

No ADR is required because filesystem ownership, persistence authority, skill publication authority, scheduler authority, and compatibility contracts remain unchanged.

## 8. Ordered work packages

### WP A — Confirm exact failing sites and intended semantics

Before editing:

1. inspect both `OpenOptions` sites and their surrounding lock lifecycle;
2. confirm they are synchronization lock files rather than data payload files;
3. confirm no code relies on lock-file truncation or contents;
4. inspect existing memory/file-backed lock helpers for a reusable local convention without broad refactoring.

Stop if either site has materially different semantics from the evidence above.

### WP B — Apply the minimum semantic correction

Implement explicit non-truncating open behavior at both sites, preferably through one private helper if that reduces duplication.

Acceptance evidence:

- no lock path changes;
- no habit serialization changes;
- no broad filesystem abstraction introduced;
- no lint suppression added.

### WP C — Run focused behavior regression checks

Confirm that the lock-policy edit did not change owning behavior:

- deterministic habit normalization/readiness tests remain green;
- concurrent habit writers remain green;
- text-memory compatibility remains green;
- proposal/publication integration remains green.

Do not add a test that merely re-asserts standard-library `OpenOptions` syntax. Existing persistence/concurrency tests are the behavioral regression layer; Clippy is the direct static recurrence guard.

### WP D — Re-establish exact-head hosted closure

Run the exact workspace Clippy command locally on the final candidate, then the repository quick gate. Push the final candidate and require normal GitHub Actions CI for that exact SHA to reach and pass Workspace tests.

The closure record must cite:

- failed predecessor head `4ea4eaa000ecf65b0e70ed7278cf071a57cf2197`;
- failed run `33813852632` / job `100841494152`;
- exact accepted corrective head;
- accepted hosted run/job;
- whether any warning appeared outside M004 scope.

## 9. Required verification

Run only the focused commands needed for this defect and the repository's existing closure gate:

```bash
cargo fmt --all -- --check
git diff --check
cargo test -p codegg-core habit --locked
cargo test -p codegg-core memory --locked
cargo test --test habit_skill_promotion --test skill_publication --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
scripts/verify.sh quick
```

If `scripts/verify.sh quick` already includes one of the broad commands, duplicate execution is acceptable only where needed to obtain direct, attributable evidence for the exact Clippy failure. Do not add all-features, live-model, LSP-real-server, benchmark, coverage, sanitizer, or release tests for this pass.

Hosted closure requirement:

- normal existing `CI / verify` push run on the exact final candidate;
- Workspace Clippy passes;
- Workspace tests execute and pass;
- no new CI lane is created.

## 10. Regression guard

The recurrence guard is intentionally existing infrastructure:

```bash
cargo clippy --workspace --all-targets --locked -- -D warnings
```

plus normal hosted execution of that command.

The two affected call sites should also preferably share one local helper so future habit-store transitions do not copy an ambiguous `OpenOptions` pattern.

Do not create a repository-wide source grep for `.create(true)`; many legitimate file-open modes exist and such a guard would be noisy and overbroad.

## 11. Failure and contention semantics

The correction must not change runtime contention semantics.

- failure to open/create the lock file remains an I/O error;
- failure to acquire `flock` remains an I/O error through the current path;
- failed habit load/mutation/save releases the advisory lock through the existing cleanup behavior;
- concurrent writers continue to serialize on the same project lock file;
- no lock file contents become authoritative state;
- process crash semantics remain those of the existing advisory-lock implementation.

## 12. Security and privacy review

No new sensitive data should be introduced. The implementation must not add logging of lock paths beyond existing diagnostics, and must not add habit payload content to CI/closure evidence.

The privacy boundary from M001 remains unchanged: shell/terminal actions persist only `ShellExec`; raw command text, executable/argv, arbitrary JSON, outputs, prompts, environment data, and hidden reasoning remain excluded.

The user-approval boundary from M002/M003 remains unchanged: model output cannot approve or publish a skill.

## 13. Documentation and registry updates

During implementation/closure:

- keep `plans/closure/memory-skill-promotion/001-status.md`, `002-status.md`, and `003-status.md` immutable;
- create `plans/closure/memory-skill-promotion/004-status.md` for the corrective result;
- update `plans/registry.md` from M004 `ready -> active/closing -> closed` as work progresses;
- when M004 closes, mark the corrective addendum closed and restore the memory-to-skill subsystem's current registry disposition to `closed at M004`;
- retain the failed `4ea4eaa` hosted evidence explicitly rather than deleting or reclassifying it from history.

No architecture documentation update is required if the production change is exactly the explicit non-truncating lock policy already implied by current behavior. If behavior changes beyond that, update `architecture/memory.md` and record the deviation before closure.

## 14. Acceptance criteria

M004 is complete only when all are true:

1. both M001-owned advisory lock opens have explicit non-truncating semantics;
2. there is no lint suppression, CI weakening, or toolchain downgrade used to obtain green status;
3. habit focused tests pass, including concurrent-writer coverage;
4. memory compatibility tests pass;
5. directly related habit-skill proposal/publication integration tests pass;
6. exact local workspace/all-target Clippy passes with `-D warnings`;
7. `scripts/verify.sh quick` passes;
8. normal hosted `CI / verify` passes on the exact accepted candidate through Workspace tests;
9. no critical/high/medium M004 finding remains;
10. `plans/closure/memory-skill-promotion/004-status.md` records requirement-to-evidence mapping and both failed predecessor and green replacement hosted evidence;
11. registry/addendum status is reconciled only after the exact-head evidence exists.

## 15. Stop conditions

Stop and register a separate follow-up instead of widening M004 if:

- the lock files contain application-significant data or require truncation semantics for correctness;
- correcting the warning requires changing lock ownership, path identity, file format, durability guarantees, proposal/publication authority, or another architectural boundary;
- a new hosted failure appears outside memory-to-skill scope and cannot be attributed to this candidate;
- a Rust/toolchain compatibility decision larger than these two call sites is required;
- closure would require changing CI architecture rather than fixing production code.

If a new unrelated workspace warning blocks hosted CI, classify it explicitly and assign it to the correct existing subsystem/corrective owner. Do not opportunistically absorb unrelated cleanup merely to make M004 appear closed.

## 16. Required closure evidence

Create `plans/closure/memory-skill-promotion/004-status.md` containing:

- status and recommendation;
- implementation commit(s);
- exact files changed;
- explanation of intended lock-file open semantics;
- proof that both ambiguous `OpenOptions` sites were corrected;
- requirement-to-evidence matrix;
- focused test outcomes;
- local workspace Clippy result;
- `scripts/verify.sh quick` result;
- failed predecessor hosted run `33813852632` / job `100841494152` on `4ea4eaa`;
- exact accepted corrective SHA and green hosted run/job through Workspace tests;
- storage/protocol/security/compatibility disposition;
- unresolved findings with severity;
- explicit statement that M001-M003 historical closure records were not rewritten;
- downstream dependency audit and final registry disposition.

Strict closure recommendation is allowed only after the exact accepted candidate has green hosted evidence.
