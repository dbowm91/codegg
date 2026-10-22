# CI and Test Throughput Recovery Milestone M007 — Remove Provably Duplicate Test Bodies

Status: ready for handoff

Repository baseline: `2c1953347896848c2f52c2dfbd9f04029d1c2cc2` (the committed tree the plan was authored against)

Implementation baseline: the `2c1953347` working tree **plus the uncommitted M001–M004 change set** (the tree this work will be applied to). See §16 for the rebase requirement this creates.

Source roadmap:

- `plans/subsystems/ci-test-throughput-recovery-roadmap.md#milestone-m007--remove-provably-duplicate-test-bodies`

Long-term requirements:

- `plans/000-long-term-specification.md` §4.7 — correctness before transparent magic;
- `plans/003-planning-process.md` §2.3–2.4.

Applicable ADRs: none required. This milestone changes **test files only** and adds a static guard. It does not alter product authority, protocol, storage, dependency selection, feature defaults, or runtime architecture.

Predecessor work (closed, historical, not reopened by this milestone):

- `plans/subsystems/ci-test-throughput-optimization-roadmap.md` M003–M005 — the predecessor consolidations;
- `plans/subsystems/ci-test-throughput-recovery-roadmap.md` M003 and M004 — the ownership manifest and the family topology this milestone works inside.

Primary class: infrastructure, with a polish obligation.

---

## 1. Objective

No two test functions in the repository assert the same thing twice — except where a recorded exception names the survivor — and a static guard proves the property.

Every removal this milestone makes is a removal of a **byte-identical assertion**, and every removal records the survivor that keeps the assertion. Nothing else is removed.

## 2. Why this milestone is ready

**Hard dependency: M006** (`plans/implementation/ci-test-throughput-recovery/006-test-suite-assertion-quality.md`). Two reasons, both structural:

1. M006 delivers the Rust-aware scanner this milestone's detector is built on. Writing a second scanner would reproduce the failure modes in §3.4 and produce a different, incompatible answer.
2. M006 removes the pasted copy of production SSRF logic and resolves the three empty test bodies. Left alone, M007's detector would flag those same lines and the two milestones would fight over them.

**Interface dependency: M003** (`plans/implementation/ci-test-throughput-recovery/003-change-scoped-verification-mode.md`). The ownership manifest maps source paths to groups and targets. This milestone removes test *functions*, not targets, so no group's membership changes; but where a member file becomes empty and is removed, the manifest's member list changes and must follow.

**Interface dependency: M004** (`plans/implementation/ci-test-throughput-recovery/004-root-test-target-family-consolidation.md`). The family topology determines which files this milestone edits and which target each surviving test belongs to.

**Why it is seventh:** M006 establishes that the assertions are true. Removing a duplicate before that is safe would remove copies of an assertion that is itself wrong — and the SSRF cluster shows exactly that case: one of its three copies asserts the opposite of production.

**Not blocked by, and not blocking, the hosted-evidence work.** The hosted-CI closure milestone (reserved as **M005**, not yet opened) is blocked on an owner decision about committing and pushing the M001–M004 change set. This milestone is independent of that decision.

## 3. Current implementation evidence

All figures were measured on the implementation baseline with a Rust-aware scanner that treats literals as opaque for structure and **content-preserving** for comparison. The method and its measured failure modes are in §3.4.

### 3.1 The scale of the duplication

| Measure | Count |
|---|---|
| Test functions scanned (`tests/` + `src/` + `crates/`) | **16,004** |
| Exact-duplicate groups (identical body, literal content included) | **90** |
| Redundant copies across those groups | **100** |
| Groups spanning more than one file | **62** |
| Redundant copies in the cross-file groups | **66** |
| Groups inside a single file | **28** |
| Redundant copies in the same-file groups | **34** |

This is a **smaller** number than the roadmap's §4.5 audit implied, and the reason it is smaller is the subject of §3.4. The honest headline is that the duplication is real but modest, and that most of what a naive scan reports as duplication is not duplication at all.

### 3.2 The SSRF cluster — the same eleven assertions, three times

The largest coherent cluster is the IP-classification assertions. **Twenty-two** groups carry a body that appears in more than one file:

| File pair | Groups |
|---|---|
| `tests/config_provider/mcp.inc` ↔ `tests/security_permission/ssrf.inc` | **16** |
| `src/security/ssrf.rs` ↔ `tests/config_provider/mcp.inc` | **3** |
| `src/security/ssrf.rs` ↔ `tests/security_permission/ssrf.inc` | **3** |

