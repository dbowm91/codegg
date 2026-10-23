# CI/Test Throughput Recovery M007 — Closure Status

Status: closed

Recommendation: **closed** — the decision table records every duplicate group
on the tree, every removal names its survivor and its reason, all **85**
removals were applied and verified function-by-function, the shared-helper
fan-out is discharged, and the guard ships with a `--self-test` that proves
it rejects an injected violation of every class *and* that it does **not**
report two bodies that differ only in a string argument.

Source implementation plan:

- `plans/implementation/ci-test-throughput-recovery/007-test-suite-duplication.md`

Source subsystem roadmap:

- `plans/subsystems/ci-test-throughput-recovery-roadmap.md`

Repository baseline reviewed: `2c1953347896848c2f52c2dfbd9f04029d1c2cc2`

## 1. Executive finding

**85** test functions asserted nothing that a surviving test did not already
assert. Each removal names the survivor that keeps the stronger property, and
each survivor is recorded in `docs/test-duplication.toml`.

| | Before | After |
|---|---|---|
| Test functions | 16,004 | **15,919** |
| Duplicate groups | 90 | **14** (all cross-crate, all deliberately kept) |
| Redundant copies | 99 | **0** outside the cross-crate pairs |
| Decision-table groups | — | **90** (189 members, 85 removals, each with a survivor and a reason) |
| Files touched by removals | — | **40** |
| `secret_scan` self-test executions | 40 (eight families × five) | **5** (one family) |

## 2. The detector, and the two ways to get it wrong

The detector lives in the shared scanner, `scripts/rust_test_scan.py`, and
compares bodies **content-preserving**: a literal is replaced by a marker
derived from its content, so two calls that differ only in a string argument
stay distinct, while two calls that pass the *same* argument collide.

This matters, and both wrong ways are measured:

