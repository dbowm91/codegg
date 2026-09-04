# Memory-to-Skill Promotion Hosted Verification Corrective Addendum

Status: ready

Repository baseline reviewed: `4ea4eaa000ecf65b0e70ed7278cf071a57cf2197`

Source subsystem roadmap:

- `plans/subsystems/memory-skill-promotion-roadmap.md`

Historical milestones and closure evidence retained by this addendum:

- M001 plan: `plans/implementation/memory-skill-promotion/001-habit-observation-and-candidate-store.md`
- M001 closure: `plans/closure/memory-skill-promotion/001-status.md`
- M002 closure: `plans/closure/memory-skill-promotion/002-status.md`
- M003 plan: `plans/implementation/memory-skill-promotion/003-approved-skill-publication-and-refresh.md`
- M003 closure: `plans/closure/memory-skill-promotion/003-status.md`

Corrective implementation plan:

- `plans/implementation/memory-skill-promotion/004-habit-lock-open-options-and-hosted-closure-corrective-pass.md`

Long-term and planning references:

- `plans/000-long-term-specification.md#26-reliability-and-recovery`
- `plans/000-long-term-specification.md#27-security-requirements`
- `plans/000-long-term-specification.md#29-system-invariants`
- `plans/001-terminology-and-domain-model.md`
- `plans/003-planning-process.md#7-corrective-passes`

Primary class: corrective reliability / verification / closure

## 1. Purpose

Preserve the accepted memory-to-skill architecture while correcting one post-closure verification defect introduced by M001 and restoring truthful exact-head hosted closure.

The feature behavior itself remains the M001-M003 design:

- automatic habit observation retains only bounded structural metadata;
- readiness remains multi-session and host-owned;
- model-assisted drafting remains explicitly user-triggered;
- the model-facing proposal surface cannot publish;
- publication remains a user-only host/TUI operation into CodeGG-owned roots;
- existing skill parsing, collision/path safety, provenance, reconciliation, and asset refresh remain authoritative.

M004 must not reopen those capability boundaries without evidence.

## 2. Post-closure evidence requiring correction

Exact `main` revision `4ea4eaa000ecf65b0e70ed7278cf071a57cf2197` triggered GitHub Actions run `33813852632`, job `100841494152`. The hosted Ubuntu runner installed Rust `1.98.1` and failed at:

```bash
cargo clippy --workspace --all-targets --locked -- -D warnings
```

before Workspace tests executed.

The two reported errors are both `clippy::suspicious_open_options` in:

```text
crates/codegg-core/src/memory/habit.rs:408
crates/codegg-core/src/memory/habit.rs:552
```

Both sites open the per-project advisory lock file with `create(true)` and `write(true)` but do not state whether an existing file should be truncated.

The affected code was introduced by M001 implementation revision `2f029d8dd7de49876cf6527c835e586bd3d46e3c`. Therefore the findings are pre-existing relative to M003 implementation, but they are not pre-existing relative to the memory-to-skill subsystem. M003 closure correctly recorded the warnings as observed evidence, but its classification of them as unrelated to the workstream is no longer the controlling disposition.

The historical M001 and M003 closure records must remain unchanged. This corrective addendum owns the later evidence rather than rewriting history.

## 3. Why prior verification missed it

M001 recorded focused tests, `cargo check`, and `scripts/verify.sh quick` as passing on its implementation environment. M003 likewise recorded successful focused verification and quick verification, while a standalone local Clippy invocation exposed these warnings and attributed them outside M003 scope.

The exact hosted Linux lane subsequently ran the repository's stronger workspace/all-target Clippy command under Rust 1.98.1 and failed before tests. The gap is therefore not a missing feature test; it is a mismatch between the closure disposition and the repository's actual exact-head hosted verification result.

The corrective response is deliberately small:

- make the lock-file open policy explicit;
- use the existing workspace Clippy command as the recurrence guard;
- obtain one green hosted run on the exact corrective candidate;
- do not add another CI lane, scanner, benchmark, or verification framework.