Nineteen of the twenty SSRF test functions in `tests/config_provider/mcp.inc` are byte-identical copies of a function in `tests/security_permission/ssrf.inc` and of one in `src/security/ssrf.rs`. The twentieth — `tests/config_provider/mcp.inc:192` `test_mcp_ipv6_not_ula` — is **not** a copy of anything, because it asserts the opposite of production. It is the drifted one, and M006 corrects it.

Examples: `src/security/ssrf.rs:199 test_is_internal_ip_private_class_a` ≡ `tests/security_permission/ssrf.inc:22 test_ipv4_10_range` ≡ `tests/config_provider/mcp.inc:80 test_mcp_ipv4_10_range`; and the same for `192.168/16`, `100.64/10`, `127.0.0.0/8`, `0.0.0.0/8`, `172.16/12`, `169.254/16`, `198.18/15`, the external case, and three IPv6 cases.

The three copies are not equivalent, and the difference is the whole point of the milestone:

- `tests/security_permission/ssrf.inc` asserts against **production** — it imports `codegg::security::ssrf::{is_internal_ip, validate_host_ip, validate_url_host}` at `:6`.
- `src/security/ssrf.rs` asserts against production from inside the module, so it can also reach `pub(crate)` items.
- `tests/config_provider/mcp.inc` asserts against a **pasted copy** of production (`mcp.inc:33`), and the copy has drifted — see M006 §3.1. **M006 fixes this copy**; after M006 the cluster is two real copies plus one that has been re-pointed at production, i.e. three copies of the same assertion.

M006 also resolves the second three-way cluster, whose members are the three **empty** test bodies: `tests/tool_programs/tool_program_m012_hosted_status.inc:67 c30_all_m012_tests_compile_and_run`, `crates/codegg-core/src/tool_program/guards.rs:89 no_cpython_import_in_module`, and `crates/egglsp/tests/real_server_smoke.rs:8076 check_name_advertised_is_removed`. All three have the body `{ }`.

### 3.3 The shared-helper fan-out

`tests/common/` holds five helper modules and is declared by **eight** families: `agent_loop`, `command_git`, `core_daemon`, `projection_replay`, `scheduler_jobs`, `server_feature`, `session_family`, `tool_programs`.

`tests/common/secret_scan.rs` carries **five** `#[test]` functions of its own — `:98`, `:105`, `:112`, `:127`, `:135`. Because the module is compiled into all eight families, those five self-tests run **40** times. Only **one** family — `command_git`, whose five member files reference `secret_scan` — has any use for the helper. The other seven compile the module and its five assertions for nothing.

This is the roadmap's §4.5 item 4 (*"a shared helper's five self-tests compiled into 46 binaries"*) at its post-M004 scale: 46 became 8, 230 redundant executions became 40.

`crates/egglsp/tests/common/` is the same shape with a different remedy: it compiles into six `egglsp` binaries and carries **no** assertions at all, so it is compile-time duplication only.

### 3.4 The detector problem, measured — and why the numbers in §3.1 are small

A duplicate detector over Rust source has to survive five things. **Five** of them were measured while preparing this plan, and each one produced a wrong answer:

1. **Blank literals and the comparison collapses.** If a string literal is replaced by a placeholder before bodies are compared, then `classify_command("find")` and `classify_command("brew")` become the same text. Measured on this tree, that normaliser reported **456** duplicate groups and **1,084** redundant copies, with a spurious "49-copy group" spanning `src/command_intent/mod.rs` and two `command_git` family members. Every one of those 49 was a *different command* tested by the same shape of assertion. **The detector must preserve literal content** — each literal replaced by a marker derived from its content, so equal arguments stay equal and different arguments stay different. With that change the count drops to the 90 groups of §3.1.
2. **Lifetimes read as character literals.** A regex of the form `'(?:\\.|[^'\\])'` matches `&'a T` and swallows the code up to the next apostrophe. A scanner built this way found **1,204** of the **4,978** test functions in `tests/`.
3. **A raw-string pattern without the `r` prefix matches a closing quote.** A pattern such as `r?#*"(?:[^"]|"(?!#))*"` starts at the closing quote of a plain string, because a word character precedes it, and consumes the following function header — seven functions in `tests/security_permission/ssrf.inc` alone.
4. **`#[tokio::test]` is followed by `async fn`.** A pattern requiring `fn` immediately after the attribute matches no asynchronous test at all — 2,371 in `tests/`.
5. **A multi-line string literal silently moves every line number after it.** If the mask used to find a test body replaces a literal's contents without keeping the newlines *inside* that literal, a multi-line raw string collapses its own line structure and every subsequent line number in that file is wrong. Measured on this tree: **298 of 1,349** files contain a multi-line string literal — a raw `r#"…"#` config fixture in `tests/config_provider/config_integration.inc`, an `r##"…"##` shell script in `tests/config_provider/eggsearch_pair_compat.inc`, a multi-line `panic!` message in `tests/eggwork_remote_execution_live/eggwork_remote_execution_live.inc` — and in each of them the reported line number drifted by up to 226 lines. **The duplicate counts stayed correct; only the line numbers moved.** That is the more dangerous failure for this milestone, because the removal table is built from line numbers: a shifted line removes the wrong function.