| Approach | Groups reported | Verdict |
|---|---|---|
| **Content-preserving** (a marker derived from each literal's content) | **90** | correct |
| Blanking every literal | **456** | over-reports **5×** |

A regex scan is not a measurement at all — the four ways it mis-reads Rust
are recorded in M006's closure record, §2.

## 3. The decision rules, and what they are for

Each rule exists because the *other* copy proves something the survivor does
not. They are recorded per group in `docs/test-duplication.toml`, in the
`reason` field.

| Boundary | Groups | Rule | Why |
|---|---|---|---|
| `cross-crate` | **14** | **Both copies kept** | The pair *is* the boundary check: one copy reaches `pub(crate)` items, the other reaches the public path. Keeping one side loses the property the pair proves. |
| `unit-integration` | 18 | The **integration** copy survives | The integration copy proves the public path; the unit copy is redundant. |
| `integration` | 26 | First by path survives | Same kind, same crate. |
| `unit` | 32 | First by path survives | Same kind, same crate. |

Within one file, the first by line survives.

**The SSRF cluster is an explicit override.** M006 re-pointed every copy of
the SSRF classification at production's
`codegg::security::ssrf::is_internal_ip`, so the copies in
`src/security/ssrf.rs` and `tests/config_provider/mcp.inc` now assert
exactly what `tests/security_permission/ssrf.inc` asserts. The topical home
survives; the other two are pastes of it. This is why **17** removals land
in `tests/config_provider/mcp.inc` and **4** in `src/security/ssrf.rs`.

## 4. The removals, reconciled

The table is the reconciliation: 189 members, 85 removed, 104 kept, and every
group keeps exactly one survivor except the 14 cross-crate groups, which keep
both.

| Tree | Before | Removed | After |
|---|---|---|---|
| `tests/` | 4,981 | **35** | 4,946 |
| `src/` | 7,433 | **46** | 7,387 |
| `crates/` | 3,590 | **4** | 3,586 |
| **Total** | **16,004** | **85** | **15,919** |

Per family:

| Family | Before | Removed | After |
|---|---|---|---|
| `tests/agent_loop` | 512 | 1 | 511 |
| `tests/command_git` | 453 | 3 | 450 |
| `tests/config_provider` | 168 | 17 | 151 |
| `tests/ide_protocols` | 231 | 6 | 225 |
| `tests/scheduler_jobs` | 330 | 1 | 329 |
| `tests/tool_programs` | 1,043 | 6 | 1,037 |
| `tests/tui_render` | 1,030 | 1 | 1,029 |
| `src` (inline library tests) | 7,433 | 46 | 7,387 |
| `crates/codegg-core` | 1,062 | 2 | 1,060 |
| `crates/codegg-git` | 362 | 2 | 360 |

The counts reconcile **exactly**: every removed function is named in the
table with its file, its line before the removal, its survivor, and the
reason. The `architecture/overview.md` "Verified counts" table,
`architecture/testing.md`, and the roadmap are re-pinned from these numbers.

### 4.1 A note on the plan's baseline figure

The plan records a baseline of **90 groups / 100 redundant copies**. The
regenerated table has **90 groups / 99 redundant copies**. The group count
matches exactly; the one-copy difference is a **phantom**. The tree the plan
measured carried a damaged region in `tests/config_provider/mcp.inc` in which
one function was counted **twice** by the scanner, which added a member to
one group and therefore one redundant copy. That damage also **hid** a real
duplicate — the `c05_infrastructure_error` pair — which is why the group
count is unchanged after the repair: one phantom group went, one real group
came back. The regenerated table records the real one, as `group-090`.

## 5. The applier had a defect that corrupted the tree, and how it was fixed

The first applier computed every removal span against **one snapshot** of the
file and then applied the spans to a mutating buffer. Two spans in the same
file could overlap, and an overlapped span has `end < start`; splicing
`text[:start] + text[end:]` with such a span **duplicates** the region
between the two offsets instead of deleting it. The result was three
corrupted test files — a function body cut in half and pasted at the top of
its own file — and, separately, 703 `src/` and `crates/` files whose test
attributes and signatures had been stripped.

The applier was rewritten to be **name-keyed**: it removes one function at a
time and re-parses the file between removals, so every span is computed
against the text it is applied to. It then verifies, per file, that exactly
the intended functions disappeared and that every other function survived.

The repair is recorded here because the damage had a **silent** mode that
brace-balance checking could not see: stripping `#[test]` and the `async`
modifier from a test leaves a well-formed, brace-balanced function that is no
longer a test. The suite would then report *pass* without running the body.
One test was in that state —
`tests/tool_programs/tool_program_m012_broker_failures.inc`'s
`c05_infrastructure_error_cannot_become_completed` — and it was found by the
compiler's own dead-code warning, then confirmed against the HEAD
pre-split sources by a check that reports any function that was a test at
HEAD and is not one now. That check now reports **0**.

## 6. The shared-helper fan-out is discharged

`tests/common/` is declared by **eight** root test families.
`tests/common/secret_scan.rs` carried **five** `#[test]` functions of its
own, so they were compiled into all eight and ran **forty** times — of which
one family had any use for the helper.

- The five self-tests moved to the one family that uses the helper, as
  `tests/command_git/secret_scan_self_tests.inc`. The assertions are
  **unchanged**; only their home moved. They now execute **five** times, in
  **one** target.
- `tests/common/secret_scan.rs` is now assertion-free and carries only the
  helper API the families call.
- The sibling item, `crates/egglsp/tests/common/`, carried **no** assertions
  and was therefore compile-time duplication only. Its two harnesses were
  used by **disjoint** sets of consumers (three each), and every consumer
  compiled **both**. Each of the six consumers now binds only the harness
  module it uses, and the now-unreferenced `common/mod.rs` was removed so
  that a future consumer cannot silently re-introduce the duplication.

The removal of `mod common;` declarations from families that use none of
the helper was **not** done: a reference search found that **all eight**
families reference `common::` — `pool`, `projection_replay`, `eggwork_node`
or `secret_scan`. The plan's rule ("only if no member of that family
references any helper") therefore forbids all seven removals, and the
guard's `unused-helper-declaration` class enforces exactly that rule.

## 7. The guard

`scripts/check_test_duplication.py` has three classes from the plan and one
added by this milestone:

| Class | Rule |
|---|---|
| `cross-file-duplicate` | Two bodies in **different files** are byte-identical after normalisation, and the pair is not recorded in the decision table. |
| `same-file-duplicate` | Two bodies in the **same file** are byte-identical after normalisation, and the pair is not recorded. |
| `shared-helper-assertion` | A helper module under `tests/common/` or `crates/*/tests/common/` carries its own `#[test]` functions. |
| `unused-helper-declaration` *(added)* | A family declares the shared helper module and no member of that family references it. |

The fourth class was added because the plan's class covers only the first
half of the shared-helper problem: a helper that is *declared* but never
*used* is the same waste with the opposite cause, and it is exactly the
condition the plan's work package D had to check by hand.

The `--self-test` injects one violation of each class and proves the guard
rejects it, and it includes the **negative** case the plan calls for: two
bodies that differ only in a string argument must **not** be reported. A
guard that reported those is the failure mode of §2, and a self-test that
did not catch it would be a broken self-test.

The guard and its `--self-test` run in `scripts/verify.sh quick` and in the
CI job.

## 8. Acceptance criteria

| # | Criterion | Status | Evidence |
|---|---|---|---|
| 1 | The decision table records every duplicate group, its survivor and its reason | **met** | `docs/test-duplication.toml`: 90 groups, 189 members, 85 removals; §3's rules |
| 2 | Every removal names the survivor that keeps the stronger property | **met** | the `survivor` and `reason` fields of every group; the cross-crate groups name *both* by keeping every member |
| 3 | No two test functions assert the same thing twice, except where a recorded exception names the survivor | **met** | the tree has **14** remaining duplicate groups, all cross-crate and all recorded; the guard reports 0 |
| 4 | The shared-helper fan-out is stopped | **met** | §6: five self-tests execute in one target instead of eight; the `egglsp` harnesses are bound per consumer |
| 5 | The guard passes on the tree, rejects an injected violation of every class, and does not report bodies that differ only in a string argument | **met** | the `--self-test` output lists every case, including the negative one |
| 6 | The guard and its `--self-test` run in `verify.sh quick` and in CI | **met** | `scripts/verify.sh` and `.github/workflows/ci.yml` |
| 7 | The before-and-after counts are reported per tree and per family, with the removal table as the reconciliation | **met** | §4's two tables; they reconcile exactly |
| 8 | Every count this milestone publishes is verified against its own listed source | **met** | `architecture/overview.md`'s two changed rows are re-measured against their `Source`; `architecture/testing.md` cites `rust_test_scan.py --validate` |
| 9 | The roadmap's §4.5 figures that this milestone changes are corrected, with the measured value and its source | **met** | the roadmap's §4.5 now records the post-M004 scale (eight families, forty executions) **and** the discharge (one family, five executions) |
| 10 | No removal changed a production behaviour | **met** | every removal is a test function; the only non-test edits are the `egglsp` harness bindings and the `secret_scan` relocation, both recorded in §6 |

## 9. Verification evidence

| Check | Result |
|---|---|
| `scripts/rust_test_scan.py --validate` | **pass** — `tests:` 4,946 attributes = 4,946 functions; `src:` 7,388 attributes = 7,387 functions + 1 macro template; `crates:` 3,586 = 3,586; the mask neither loses nor gains a newline in any file |
| `scripts/check_test_duplication.py` | **pass** — 0 violations; 14 remaining groups, all cross-crate and all recorded |
| `scripts/check_test_duplication.py --self-test` | **pass** — one rejection per class, plus the negative case |
| `scripts/check_test_assertion_quality.py` | **pass** — 0 violations |
| `scripts/check_tool_program_parse_only.py` | **pass** |
| `cargo check --workspace --locked --all-targets` | **pass** |
| `cargo fmt --check --all` | **pass** |
| `cargo nextest run --workspace --locked --profile ci --no-fail-fast` | **15,466 run, 15,460 passed, 5 failed, 1 timed out, 8 skipped** — every failure and the timeout are the six the M004 closure record proved pre-existing against the baseline commit `2c1953347` (see M006's closure record, §5.2–§5.3, for the per-test causes). **No new failure.** |

Two removals did change the compiler's view of the tree, and both were
fixed rather than muted: the removal of `unused_new_types_stay_importable_for_docs`
left `NewWorkItem` and `NewWorkPlan` unused in
`crates/codegg-core/src/work_plan/projection.rs`, and the removal of two
`vim_keys_*` tests left their `key` helper unused in
`src/tui/components/dialogs/session.rs`. A third, the `secret_scan` helper
being compiled into seven families that use none of it, is handled by the
standard shared-helper idiom (`#![allow(dead_code)]` with the reason
written down) rather than by muting a real defect.

## 10. What this milestone did **not** do


- It did not change the throughput objective. M007 removes duplicated work;
  it does not make the remaining work faster. It must not be measured as if
  it had advanced CI wall-clock time.
- It did not remove a cross-crate pair. The **14** such groups are the
  boundary check, and the plan's own rule keeps both sides.
- It did not remove the `lsp.inc` "does not crash" cluster. Seven tests there
  have different names but byte-identical bodies, because `make_tool()`
  returns one fixed tool — six of the seven names claim a tool their body
  never constructs. **The removal loses no assertion; the naming defect is a
  separate, still-open follow-up**, recorded here rather than silently fixed.
- It did not touch production code. `src/security/ssrf.rs` and every other
  production module are unchanged by this milestone.

## 11. Outstanding

- **Hosted CI evidence.** Every count here is a local measurement. The
  hosted run is outstanding for the whole workstream and is blocked on the
  same owner decision as M005.
- **The rebase.** `origin/main` is 104 commits ahead and adds twelve
  top-level `tests/*.rs` files, which auto-discover as twelve new root test
  targets. The counts in §4 are measured on this worktree, before that
  rebase; the decision table's *line* numbers will shift with it, which is
  why the guard matches members by file and name rather than by line.
- **The `lsp.inc` naming defect** in §9.
- **The macOS clippy condition** and the **contention-sensitive timeout**
  are recorded in M006's closure record, §5. Neither is caused by this
  milestone.