## 4. Corrective ownership boundary

M004 owns only:

- the two ambiguous advisory-lock `OpenOptions` sites in `memory/habit.rs`;
- any very small same-module helper needed to express the intended non-truncating lock-file policy once;
- focused regression verification for habit persistence/promotion compatibility;
- exact-head hosted CI evidence and final registry/closure reconciliation.

M004 does not own:

- habit fingerprint vocabulary or privacy policy;
- readiness thresholds or candidate lifecycle redesign;
- proposal schema or model prompting;
- skill publication paths, permission/approval rules, parser behavior, collision semantics, or asset refresh;
- generic filesystem helper refactors across the repository;
- unrelated project-catalog/tool-broker static-guard findings already documented elsewhere;
- CI workflow expansion or toolchain pinning merely to avoid a warning.

## 5. Required invariant

Advisory lock files are synchronization artifacts, not persisted habit payloads. Opening an existing lock file must not implicitly truncate or otherwise rewrite it before acquiring the advisory lock.

The preferred semantic correction is therefore an explicit non-truncating open, for example:

```rust
OpenOptions::new()
    .create(true)
    .write(true)
    .truncate(false)
    .open(lock_path)?
```

A small private helper is preferable if it removes the duplicated policy cleanly. `truncate(true)` is not the default corrective choice because truncating the synchronization inode is unnecessary and would introduce a write side effect before `flock_lock` establishes ownership.

## 6. Corrective dependency graph

```text
M001 habit observation/candidate store -------- closed historical evidence
M002 proposal/preview ------------------------- closed historical evidence
M003 publication/refresh ---------------------- closed historical evidence
                         |
                         v
M004 lock-open policy + exact-head hosted closure
```

M004 has no hard external dependency beyond current `main`. It is dependency-ready now.

Agent convergence is independent and remains closed. The unrelated supported-Linux Landlock evidence condition remains independent.

## 7. Milestone M004 — Habit lock-open policy and hosted closure

Status: ready

Plan:

- `plans/implementation/memory-skill-promotion/004-habit-lock-open-options-and-hosted-closure-corrective-pass.md`

Objective:

Remove the two M001-owned workspace Clippy failures without changing habit-store semantics, then establish green exact-head hosted evidence through Workspace tests.

Exit conditions:

- both advisory-lock opens state explicit non-truncating behavior, directly or through one private helper;
- no Clippy suppression or CI weakening is used;
- habit persistence/concurrency tests remain green;
- proposal/publication compatibility tests remain green;
- `cargo clippy --workspace --all-targets --locked -- -D warnings` passes locally on the final candidate;
- `scripts/verify.sh quick` passes;
- normal push CI for the exact final candidate passes through Workspace tests;
- `plans/closure/memory-skill-promotion/004-status.md` records the failed predecessor run and accepted replacement run/job;
- registry returns the memory-to-skill subsystem to `closed` only after that closure evidence exists.

## 8. ADR and compatibility disposition

No ADR is required. M004 does not change scheduler authority, authorization, asset ownership, storage format, protocol schemas, skill precedence, or any public compatibility contract.

If implementation discovers that correcting the warnings requires changing lock ownership, persistence format, filesystem durability semantics, or publication authority, stop M004 and register a separately justified architectural follow-up rather than widening this pass.

## 9. Verification posture

Verification must remain proportional to the defect. Do not add new CI workflows or broad test matrices.

Required local evidence is the owning habit tests, the directly related promotion/publication integration tests, the exact workspace Clippy command that failed in hosted CI, formatting/diff checks, and `scripts/verify.sh quick`. The final closure additionally requires the existing hosted CI lane to pass on the exact candidate.

A new unit test whose only purpose is to assert Rust `OpenOptions::truncate(false)` syntax is not required; the existing habit-store concurrency/persistence tests verify behavior, while workspace Clippy is the direct regression guard for the discovered defect.