A scanner handling raw strings in all hash-count forms, byte strings, character literals as distinct from lifetimes, Rust's nested block comments, and `async fn` reproduces the ground-truth attribute counts **exactly** on `tests/` (4,978) and on `crates/` (3,593), and to within 2 on `src/` (7,433 of 7,435). It must additionally be **line-preserving** — the mask it uses must keep the original's newline count — and that property must be checked per file rather than assumed.

**The consequence for this milestone:** the detector is the deliverable as much as the removals are. A guard built on a scanner with any of these five defects will either report thousands of false duplicates, miss real ones, or remove the wrong function — and a guard that reports false positives gets switched off.

### 3.5 What "duplicate" does not mean here

A cross-file duplicate is not automatically redundant. Two copies of an identical body can carry different *reach*:

- A **unit** test inside `src/` may reach `pub(crate)` items; an **integration** test under `tests/` may only reach `pub` items.
- A **cross-crate** pair — `src/context/artifact.rs:295 test_estimate_tokens_empty` and `crates/eggcontext/src/lib.rs:322 estimate_empty_text` — tests the same behaviour from both sides of a crate boundary. Keeping only one side loses the boundary check.

So the survivor is chosen by the **stronger property**, not by convenience: where a unit body and an integration body are identical and the body touches only `pub` items, the **integration** copy survives because it proves the public path; the unit copy is then redundant. Where the body touches `pub(crate)` items, the **unit** copy survives and the integration copy cannot exist at all — which is why such a pair cannot be a duplicate in the first place, and the detector will not report it.

Of the 62 cross-file groups, the 66 redundant copies fall on the boundary as follows:

| Boundary | Redundant copies |
|---|---|
| unit ↔ integration | **33** |
| integration ↔ integration | **20** |
| unit ↔ unit | **13** |

The file pairs carrying them, largest first:

| Groups | Pair |
|---|---|
| 16 | `tests/config_provider/mcp.inc` ↔ `tests/security_permission/ssrf.inc` |
| 4 | `src/tui/app/state/manifest.rs` ↔ `tests/tui_render/tui_manifest_restore.inc` |
| 4 | `crates/egglsp/src/degradation_policy.rs` ↔ `tests/ide_protocols/phase5_context_integration.inc` |
| 3 | `src/security/ssrf.rs` ↔ `tests/config_provider/mcp.inc` |
| 3 | `src/security/ssrf.rs` ↔ `tests/security_permission/ssrf.inc` |
| 3 | `crates/egglsp/src/client.rs` ↔ `tests/ide_protocols/lsp.inc` |
| 2 | `src/tui/composer_claims.rs` ↔ `tests/tui_render/tui_composer_claims.inc` |
| 2 | `crates/codegg-core/src/memory/mod.rs` ↔ `tests/agent_loop/memory.inc` |
| 2 | `tests/tool_programs/hosted_tool_program_adapter.inc` ↔ `tests/tool_programs/hosted_tool_program_security.inc` |
| 2 | `tests/tool_programs/tool_program_m012_authority.inc` ↔ `tests/tool_programs/tool_program_m012_broker_failures.inc` |
| 2 | `src/python_script/analyze.rs` ↔ `src/python_script/mod.rs` |
| 2 | `src/tui/components/dialogs/agent.rs` ↔ `src/tui/components/dialogs/session.rs` |

