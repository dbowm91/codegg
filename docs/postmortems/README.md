# Escaped-defect postmortems

A guard exists because something got past the evidence that was there at the
time. That reason is not in the guard: the comment budget bounds how much a
guard may say, and a guard's comment corpus is measured, so a long rationale in
a guard is not the right home for it. This directory is.

## The rule

**A defect that escapes gets a dated record here, in the same change that adds
the guard which prevents it.** Not before the guard exists, and not in a
follow-up: a record written ahead of its guard is a resolution, not a
postmortem, and naming a guard that does not exist is the failure mode this
directory exists to remove.

Escaped means **it got past the evidence that existed when it was written** —
the test suite, the guards, the review of the change that introduced it. That
is the whole population. A record per internal defect is a tax that gets paid
in low-quality records, so an ordinary defect found by a test it already had
does not belong here; it belongs in the change that fixed it.

Records are written **after** the fix. They name a guard that exists and
self-tests today, verified before it is named.

## Required sections

Exactly these eight `##` sections, in this order. No more, no fewer: a section
this convention does not have is a deliberate edit to
`scripts/check_postmortems.py`, not a side effect of one record.

<!-- template:start -->

```markdown
# NNNN — <one-line title>

## Status

Fixed.

## Date

YYYY-MM-DD

## Symptom

What a user or a consumer of the system observed. The error text if there was
one.

## Reproduction

A command or a test path, not prose. The minimal bytes or steps, in a fenced
block.

## Root cause

The mechanism, not the blame. Why the code did the wrong thing.

## Why the existing evidence missed it

The evidence that was there when the defect was written, and what it could not
see. Name the test that should have caught it and the property it did not
assert.

## Guard added

- Guard: `path/to/guard.py` or `path/to/file.rs::test_fn_name`
  Self-test: the injection that makes this entry fail, in one line.
  Must mention: `token-that-names-the-defect`

## Blast radius

What was wrong, in what, for how long. What was not wrong.
```

<!-- template:end -->

## Naming

`NNNN-<kebab-slug>.md` — four digits, a lowercase kebab-case slug, `.md`.

Numbers start at `0001` and are contiguous. A number is never reused: a record
that is deleted leaves a gap, and the gap is a failure, because the number it
consumed is a fact about the history. Renumbering an existing record is allowed
only before it is committed.

Every other `*.md` in this directory is a record. `README.md` is not; it is
this convention.

## The `Guard added` entry

One bullet per guard. Three fields, all required:

| field | meaning |
|---|---|
| `Guard:` | One backticked claim. Either a path under `scripts/` (a guard script) or `<repo path>.rs::<test fn>` (a test standing in as the guard). Nothing else is a guard. |
| `Self-test:` | One line: the injection that makes this entry fail. For a script guard that is the case its `--self-test` runs; for a test it is the defect re-introduced. A bare `n/a` is rejected. |
| `Must mention:` | One or more backticked tokens that name the defect class. Each token must appear in the named guard — in a script's text, or in the named test function's own body. |

`Must mention` is why this is not satisfiable by form alone. A record that
names a plausible-looking guard path which has nothing to do with the defect
fails: the guard cannot have deleted every reference to the thing it prevents.

A test is accepted in place of a guard script only when the test is the guard —
it asserts the property as a contract and fails when the defect returns. Name
it as `<repo path>.rs::<test fn>`; the guard resolves the file, then asks
`scripts/rust_test_scan.py`'s inventory whether that function is defined in it.
No second Rust parser exists, and this guard does not add one.

## What the guard checks

`scripts/check_postmortems.py` runs in `scripts/verify.sh quick` and in the CI
`verify` job, because a documentation defect fails silently. It verifies:

- the README's template block lists exactly the eight declared sections, in
  order, and the boundary sentence is present;
- every record's name matches `NNNN-<kebab-slug>.md`;
- numbers are unique and contiguous from `0001`;
- every record has all eight sections, in order, and every section has a body;
- `Status` names a declared state, and `Date` is a parseable `YYYY-MM-DD`;
- `Reproduction` contains a fenced block or a backticked test path;
- every `Guard:` claim resolves, is a `scripts/` path or a `.rs::test` path, and
  carries `--self-test` when it is a script;
- every `Must mention:` token appears in the named guard or test body;
- no record carries a credential-shaped string;
- no record path appears in `plans/registry.md`.

It invokes no `cargo`. `--self-test` copies the tree into a temporary
directory, injects one defect of each class, and requires every one to be
rejected while the unmodified copy passes.

## The boundary against closure records

A closure record says what a milestone built; a postmortem says what escaped
and what now prevents it. `plans/closure/<subsystem>/NNN-status.md` answers
"did this milestone land?" and owns the evidence for that answer. A postmortem
does not restate milestone evidence, does not schedule work, does not own a
milestone, and does not appear in `plans/registry.md`.

A defect that has no guard stays in the closure record that found it. A record
here with no guard would be the exact thing this convention forbids.

## When a record's guard is deleted

The guard fails, naming the record and the path. The resolution is to name the
guard that replaced it, never to delete the record. The record is the only
place the reason survived the person who added the guard.

## Security

A postmortem quotes a real defect, so it quotes real output — and a
reproduction is a command, which can carry a token. Redact it. The guard
rejects a credential-shaped string; it does not reject a hardcoded personal
path, so redact that too, on the same principle.
