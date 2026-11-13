# Harness Practice Adoption Milestone 007 — Escaped-Defect Postmortems

Status: closed

Repository baseline: `7642b9e381d9c91d650fa375be2a7aa0aae9bcd8`
(branch `feat/auto-20261109-f8abad0d`)

Source roadmap:

- `plans/subsystems/harness-practice-adoption-roadmap.md` §7 M007, §9, §10 D4

Long-term requirements:

- `plans/000-long-term-specification.md#29-system-invariants`
- `plans/003-planning-process.md` — closure evidence and the guard rule
- `architecture/testing.md` — the existing statement that a guard only guards if
  the regression fails it

Applicable ADRs:

- **None.** Documentation and a guard over documentation.

Primary class: polish

## 1. Objective

Every defect that **escaped to a release** has a dated record naming the guard
that now prevents it, and a guard fails when that record names something that
does not exist — so the reason a guard exists survives the person who added it.

## 2. Why this milestone is ready

- No hard dependency.
- The guard suite is already the repository's evidence mechanism, and every
  guard added in this line and before it carries `--self-test`.
- `plans/closure/` establishes the record format this milestone deliberately
  does **not** duplicate.
- The line's own M001–M006 add guards whose rationale is exactly what a
  postmortem preserves.

## 3. Current implementation evidence

**Closure records are forward-looking.** `plans/closure/<subsystem>/NNN-status.md`
records, for one milestone, what was built and what evidence proves it. It
answers "did this milestone land?", not "why does this guard exist?".

**Scratch findings are not durable.** `agentfindings.md` is git-ignored local
harness notes. It is not a record and cannot be cited.

**Guards carry their rationale inconsistently.** Some guards open with a
paragraph explaining the defect they prevent; others begin with rules. The
existing comment budget bounds how much a guard may say, so a long rationale in
a guard is not the right home either.

**There is no defect memory.** Nothing records, for a defect that shipped, the
reproduction, the reason the evidence that existed at the time did not catch it,
and the guard that now does.

## 4. Invariants that must not regress

- **A record names a guard that exists.** The named path resolves and the guard
  carries `--self-test`. A record that names a test instead of a guard is
  allowed only when it names the test's full family/member path, and the guard
  verifies the test exists through the Rust-aware scanner.
- **A record is not a plan.** It does not schedule work, own a milestone, or
  appear in `plans/registry.md`.
- **A record is not a closure record.** It does not restate milestone evidence.
  §16 states the boundary in one sentence the README repeats.
- **Numbering is unique and monotonic.** A number is never reused.
- **A record is written after the fix, not before.** It names the guard that
  exists. An aspirational record is the failure mode this milestone removes.
- **The guard is cheap.** Documentation checks run in `quick`; they must not
  compile anything.

## 5. Scope

### In scope

- `docs/postmortems/README.md` — the convention, the template, and the boundary
  against closure records.
- At least one seeded record from a defect that was already fixed and already
  has a guard.
- `scripts/check_postmortems.py` with `--self-test`.
- Registration in `scripts/verify.sh quick` and the CI `verify` job.

### Explicitly out of scope

- A record per internal defect. §10 D4 of the roadmap: escaped defects only.
- Rewriting historical defects that have no guard. A record with no guard would
  violate §4; those stay in closure records.
- A template generator or a CLI command.
- Any change to `plans/closure/` or to the guard suite's structure.
- A retroactive numbering scheme over Git history.

## 6. Required production changes

None. This milestone touches `docs/`, `scripts/`, and the verify entry points.

### Documentation and static guards

- `docs/postmortems/README.md`: the rule, the required sections, the naming
  scheme, the boundary against closure records, and the review expectation that
  a fix which escapes gets a record in the same change that adds its guard.
- `docs/postmortems/NNNN-<slug>.md`: the seeded record.
- `scripts/check_postmortems.py` and its `--self-test`.
- `scripts/verify.sh` and `.github/workflows/ci.yml`: register the guard.

## 7. Ordered work packages

### Work package A — The convention

Intent: define the record before writing one, so the seeded record is an
instance rather than the definition.

Required changes:

- Required sections: `Status`, `Date`, `Symptom`, `Reproduction`, `Root cause`,
  `Why the existing evidence missed it`, `Guard added`, `Blast radius`.
- `Guard added` names at least one path under `scripts/` or a test path, and for
  each guard states its self-test injection in one line.
- Naming: `NNNN-<kebab-slug>.md`, four digits, monotonic, never reused.
- The boundary sentence: *a closure record says what a milestone built; a
  postmortem says what escaped and what now prevents it.*

Acceptance evidence:

- The README's template and the seeded record agree section-for-section.

### Work package B — The seeded record

Intent: prove the convention on a real defect rather than on a hypothetical one.

Required changes:

- Select a defect that (a) escaped to a release or a merged branch, (b) was
  fixed, and (c) has a guard that exists and self-tests **today**. Verify the
  guard before naming it; a named guard that does not self-test fails the
  guard this milestone adds.