The remaining thirty-one pairs carry one group each and are spread across `src/`, `crates/`, and the families — `src/tui/app/state/persistence.rs` ↔ `tests/tui_render/tui_manifest_restore.inc`; `src/tui/app/state/view_switch.rs` ↔ `tests/tui_render/tui_project_routing.inc`; `crates/codegg-providers/src/responses_api.rs` ↔ `tests/tool_programs/hosted_tool_program_security.inc`; `src/eggsact/adapter.rs` ↔ `tests/tool_programs/eggsact_adapter.inc`; `crates/codegg-core/src/work_plan/checkpoint.rs` ↔ `crates/codegg-core/src/work_plan/projection.rs`; and so on. The implementing agent regenerates this table from the detector and reconciles it against §3.1 before any removal.

### 3.6 The same-file groups

Twenty-eight groups hold two copies of the same body inside one file, carrying 34 redundant copies. They concentrate in `tests/ide_protocols/lsp.inc` (6), `src/shell/rtk.rs` (4), `src/python_script/analyze.rs` (4), and `tests/command_git/command_routing_adversarial.inc` (3); the remaining twenty-two files carry one or two each.

These are not always redundant: a file that asserts the same property twice for two different inputs is a **table written longhand**, and the detector must not collapse it. The triage in work package B must distinguish "the same assertion twice" from "the same shape twice, with different arguments" — the content-preserving digest of §3.4 is what makes that distinction, and the agent must verify each same-file group by reading it.

## 4. Invariants that must not regress

- **I1 — A removal names its survivor.** No test body is removed unless a byte-identical body remains, and the removal record names where that survivor lives. This is the roadmap's invariant I1 applied to duplication: the exception it allows is "a removal recorded as a proven duplicate naming its survivor".
- **I2 — The survivor keeps the stronger property.** Where a unit copy and an integration copy are identical, the survivor is the one that proves the stronger reach — the integration copy when the body touches only `pub` items, the unit copy when it touches `pub(crate)` items, and both when they straddle a crate boundary.
- **I3 — The test-function count is reported, not assumed.** Before and after counts are recorded. Every removed function appears in a removal table with its survivor.
- **I4 — The detector is content-preserving.** A detector that compares bodies with literal contents blanked is invalid, because it cannot distinguish a table written longhand from a duplicate. §3.4's first failure mode is the test.
- **I5 — The sole-guard set is untouched.** The compile-time assertions and the identity-newtype round-trip named in the roadmap's §4.5 — `crates/codegg-core/src/project_catalog.rs:2306`, `src/shell/redactor.rs:699`, `crates/codegg-core/src/identity.rs:466`, `crates/eggsentry/src/command.rs:1098` and `:848`, `src/scheduler/job_notifications.rs:1957`, `crates/egglsp/src/edit.rs:779`, the `codegg-git` parse/render/risk triples, and the `Send + Sync` assertion sites — are not removal candidates.
- **I6 — Family topology and the ownership manifest stay consistent.** If a member file becomes empty and is removed, the family `mod.rs` drops its `#[path]` line, the manifest's member list follows, and `scripts/check_test_ownership.py` still passes.
- **I7 — The Nextest `ci` profile and the CI job's executed set are unchanged.** This milestone adds no `required-features`, renames no target, and moves no test between families.
- **I8 — Guard-trigger routing is prose only.** The new guard is wired into `scripts/verify.sh quick` and the CI job unconditionally; no machine-readable router is introduced.

## 5. Scope

### In scope

- The 62 cross-file duplicate groups and the 28 same-file groups, measured on the implementation baseline.
- The shared-helper fan-out: `tests/common/secret_scan.rs`'s five self-tests compiled into eight families (§3.3).
- `crates/egglsp/tests/common/`'s compile-only duplication across six binaries (§3.3).
- The content-preserving duplicate detector and the static guard `scripts/check_test_duplication.py`, with a `--self-test`, wired into `scripts/verify.sh quick` and `.github/workflows/ci.yml`.
- The removal table: every removed function, its file and line, and its survivor.

### Explicitly out of scope

- **Removing any assertion that is not byte-identical to a retained one.** Near-duplicates, table-written-longhand, and same-shape-different-argument groups are triaged and **kept**, with the reason recorded.
- **Changing the assertion-quality findings.** Those are M006's subject. This milestone does not re-point, repair, or delete a test because its content is wrong or vacuous.
- **Consolidating test files into new families, or renaming any target.** That is M004's subject and it is closed.
- **Any product source change.**
- **Enabling feature-gated targets in CI.** That is roadmap §10 D1, an owner decision.

## 6. Required production changes

**None.** The only non-test files this milestone adds are `scripts/check_test_duplication.py` and the detector module it shares with M006.

### Documentation and static guards

`scripts/check_test_duplication.py` has three classes:

| Class | Rule |
|---|---|
| `cross-file-duplicate` | Two test bodies in **different files** are byte-identical after normalisation, and neither is recorded in the removal table as a survivor pair. |
| `same-file-duplicate` | Two test bodies in the **same file** are byte-identical after normalisation, and the pair is not recorded as an intentional repeat. |
| `shared-helper-assertion` | A helper module under `tests/common/` or `crates/*/tests/common/` carries its own `#[test]` functions, or is declared by a family that does not use it. |

The normalisation is: strip comments, replace every literal with a **content-derived marker**, collapse whitespace, and drop the function's own name. The guard's `--self-test` must inject one violation of each class and prove the guard rejects it, and it must include a **negative** case: two bodies that differ only in a string argument must **not** be reported. A guard that reports those is the failure mode of §3.4 and a self-test that does not catch it is a broken self-test.

## 7. Ordered work packages

### Work package A — Extend the shared scanner into a content-preserving duplicate detector

Intent: §3.1's numbers are only trustworthy because the detector preserves literal content. The detector is the milestone's real deliverable.

Required changes:

1. Extend the M006 scanner module with a **digest** mode: every string, raw, byte, and character literal is replaced by a marker derived from its *content*, so equal arguments compare equal and different arguments compare different.
2. Add a **normalisation** step for body comparison: comments removed, literals replaced by their content markers, whitespace collapsed, the function's own name dropped.
3. Keep the scanner's **line-preserving** property for body extraction, so the original text is available for the §3.5 triage and so a reported line number is the function's own line. A mask that collapses a multi-line string literal's newlines shifts every line number after it in that file — measured on this tree in **298 of 1,349** files (§3.4, failure mode 5) — and the removal table is built from line numbers.
4. Validate the detector before it is used to remove anything:
   - the parsed test-function counts must still equal the ground-truth attribute counts (4,978 / 7,433 / 3,593);
   - the mask's newline count must equal the original's for every file;
   - the detector must report the group count measured in §3.1 — **90 groups**, **62 of them cross-file** — on the implementation baseline, and the implementation must record any difference with its cause;
   - the detector must report **zero** duplicates for a fixture whose members differ only in a string argument.
5. Record the validation output in the milestone's own §17 implementation record.

Acceptance evidence:

- The ground-truth count check passes with no unexplained residual.
- The measured group count matches §3.1, or the difference is explained.
- The negative fixture — bodies differing only in an argument — is reported as **not** duplicate.
- The detector is imported from the M006 module, not reimplemented.

### Work package B — Triage every group into a decision

Intent: no body is removed before every group has been read and classified. §3.5 shows that a byte-identical body can still be load-bearing.

Required changes:

1. Produce a **decision table** with one row per group: the group's members with `file:line`, the boundary kind (the five kinds in §3.5), the **survivor**, the reason the survivor keeps the stronger property, and the disposition of the others.
2. For the 28 same-file groups, read each one and classify it as an **intentional repeat** (kept, with the reason) or a **true duplicate** (one removed, naming the survivor). A group whose bodies differ only in their arguments is an intentional repeat, not a duplicate — §3.4's first failure mode is exactly what makes this distinction visible, and a group that reaches this row as a duplicate must be re-checked against the original text.
3. Apply I2 to every cross-file group. Where a unit and an integration copy are identical and the body touches only `pub` items, the integration copy is the survivor. Where the body touches `pub(crate)` items, the unit copy is the survivor and the integration copy cannot be a duplicate at all.
4. Where a pair straddles a crate boundary — §3.5's last two rows — both copies are kept unless the decision table records a reason specific to that pair. The boundary check is a property the pair proves and one copy does not.
5. For the SSRF cluster specifically, record that M006 has already re-pointed `tests/config_provider/mcp.inc` at production, so all three copies assert the same thing against the same function, and record the survivor decision for each of the thirteen groups.
6. Record the sole-guard set (I5) as **explicitly excluded** in the table, so its exclusion is a decision rather than an omission.