- Write the record from the closure record and the commit that fixed it. Do not
  invent a root cause: if the closure record does not state one, say what the
  fix proves and what remains unexplained.

Acceptance evidence:

- The record's named guard exists and its `--self-test` rejects its injection.
- The record's reproduction is a command or a test path, not prose.

### Work package C — The guard

Intent: the convention cannot rot.

Required changes:

- `scripts/check_postmortems.py` verifies: every record has every required
  section; numbering is unique and monotonic; every `scripts/check_*` path named
  resolves; every named guard contains `--self-test`; every named test path
  resolves through `scripts/rust_test_scan.py`'s inventory; and no record is
  empty.
- `--self-test` injects, in a temporary tree: a missing section, a dangling
  guard path, a guard without `--self-test`, a duplicated number, and a named
  test that does not exist — and proves each is rejected.
- Registered in `scripts/verify.sh quick` and in the CI `verify` job, because a
  documentation defect fails silently.

Acceptance evidence:

- All five injections are rejected.
- The guard is green on the seeded record.
- The guard runs in `quick` without invoking `cargo`.

## 8. Failure, cancellation, restart, and contention semantics

- **A record names a guard that was later deleted.** The guard fails. The
  resolution is to name the replacement, not to delete the record.
- **A record's number collides.** The guard fails; renumbering an existing record
  is allowed only before it is committed.
- **A guard loses `--self-test`.** The postmortem guard fails, which is the
  intended coupling: removing the self-test makes the record's claim false.
- **Restart, cancellation, contention.** Not applicable.

## 9. Compatibility and migration

- Documentation and a guard. No production surface.
- Existing closure records are unchanged and remain the milestone evidence.
- The guard is additive to `quick` and CI; it adds no build cost.

## 10. Required tests

### Focused unit tests

Not applicable: the guard is a Python script and its `--self-test` is the test.

### Integration tests

- The guard is green on the committed record and rejects all five injections.

### Restart and recovery tests

- Not applicable.

### Contention and cancellation tests

- Not applicable.

### Security and negative tests

- A record containing a credential-shaped string fails the guard. A postmortem
  quotes a real defect; it must not quote a secret.

### Migration and compatibility tests

- A record naming a deleted guard fails; the guard's message names the record and
  the path.

## 11. Required verification commands

```bash
python3 scripts/check_postmortems.py
python3 scripts/check_postmortems.py --self-test

# the named guard's own self-test, as the record claims
python3 scripts/check_<named>.py --self-test

# the routine set
scripts/verify.sh quick
```

The closure record states which commands ran, and states the named guard's
self-test result explicitly — that result is the record's evidence.

## 12. Documentation updates

- `docs/postmortems/README.md` — new.
- `docs/postmortems/NNNN-<slug>.md` — the seeded record.
- `architecture/testing.md` — one paragraph: a defect that escapes gets a
  postmortem naming its guard, and the postmortem guard enforces that.
- `plans/subsystems/harness-practice-adoption-roadmap.md` §12 — status only.

## 13. Acceptance criteria

1. A seeded postmortem exists for a defect that was fixed, with a reproduction
   that is a command or a test path.
2. The record's named guard exists and its `--self-test` rejects its injection,
   shown in the closure record.
3. `scripts/check_postmortems.py` rejects all five of its self-test injections.
4. The guard is registered in `scripts/verify.sh quick` and in CI, and adds no
   `cargo` invocation.
5. No record appears in `plans/registry.md`, and no closure record was modified.

## 14. Stop conditions

The agent must stop and report rather than improvise when:

- the chosen defect has no guard that exists and self-tests — pick another
  defect; do not name a guard that does not yet exist;
- the root cause cannot be established from the closure record and the fix
  commit — write what the fix proves and say what is unexplained, rather than
  inventing a narrative;
- the guard would need to parse Rust beyond the existing scanner — use
  `scripts/rust_test_scan.py`, and do not write a second parser;
- the convention would require changing `plans/closure/` or the registry.

## 15. Closure evidence required

- The seeded record's path and its eight sections.
- The named guard's `--self-test` output showing its injection rejected.
- The postmortem guard's `--self-test` output with all five injections rejected.
- The `verify.sh quick` result with the guard registered.
- A statement that no closure record and no registry row was modified.

## 16. Handoff notes

- **Escaped defects only.** A record per internal defect is a tax that will be
  paid in low-quality records.
- **Name a guard that exists.** This is the whole difference between a
  postmortem and a resolution.
- **Do not restate the milestone.** The closure record already says what was
  built; the postmortem says what got past it.
- **Quote no secrets.** A reproduction quotes a command; a command can carry a
  token. Redact it.
- **The coupling is deliberate.** A record whose guard loses `--self-test`
  fails the build. That is the mechanism, not an inconvenience.