Acceptance evidence:

- A decision table covering every group in §3.1, with a survivor and a reason per row.
- Every same-file group classified as an intentional repeat or a true duplicate, with a reading note.
- The sole-guard set recorded as excluded.

### Work package C — Apply the removals

Intent: the removals themselves, which are small and mechanical once the table exists.

Required changes:

1. Remove exactly the bodies the decision table marks for removal, and nothing else.
2. Do not remove a whole test file unless every one of its test functions is removed; if a member file becomes empty, remove the file, drop its `#[path]` line from the family `mod.rs`, and update the ownership manifest if the manifest lists it (I6).
3. Do not rename, move, or reformat a surviving test.
4. Update the decision table with the removal as executed — the file and line as they were **before** the removal.

Acceptance evidence:

- The removal table matches the executed removals exactly, with the before-state line numbers.
- The test-function count after the milestone equals the count before minus the number of removed functions.
- `scripts/check_test_ownership.py` and `scripts/check_test_target_references.py` still pass.

### Work package D — Stop the shared-helper fan-out

Intent: eight families compile five assertions they do not need.

Required changes:

1. Move the five `#[test]` functions in `tests/common/secret_scan.rs` (`:98`, `:105`, `:112`, `:127`, `:135`) into the one family that uses the helper — `command_git` — as a family member, and leave `tests/common/secret_scan.rs` assertion-free. The five assertions are preserved exactly; they execute once instead of eight times.
2. For the seven families that declare `mod common;` but use none of it — `agent_loop`, `core_daemon`, `projection_replay`, `scheduler_jobs`, `server_feature`, `session_family`, `tool_programs` — remove the `mod common;` declaration **only if** no member of that family references any helper in `tests/common/`. Each removal must be justified by a reference search over that family's members, recorded in the closure evidence.
3. Apply the same reasoning to `crates/egglsp/tests/common/`: it carries no assertions, so the duplication is compile-only. Decide per consumer whether the helper can be shared or must be inlined, and record the decision. This crate is outside the root package's family topology and is a separate, smaller piece of work.
4. Do not change what any helper *does*. The helpers' behaviour and their assertions are preserved.

Acceptance evidence:

- The five `secret_scan` assertions execute in exactly one target, named.
- The reference search behind each removed `mod common;` declaration is recorded.
- The `shared-helper-assertion` guard class reports zero hits and fires on an injected violation.

### Work package E — Ship the guard and wire it

Intent: the property must hold after this milestone, not merely during it.

Required changes:

1. Deliver `scripts/check_test_duplication.py` with the three classes from §6, over the content-preserving detector from work package A.
2. Give it a `--self-test` that injects one violation of each class and proves rejection, **and** that proves the §3.4 negative case is not reported.
3. Seed the guard's allowlist with the survivors and intentional repeats from the decision table, each with its reason, so a later reader sees *why* a pair is allowed.
4. Wire the guard and its `--self-test` into `scripts/verify.sh quick` and into the CI job's guard block, alongside the existing `check_cargo_profile_alignment.py`, `check_test_ownership.py`, `check_test_target_references.py`, `scoped_selection.py --self-test`, and M006's `check_test_assertion_quality.py`.
5. Make the guard fail on the **pre-milestone** tree, so its value is demonstrated rather than asserted.

Acceptance evidence:

- The guard passes on the milestone's tree and fails on the implementation baseline.
- The `--self-test` proves one rejection per class and the negative case.
- The guard and its self-test run in `scripts/verify.sh quick` and in the CI job.

### Work package F — Reconcile the counts

Intent: the counts this milestone publishes must be measured against their own source, and the ones already published must not be left stale.

Required changes:

1. Report the before-and-after test-function counts, per tree and per family, with the removal table as the reconciliation.
2. Re-pin `architecture/overview.md`'s "Verified counts" table if this milestone changes a count it owns, verifying each number against its own listed `Source`.
3. Update `architecture/testing.md` with the corrected totals and with the scanner's validation counts as their source.
4. Correct the roadmap's §4.5 item 4 figure — *"a shared helper's five self-tests compiled into 46 binaries"* — to the measured post-M004 scale (eight families, forty executions), and record that the item is discharged by work package D.
5. Record in the closure evidence the corrected cross-file duplication figure, so the next reader has the measured number rather than the roadmap's implied one.

Acceptance evidence:

- The before-and-after counts and the removal table reconcile exactly.
- Every count published by this milestone is verified against its listed source.
- The roadmap's §4.5 figures that this milestone changes are corrected, with the measured value and its source.

## 8. Failure, cancellation, restart, and contention semantics

Not applicable in the production sense: this milestone changes no runtime behaviour, no persistence, and no concurrency.

One test-level semantic matters and must not regress: **a removed test must leave its assertion behind**. §10's verification is what proves it.

## 9. Compatibility and migration

No data migration, no protocol change, no configuration change.

The one compatibility surface is **test identity**. Removing a test function removes its nextest id. Any workflow or script that filters on a specific test id — including `.github/workflows/linux-m004-native.yml` and `scripts/test-eggsearch-pair.sh` — must be checked against the removal table, and a filter that would now match nothing must be widened rather than left vacuous. A filter matching nothing is the failure mode M004 already had to correct.

## 10. Required tests

### Focused unit tests

- The detector's self-tests: raw strings in every hash-count form, byte strings, character literals, lifetimes (`&'a T`), nested block comments, `async fn` after `#[tokio::test]`, and a file whose string literal contains a brace, a quote, and a `//`.
- A detector test proving that two bodies differing **only** in a string argument are not duplicates, and that two bodies with an identical argument **are**.
- A detector test proving the ground-truth count check passes on the repository and fails on a fixture with one removed attribute.
- A detector test proving that a body whose literals were blanked is **not** accepted as equivalent — the §3.4 first failure mode, pinned.

### Integration tests

- `cargo nextest run -p codegg --locked --test security_permission` and `--test config_provider` after the SSRF cluster's removals.
- `cargo nextest run -p codegg --locked --test command_git` after work package D.
- `cargo nextest run -p codegg-core --locked`, `-p egglsp --locked`, and `-p codegg-providers --locked` for the cross-crate groups they carry.

### Restart and recovery tests

None. This milestone changes no durable state.

### Contention and cancellation tests

None. This milestone changes no concurrency.

### Security and negative tests

- The new guard's `--self-test`, proving one rejection per class and the §3.4 negative case.
- A demonstration that the guard does **not** flag the sole-guard set (I5), using those bodies as the negative cases.

### Migration and compatibility tests

- A check that every nextest id referenced by a workflow or script still exists after the removals.

## 11. Required verification commands

```bash
# narrow tests first
cargo nextest run -p codegg --locked --test security_permission
cargo nextest run -p codegg --locked --test config_provider
cargo nextest run -p codegg --locked --test command_git
cargo nextest run -p codegg-core --locked
cargo nextest run -p egglsp --locked
cargo nextest run -p codegg-providers --locked

# static guards
python3 scripts/check_test_duplication.py --self-test
python3 scripts/check_test_duplication.py
python3 scripts/check_test_ownership.py
python3 scripts/check_test_target_references.py
python3 scripts/check_test_assertion_quality.py

# formatting and linting
cargo fmt --all
cargo fmt --check --all
cargo clippy --workspace --all-targets --locked -- -D warnings

# broader suite appropriate to the change
cargo nextest run --workspace --locked --profile ci
```

Do not claim commands that were not actually run in the closure record.

## 12. Documentation updates

- `plans/subsystems/ci-test-throughput-recovery-roadmap.md` §4.5 — correct the shared-helper figure (item 4) and record the measured cross-file duplication figure.
- `architecture/overview.md` — the "Verified counts" table is the single source of truth for counts. Any count this milestone changes is re-pinned here, verified against its own listed `Source`.
- `architecture/testing.md` — the corrected test-function totals and the scanner's validation counts as their source.
- `AGENTS.md` — add the new guard to the `verify.sh quick` list.
- `.opencode/skills/testing-ci/SKILL.md` — document the new guard and the corrected counts.
- `docs/test-ownership.toml` — updated only if a member file is removed (I6).

## 13. Acceptance criteria

1. No two test functions in the repository assert the same thing twice, except where the guard's allowlist records the pair and its reason.
2. Every removed test function appears in a removal table naming the survivor that keeps the assertion, with the survivor's `file:line`.
3. Where a unit copy and an integration copy were identical, the survivor is the one that proves the stronger reach, per I2, and the decision table records the reason.
4. The five `secret_scan` assertions execute in exactly one target.
5. The detector is content-preserving: it reports zero duplicates for bodies that differ only in a string argument, and that negative case is pinned by the `--self-test`.
6. The detector's parsed test-function counts equal the ground-truth attribute counts, with no unexplained residual.
7. `scripts/check_test_duplication.py` passes on this milestone's tree, fails on the implementation baseline, and its `--self-test` proves one rejection per class and the negative case.
8. The guard and its `--self-test` run in `scripts/verify.sh quick` and in the CI job.
9. The before-and-after test-function counts are reported and reconcile exactly with the removal table.
10. Every nextest id referenced by a workflow or a script still exists.
11. The sole-guard set is recorded as explicitly excluded, with its exclusion a decision rather than an omission.

## 14. Stop conditions

The agent must stop and report rather than improvise when:

- the detector cannot reproduce the ground-truth test-function counts on any tree, or a residual cannot be explained per file;
- the measured group count differs from §3.1 and the difference cannot be explained — the measurement is authoritative and §3.1 is corrected;
- a group in the decision table cannot be classified by reading it — a group that is neither an intentional repeat nor a provable duplicate is **kept**, and the milestone records that it was kept;
- removing a body would lose a `pub(crate)` reach or a crate-boundary check that the surviving copy does not have;
- a removal would empty a family member file and the ownership manifest cannot be updated in the same change;
- a workflow or script filter would match nothing after the removals and cannot be widened — record it and leave the filter, rather than shipping a vacuous step;
- the milestone would require changing a product source file — that is out of scope and must be raised, not done;
- scope would expand into another subsystem roadmap.

## 15. Closure evidence required

The closure record `plans/closure/ci-test-throughput-recovery/007-status.md` must contain:

- the decision table, with every group, its boundary kind, its survivor, and its reason;
- the removal table, with each removed function's before-state `file:line` and its survivor's `file:line`;
- the same-file groups' classification, with a reading note per group;
- the detector's validation table: per-tree test-function counts, the ground-truth attribute counts, and any residual explained;
- the measured group counts before and after, reconciled against the removal table;
- the reference search behind each removed `mod common;` declaration;
- the guard's three-class result on the milestone tree, its failure on the implementation baseline, and its `--self-test` output — including the negative case;
- the sole-guard set, recorded as excluded;
- the corrected roadmap §4.5 figures, with the measured value and its source;
- an explicit statement of what coverage is **preserved**, what is **dropped**, and what is **gained** by this milestone;
- a recommendation: **closed**, **conditionally closed**, **corrective pass required**, or **blocked**.

## 16. Handoff notes

**Uncommitted predecessor work.** The M001–M004 change set is **not committed** at the time of writing — `git status --short` reports 397 paths, and `git log origin/main..HEAD` is empty. This milestone is planned against that working tree and must be applied **after** the M001–M004 set is committed. The commit will change the baseline this milestone's counts were measured on; the implementing agent must re-run the detector's validation and re-measure §3.1's counts on the tree it actually works on, and record the baseline it used.

**Rebase requirement.** `origin/main` has advanced past this worktree's HEAD, and the advance adds twelve top-level `tests/*.rs` files plus one `tests/common/` helper. The root package has no `autotests = false`, so those twelve files would auto-discover as twelve new cargo test targets, taking the root from 21 to 33 and invalidating the family topology M004 established. That absorption belongs to the hosted-evidence closure milestone (reserved as M005). This milestone must not start until the topology question is settled, or its file paths will be wrong.

**Preserved user changes.** The working tree contains 397 modified and deleted paths belonging to the M001–M004 change set. Do not revert, reset, or clean them.

**Resource constraint.** The detector is a static pass over roughly 1,350 files and takes well under a minute; it needs no build. The expensive commands in §11 are the workspace-wide ones. Cap build jobs (`CARGO_BUILD_JOBS=2`) and run them when the machine is quiet — the machine this plan was authored on carried contention from other worktrees throughout.

**Do not trust a duplicate count from any other source.** The §3.1 figures were produced by the content-preserving detector and are the only duplication figures this milestone may quote. A figure produced by a scanner that blanks literals is off by a factor of five and will produce removals that are wrong.

**The triage is the work.** The removals are 66 bodies. The decision about which of 62 groups is safe to touch, read one at a time, is the milestone. A milestone that removes bodies without that reading has removed coverage.
