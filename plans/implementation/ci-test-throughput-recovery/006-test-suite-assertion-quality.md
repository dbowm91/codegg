# CI and Test Throughput Recovery Milestone M006 — Test-Suite Assertion Quality

Status: ready for handoff

Repository baseline: `2c1953347896848c2f52c2dfbd9f04029d1c2cc2` (the committed tree the plan was authored against)

Implementation baseline: the `2c1953347` working tree **plus the uncommitted M001–M004 change set** (the tree this work will be applied to). See §16 for the rebase requirement this creates.

Source roadmap:

- `plans/subsystems/ci-test-throughput-recovery-roadmap.md#milestone-m006--test-suite-assertion-quality`

Long-term requirements:

- `plans/000-long-term-specification.md` §4.7 — correctness before transparent magic;
- `plans/003-planning-process.md` §2.3–2.4.

Applicable ADRs: none required. This milestone changes **test files only**. It does not alter product authority, protocol, storage, dependency selection, feature defaults, or runtime architecture, and it ships no production behaviour.

Predecessor work (closed, historical, not reopened by this milestone):

- `plans/subsystems/ci-test-throughput-optimization-roadmap.md` M003–M005 — the predecessor consolidations and the corrected verification counts;
- `plans/subsystems/ci-test-throughput-recovery-roadmap.md` M003 and M004 — the ownership manifest and the family topology this milestone works inside.

Primary class: polish, with a correctness obligation.

---

## 1. Objective

Every test function in the repository either asserts a real, checkable property of production code, or is removed — and **no test asserts a property that production contradicts**.

This milestone does not advance the roadmap's throughput objective. It discharges the **assertion-quality** half of the roadmap's P2 capability ("removal of test-suite waste that is provably not capability"), and it fixes a live contradiction in which two green tests assert opposite answers to the same question.

## 2. Why this milestone is ready

**Hard dependencies:** none. M001–M004 are implemented and locally verified; their closure records recommend *conditionally closed* only on hosted evidence, which does not gate work in this repository.

**Interface dependency: M004** (`plans/implementation/ci-test-throughput-recovery/004-root-test-target-family-consolidation.md`). M004 moved the root integration tests into 19 family directories with `.inc` members, and split the previous one-file-per-target topology. Every path this milestone touches is a post-M004 path, and the family `.inc` convention (`#[path = "member.inc"] mod member;` in `tests/<family>/mod.rs`) is the structure this milestone edits.

**Why it is sixth:** it is the first milestone whose subject is the *content* of the test suite rather than its *topology* or *invocation*. M004 established where the tests live; this milestone establishes that what they say is true.

**Ordering relative to M007:** M006 is a **hard predecessor** of M007 (`007-test-suite-duplication.md`). M006 removes the pasted copy of production logic and the four vacuous test bodies; M007's duplicate detector would otherwise flag those same sites, and the two milestones would fight over the same lines. M007 also reuses M006's scanner, so M006 must deliver it.

**Explicitly not a dependency of, and not blocked by, the hosted-evidence work.** The hosted-CI closure milestone (reserved as **M005**, not yet opened) is blocked on an owner decision about committing and pushing the M001–M004 change set. M006 is independent of that decision and must not wait for it.

## 3. Current implementation evidence

All figures below were measured on the implementation baseline by a Rust-aware scanner that treats string and character literals as opaque, treats `'` as a lifetime rather than a character literal, and handles Rust's nested block comments. The scanner's method and its two measured failure modes are recorded in §7 work package A.

### 3.1 A pasted copy of production SSRF logic, and a live contradiction

`tests/config_provider/mcp.inc` defines its own copies of production's IP classifier:

- `tests/config_provider/mcp.inc:5` — `fn ipv6_segments_to_ipv4(ipv6: &Ipv6Addr) -> Option<Ipv4Addr>`
- `tests/config_provider/mcp.inc:33` — `fn is_internal_ip(ip: &IpAddr) -> bool`

Production's is `src/security/ssrf.rs:22` — `pub fn is_internal_ip(ip: &IpAddr) -> bool`. The copy has drifted from it in three predicates:

| Predicate | Production (`src/security/ssrf.rs`) | Pasted copy (`tests/config_provider/mcp.inc:33`) |
|---|---|---|
| IPv4 multicast | `(octets[0] & 0xf0) == 224` → blocked (`:34`) | **absent** — the copy allows `224.0.0.0/4` |
| IPv6 unique-local | `(segments[0] & 0xfe00) == 0xfc00` → the whole `fc00::/7` (`:43`) | `segments[0] == 0xfc00 \|\| segments[0] == 0xfd00` → only those two exact values |
| IPv6 multicast | `(segments[0] & 0xff00) == 0xff00` → blocked (`:44`) | **absent** — the copy allows `ff00::/8` |

The copy's helper `ipv6_segments_to_ipv4` (`tests/config_provider/mcp.inc:5`) is a copy of production's `pub fn ipv6_segments_to_ipv4` (`src/security/ssrf.rs:57`).

Twenty test functions assert against the copy — `tests/config_provider/mcp.inc:67` through `:219`: eleven `test_mcp_ipv4_*` and nine `test_mcp_ipv6_*`. The consequence is a **direct contradiction between two green tests**:

- `tests/config_provider/mcp.inc:192` `test_mcp_ipv6_not_ula` asserts that `fc01::` is **not** an internal IP.
- `tests/security_permission/ssrf.inc:127` `test_ipv6_ula_fc00` asserts at `:135` that `fc01::` **is** an internal IP, against the real function.

Both pass today, because the first calls the pasted copy and the second calls production. Only one can be true of the production predicate. Production blocks `fc01::`; the copied test asserts it is allowed.

The paste was never necessary. `codegg::security::ssrf` is publicly reachable — `src/lib.rs:74` declares `pub mod security;` and `src/security/mod.rs:31` declares `pub mod ssrf;` — and `tests/security_permission/ssrf.inc:6` already imports `is_internal_ip`, `validate_host_ip`, and `validate_url_host` from it.

### 3.2 Four test functions whose bodies cannot fail

| Site | Why it cannot fail |
|---|---|
| `crates/codegg-core/src/tool_program/guards.rs:76` `parse_only_marker_compiles` | Calls `assert_parse_only!()`, whose expansion (`:37`–`:49`) is a `const _: () = { … }` block that documents the invariant and asserts nothing at runtime. |
| `crates/codegg-core/src/tool_program/guards.rs:82` `cpython_guard_function_exists` | Calls `cpython_execution_is_forbidden()` (`:57`), a documentation-only function with an empty body. |
| `crates/codegg-core/src/tool_program/guards.rs:89` `no_cpython_import_in_module` | The body is **comments only**. |
| `crates/egglsp/tests/real_server_smoke.rs:8076` `check_name_advertised_is_removed` | The body is **comments only**. |

The three in `guards.rs` describe an invariant whose real enforcement is *"no `pyo3` dependency in `Cargo.toml`, no `std::process::Command` for Python, no `eval`/`exec`/`compile` on user source"* — the test bodies assert none of those. The fourth asserts a contract its own comment says is carried by a type.

### 3.3 Repository-writing code generators that live in the test namespace

Five `#[ignore]`d test functions write to the filesystem. Three of them write **inside the repository tree**:

- `src/tool_advisor/causal_frontier.rs:5209` `m002_regenerate_checked_in_receipt` — writes `assets/tool-advisor/…`;
- `src/tool_advisor/causal_frontier.rs:6352` `m003_regenerate_checked_in_receipt` — writes the checked-in M003 receipt;
- `src/tool_advisor/retrieval_relevance.rs:1121` `generate_derived_asset` — writes the derived asset.

Two write only into temporary directories and are **not** repository writers: `tests/config_provider/eggsearch_pair_compat.inc:231` and `tests/agent_loop/agent_loop_harness.inc:845`. A guard that cannot tell these apart would flag legitimate tests, so the guard's rule must distinguish *writing into the repository tree* from *writing into a temporary directory*.

Two further `#[ignore]`d functions are not writers: `src/tool_advisor/retrieval_relevance.rs:1484` `print_bm25_frontier` (prints only) and `crates/egggit/src/process.rs:1936` `private_object_view_warm_performance_samples` (an environment-gated performance qualification, not a generator).

The hazard is not that these write — it is that they are **`#[test]` functions**. `cargo test -- --ignored` is a natural thing to run, it reports these as *ignored tests* alongside real tests, and it will rewrite a checked-in receipt in the working tree with no further ceremony.

### 3.4 Inert `cfg(test)` gates

Twenty-two files carry `#![cfg(test)]` as an inner attribute. All twenty-two are members of the `tool_programs` family. In a `--test` build `cfg(test)` is always true, so the attribute is **inert** — it asserts nothing and guards nothing.

The roadmap's §4.5 inventory attributes "208 test functions" to these gates. **That figure is not reproducible.** Measured on the implementation baseline, the twenty-two gated files carry **12** test functions in total, all of them in `tests/tool_programs/tool_program_m013_authority.inc`; the remaining twenty-one are helper-only modules.

Fourteen further files carry an outer `#[cfg(test)] mod tests { … }` wrapper — `tests/config_provider/mcp.inc`, `tests/config_provider/mcp_reconnect.inc`, `tests/config_provider/search_backend_eggsearch.inc`, `tests/security_permission/ssrf.inc`, `tests/security_permission/doom_loop.inc`, `tests/agent_loop/subagent.inc`, `tests/agent_loop/provider_transcripts.inc`, `tests/agent_loop/causal_active_m005.inc`, `tests/agent_loop/compaction.inc`, `tests/agent_loop/event_processor.inc`, `tests/core_daemon/ws_rate_limit.inc`, `tests/tool_programs/path_validation.inc`, `tests/common/secret_scan.rs`, `tests/causal_observe_replay/causal_observe_replay.inc`. The `cfg(test)` on those is equally inert; the module namespace they create is load-bearing.

The hazard is **silent zeroing**. An inner `#![cfg(test)]` attribute on a module means that if the module is ever reached from a build where `cfg(test)` is false, it vanishes — no error, no warning, and the assertions it held simply stop existing.

### 3.5 Tests that report PASS after skipping

Fifty-six `skipped:` markers appear across `src/`, `tests/`, and `crates/`. Measured by the scanner of §3.6, **89** test bodies reach a skip marker and an early `return` **before any assertion** — the shape in which a test reports *passed* having done nothing. The dominant form is an environment guard:

```rust
if std::env::var(M005_OPT_IN).as_deref() != Ok("1") {
    eprintln!("skipped: set {M005_OPT_IN}=1 to run the native M005 owner probe");
    return;
}
```

Thirteen tests in `tests/linux_git_test_support/linux_m004_process_ipc.inc` use it (at `:132`, `:215`, `:265`, and ten more), and one more is in `tests/linux_git_test_support/linux_git_c002_performance.inc:712`. The opt-in variables are set by `.github/workflows/linux-m004-native.yml`, so the **workflow** is not vacuous — but the family's own test output reports thirteen vacuous passes, and the assertions behind them are the ones the workflow exists to prove.

A runtime `return` is not a skip. Nextest reports an `#[ignore]`d test as *skipped*; it reports a self-returning test as *passed*. The suite's pass count therefore overstates what actually ran.

### 3.6 The scanner problem, measured

The measurement this milestone needs — "which test bodies assert nothing", "which bodies duplicate which", "which bodies skip before asserting" — cannot be done with a regex over Rust source. **Four** failure modes were measured while preparing this plan:

1. **Lifetimes read as character literals.** A regex of the form `'(?:\\.|[^'\\])'` matches `&'a T` and swallows the code up to the next apostrophe. A scanner built this way found **1,204** of the **4,978** test functions in `tests/` — an undercount of 76%.
2. **A raw-string pattern without the `r` prefix matches a closing quote.** A pattern such as `r?#*"(?:[^"]|"(?!#))*"` will start at the closing quote of a plain string, because a word character precedes it, and will consume the following function header. In `tests/security_permission/ssrf.inc` this swallowed seven test functions.
3. **`#[tokio::test]` is followed by `async fn`.** A pattern that requires `fn` immediately after the attribute matches no asynchronous test at all — 2,371 in `tests/`.
4. **A multi-line string literal silently moves every line number after it.** When a scanner replaces a literal's contents with a placeholder, it must keep the newlines *inside* the literal. Replacing a multi-line raw string with a run of `~` collapses its line structure, and every subsequent line number in that file is wrong. Measured on this tree: **298 of 1,349** files contain a multi-line string literal — a raw `r#"…"#` config fixture in `tests/config_provider/config_integration.inc`, an `r##"…"##` shell script in `tests/config_provider/eggsearch_pair_compat.inc`, a multi-line `panic!` message in `tests/eggwork_remote_execution_live/eggwork_remote_execution_live.inc` — and in each of them the reported line number drifted by up to 226 lines. The duplicate **counts** stayed correct; only the line numbers moved, which is the more dangerous failure, because a removal table built on shifted line numbers removes the wrong function.

A scanner that handles raw strings, plain strings, character literals as distinct from lifetimes, nested block comments, and `async fn` reproduces the ground-truth attribute counts **exactly** on `tests/` (4,978) and on `crates/` (3,593), and to within 2 on `src/` (7,433 of 7,435). It must additionally be **line-preserving** — the mask it uses for body extraction must keep the original's newline count — and that property must be checked per file rather than assumed. That scanner is the foundation of this milestone's guard and of M007's.

### 3.7 What this milestone is not

The roadmap's §4.5 inventory listed these items against a pre-M004 baseline. Two of its figures do not reproduce and are corrected here rather than carried forward:

- "twenty-two crate roots carrying an inert `#![cfg(test)]` gate (**208 test functions**)" → **22 files, 12 test functions** (§3.4);
- "**twenty-four** tests that report pass after skipping" → **89** test bodies reach a skip before an assertion (§3.5), of which 56 carry a `skipped:` marker. The earlier figure counted only the ones the old audit happened to sample.

A third figure needs the same treatment. The old audit's *"twenty tests asserting against a pasted copy of production SSRF logic"* reproduces exactly — twenty test functions in `tests/config_provider/mcp.inc` — but it did not record that the copy has **drifted** from production, and therefore did not record the contradiction in §3.1. The count was right and the consequence was missed.

## 4. Invariants that must not regress

- **I1 — No behavioural assertion is deleted to save time.** This milestone removes assertions only where the assertion is provably absent (§3.2), provably false (§3.1), or provably duplicated (M007's subject). Every removal is recorded with its reason.
- **I2 — A test that exercises production must exercise the production function.** A test may not assert against a copy of production logic. This is the invariant §3.1 violates today.
- **I3 — The test-function count does not silently shrink.** Every test function present on the implementation baseline must still be present at the end of this milestone, or be recorded as removed under I1 with its reason. The count must be reported before and after.
- **I4 — Recorded counts are measured against their own source.** The §3 figures are the source of truth for this milestone; they are not copied between documents.
- **I5 — The Nextest `ci` profile, the `ci` workflow's executed set, and the feature defaults are unchanged.** This milestone adds no `required-features`, moves no test between families, and renames no target.
- **I6 — The ownership manifest stays true.** `docs/test-ownership.toml` maps source paths to groups and targets. If this milestone moves a test out of a family or removes a member file entirely, the manifest and `scripts/check_test_ownership.py` must be updated in the same change.
- **I7 — Guard-trigger routing is prose only.** The new guard is wired into `scripts/verify.sh quick` and the CI job unconditionally; no machine-readable router is introduced.

## 5. Scope

### In scope

- Group A — re-point the twenty `tests/config_provider/mcp.inc` SSRF assertions at production and delete the pasted helpers (§3.1).
- Group B — give the four vacuous test bodies a real assertion, or remove them with the invariant recorded (§3.2).
- Group C — move the three repository-writing generators out of the reach of a routine `--ignored` run, and add a guard that keeps them out (§3.3).
- Group D — remove the inert `cfg(test)` attributes and the silent-zeroing hazard (§3.4).
- Group E — make a skip report as a skip, and report the vacuous-pass count honestly (§3.5).
- The Rust-aware scanner (§3.6), delivered as a shared module usable by M007.
- The static guard `scripts/check_test_assertion_quality.py`, with a `--self-test`, wired into `scripts/verify.sh quick` and `.github/workflows/ci.yml`.
- The corrections to the roadmap's §4.5 figures that this milestone establishes (§3.7).

### Explicitly out of scope

- **Removing duplicate test bodies.** That is M007. This milestone must not remove a body merely because an identical body exists elsewhere, with one exception: the three empty-body tests in §3.2 are byte-identical to each other and are resolved here because they are vacuous.
- **Changing any product source.** `src/security/ssrf.rs` is **not** modified. The copy is wrong, not the production function.
- **Enabling feature-gated targets in CI.** That is roadmap §10 D1, an owner decision.
- **Changing the roadmap's throughput mechanism** — profiles, Nextest profiles, the scoped mode, the family topology. All are M001–M004's subject and are closed.
- **Running the whole workspace to measure a change.** The milestone's own measurement uses the scanner, which is static.

## 6. Required production changes

**None.** This milestone ships no production change. The only non-test file it adds is `scripts/check_test_assertion_quality.py` and the shared scanner module it depends on.

### Documentation and static guards

The guard has four classes, each with an explicit rule and an explicit allowlist:

| Class | Rule |
|---|---|
| `inert-cfg-test` | A `#![cfg(test)]` inner attribute, or a `#[cfg(test)]` on a `mod`, in a source file that is compiled only as a test target. |
| `ignored-repository-writer` | An `#[ignore]`d test whose body performs a filesystem write whose target is inside the repository tree. Writes into a temporary directory are not hits. |
| `no-assertion` | A test body containing no panic-capable construct — no `assert!`/`assert_eq!`/`assert_ne!`, no `panic!`/`unreachable!`/`todo!`/`unimplemented!`, no `.expect(`/`.unwrap(`/`.unwrap_err(`, no `assert_matches!` — and no skip marker. |
| `skip-before-assertion` | A test body that reaches a skip marker and an early `return` before any panic-capable construct. |

The `no-assertion` class needs a stated limit, and the limit is measured. The scanner of §3.6 puts **167** test bodies in the `no-assertion` class and **89** in the `skip-before-assertion` class. Both are **review queues, not verdicts**:

- A body with no panic-capable construct is often **delegating**. `tests/server_feature/projection_transport_real.inc:2020` is `projection_test_runtime().block_on(real_tui_pending_snapshot_close_frame_cancels_before_barrier_release_impl())` — the assertion lives inside `…_impl()`. `tests/tool_programs/tool_program_m015_daemon_failpoints.inc:397` is the same shape. Removing these would delete the coverage they carry.
- A body with no panic-capable construct is sometimes a **compile-time assertion** whose value is entirely at compile time. `crates/codegg-core/src/project_catalog.rs:2306` `catalog_error_is_send_sync_static`, `src/shell/redactor.rs:699` `redactor_is_send_sync`, `crates/codegg-core/src/identity.rs:466` `every_identity_round_trips_as_a_string`, `crates/eggsentry/src/command.rs:1098` `all_static_regexes_compile`, `src/scheduler/job_notifications.rs:1957` `unused_newjob_import_guard`, and `crates/egglsp/src/edit.rs:779` `workspace_edit_preview_type_is_reexported` all assert through a type bound, a `const` block, or a compile-time check. These are the **sole-guard set** the roadmap's §4.5 named as untouchable, and they are *correct* as they are. A guard that flags them is wrong.

The guard must therefore recognise a **compile-time assertion** and a **delegating body** as non-vacuous, and every surviving hit must be triaged into exactly one of:

- an assertion is added to the body;
- the body delegates to a named helper that holds the assertion — recorded in the allowlist **with the helper named**, so the next reader can find the assertion;
- the body carries a compile-time assertion — recorded in the allowlist **with the construct named** (`const` block, trait bound, or compile-time check);
- the test is removed.

An allowlist entry that does not name the helper or the compile-time construct is a failure, not an exemption.

## 7. Ordered work packages

### Work package A — Land the scanner and prove it on the tree

Intent: every later work package and every later milestone depends on a scanner that is correct. Two scanners that were wrong by 76% and by seven functions are the reason this is a work package and not a step.

Required changes:

1. Deliver a Rust-aware scanner as a shared module (for example `scripts/rust_test_scan.py`) with these properties, each of which is a measured failure mode in §3.6:
   - string literals, raw strings in all hash-count forms (`r"…"`, `r#"…"#`, `br#"…"#`), and byte strings are opaque;
   - a character literal is distinguished from a **lifetime** — `'(?:\\.|[^'\\\n])'` matches only a closing quote that immediately follows;
   - Rust's **nestable** block comments are skipped whole;
   - `#[tokio::test]` is followed by `async fn` in the attribute pattern;
   - the scanner is **length-preserving** for body extraction, so a body span found in the masked text can be sliced out of the original — this is what makes a literal's *content* available to the checks, and it is the difference between a correct duplicate check and one that collapses every call with a string argument into a single false group;
   - a separate content-preserving **digest** replaces each literal with a marker derived from its content, so two calls that differ only in their arguments stay distinct;
   - the mask keeps the original's **newline count** — a multi-line string literal must not collapse its own line structure, or every line number after it in that file is wrong (§3.6, failure mode 4).
2. Validate the scanner against ground truth before anything else uses it:
   - the number of parsed test functions must equal the number of `#[test]` + `#[tokio::test]` attributes in the same file, for every file in `tests/`, `src/`, and `crates/` — measured today at 4,978, 7,433 (7,435 attributes), and 3,593 respectively;
   - the scanner must reproduce those counts **exactly**, and any residual must be explained per file rather than tolerated;
   - the mask's newline count must equal the original's for every file — measured today, **298 of 1,349** files fail this check because they contain a multi-line string literal (§3.6, failure mode 4). A scanner that reports the right counts and the wrong line numbers is more dangerous than one that reports neither, because the removal tables downstream are built from line numbers.
3. Record the validation output in the milestone's own §17 implementation record: the per-tree counts, the per-file residuals, and the command that reproduces them.

Acceptance evidence:

- The per-tree test-function counts, produced by the scanner, equal the ground-truth attribute counts, with every residual explained.
- A demonstration that the scanner **distinguishes** two calls that differ only in a string argument — the failure that produced a spurious 49-copy "duplicate" group during this plan's preparation must be shown not to occur.
- The scanner is importable by a second guard without modification, which is M007's hard requirement.

### Work package B — Re-point the SSRF assertions at production

Intent: the contradiction in §3.1 must not survive this milestone. Two green tests asserting opposite answers to the same question is a defect in the suite's truthfulness, not a style issue.

Required changes:

1. Delete the pasted `ipv6_segments_to_ipv4` (`tests/config_provider/mcp.inc:5`) and `is_internal_ip` (`tests/config_provider/mcp.inc:33`).
2. Import the production functions instead — `codegg::security::ssrf::{is_internal_ip, validate_host_ip, validate_url_host}` — as `tests/security_permission/ssrf.inc:6` already does.
3. Keep all twenty test functions and their assertions. Re-point them; do not delete any.
4. Reconcile the one assertion that is now known to be wrong: `test_mcp_ipv6_not_ula` (`:192`) asserts that `fc01::` is not internal, and production blocks it. The assertion must be changed to the truth — `fc01::` **is** internal — and the change must be recorded as a **correction of a wrong assertion**, not as a deletion.
5. Add the three predicates the copy never had — IPv4 multicast `224/4`, IPv6 unique-local beyond the exact `fc00::`/`fd00::` values, IPv6 multicast `ff00::/8` — as new assertions against production. These are coverage the suite did not have.
6. If any of the twenty tests cannot be re-pointed without changing its meaning, record that fact and keep the test out of this milestone rather than distorting it.

Acceptance evidence:

- `tests/config_provider/mcp.inc` contains no locally defined copy of a production function.
- The `fc01::` assertion in `tests/config_provider/mcp.inc` agrees with `tests/security_permission/ssrf.inc:135`.
- The twenty test functions still exist, with their count reported.
- The three newly assertable predicates are present as assertions against production.
- The `config_provider` family and the `security_permission` family both pass.

### Work package C — Give the four vacuous bodies a real assertion, or remove them

Intent: a test that cannot fail is a test that reports a pass it did not earn.

Required changes:

1. For the two `crates/codegg-core/src/tool_program/guards.rs` tests, replace the vacuous body with a check that can actually fail. The invariant they describe — *no `pyo3` dependency, no `std::process::Command` for Python, no `eval`/`exec`/`compile` on user source* — is checkable statically against `Cargo.toml` and against the module's own source. The repository's idiom for that is a `scripts/check_*.py` guard with a `--self-test`, and the guard is the right home for it: a test that asserts a source-text property is a static check wearing a test's clothes.
2. Decide the disposition of `crates/egglsp/tests/real_server_smoke.rs:8075` `check_name_advertised_is_removed`. Its comment states the contract is carried by the typed `LspOperationCompatibility` record's `advertised` field. If that record is reachable from the integration test, assert the field directly. If it is not reachable, the test is redundant with the type and is removed, with the reason recorded.
3. Do the same for `crates/codegg-core/src/tool_program/guards.rs:88` `no_cpython_import_in_module`, whose body is comments only.
4. Whatever disposition each of the four takes, record it against I1. A removal here is a removal of an assertion that was never made.

Acceptance evidence:

- No test body in the repository is byte-identical to an empty body without a recorded disposition.
- The `pyo3`/Python-execution invariant is enforced by a check that **fails on a violation**, demonstrated by the `--self-test`.
- The before-and-after test-function count for `crates/codegg-core` and `crates/egglsp` is reported.

### Work package D — Take the repository writers out of the test namespace

Intent: `cargo test -- --ignored` must not be able to rewrite a checked-in asset.

Required changes:

1. For each of the three repository writers, make the write path require an explicit opt-in that a routine `--ignored` run cannot satisfy — for example an environment variable that the test asserts is set before it writes, or a `--ignored`-plus-flag combination the runner cannot produce accidentally. The exact mechanism is the implementing agent's choice; the requirement is that the default `--ignored` invocation is a no-op on the working tree.
2. Keep the generation capability intact: the checked-in assets must still be regenerable by a documented command, and the command must be recorded in the test's own doc comment, as the existing generators already do.
3. Leave `src/tool_advisor/retrieval_relevance.rs:1483` `print_bm25_frontier` and `crates/egggit/src/process.rs:1935` on their current footing unless the guard flags them; neither writes the repository.
4. Leave the two temporary-directory writers (`tests/config_provider/eggsearch_pair_compat.inc:209`, `tests/agent_loop/agent_loop_harness.inc:844`) alone. The guard must not flag them, and the guard's rule must be precise enough to prove it.

Acceptance evidence:

- `cargo test --workspace --locked -- --ignored` on a clean tree changes no file under `assets/` and no other tracked file. The before/after `git status --short` is recorded.
- The three generators still run when their opt-in is supplied, and the assets they produce are byte-identical to the checked-in ones.
- The guard's `ignored-repository-writer` class reports zero hits on the tree and fires on an injected violation.

### Work package E — Remove the inert `cfg(test)` gates

Intent: the silent-zeroing hazard is real and the attributes currently protect nothing.

Required changes:

1. Remove the `#![cfg(test)]` inner attribute from the twenty-two `tests/tool_programs/*.inc` files. The attribute is inert in a `--test` build and would silently erase the module in any other build.
2. Remove the redundant `#[cfg(test)]` from the fourteen `#[cfg(test)] mod tests { … }` wrappers, keeping the `mod tests { … }` module itself, which is load-bearing.
3. Leave every assertion in place. This is a pure hazard removal: **no test function is added, removed, or changed by this work package**, and the test-function count must be identical before and after.
4. Update the roadmap's §4.5 figure for these gates to the measured one (22 files, 12 test functions), with the correction recorded.

Acceptance evidence:

- The test-function count is unchanged by work package E.
- The guard's `inert-cfg-test` class reports zero hits and fires on an injected violation.
- The roadmap's §4.5 figure is corrected with the measured value and the reason for the earlier figure's non-reproducibility.

### Work package F — Make a skip report as a skip

Intent: a self-returning test reports *passed*. The suite's pass count overstates what ran.

Required changes:

1. Classify every `skipped:` marker in `src/`, `tests/`, and `crates/` into: **unconditional** (the condition cannot be satisfied in any environment), **opt-in-gated** (a variable a qualification workflow sets), or **environment-absent** (a tool such as `git` is not on `PATH`). Record the classification with the count per class.
2. For **unconditional** skips, convert the test to `#[ignore]` so nextest reports it as *skipped*. This is a reporting change; the assertions are unchanged.
3. For **opt-in-gated** skips — the thirteen in `tests/linux_git_test_support/linux_m004_process_ipc.inc` and the one in `tests/linux_git_test_support/linux_git_c002_performance.inc` — the opt-in is set by `.github/workflows/linux-m004-native.yml`. The requirement is that a routine local run must not report these as *passed*. Either the test declares itself skipped when the opt-in is absent, or the guard records them as an explicit, enumerated exception with the workflow that satisfies them named.
4. For **environment-absent** skips, the skip must be reported as a skip rather than a pass.
5. Report the before-and-after skip count in the milestone's §17, and state the honest correction: the suite's "passed" count currently includes tests that did not run, and the corrected number is the one that may be quoted.

Acceptance evidence:

- Every `skipped:` marker is classified, and the classification is recorded with counts.
- No test body reaches a skip-and-return before any assertion without being either reported as skipped or listed as a named exception.
- The guard's `skip-before-assertion` class reports zero unexplained hits and fires on an injected violation.

### Work package G — Ship the guard and wire it

Intent: the property must hold after this milestone, not merely during it.

Required changes:

1. Deliver `scripts/check_test_assertion_quality.py` with the four classes from §6, each with an explicit allowlist whose entries carry reasons — and, for the `no-assertion` class, the name of the helper that holds the delegated assertion.
2. Give the guard a `--self-test` that injects one violation of each class and proves the guard rejects it. A self-test that cannot fail is not a self-test.
3. Wire the guard and its `--self-test` into `scripts/verify.sh quick` and into the CI job's guard block, alongside the existing `check_cargo_profile_alignment.py`, `check_test_ownership.py`, `scoped_selection.py --self-test`, and `check_test_target_references.py` steps.
4. Make the guard fail on the **pre-milestone** tree, so its value is demonstrated rather than asserted.

Acceptance evidence:

- The guard passes on the milestone's tree and fails on the implementation baseline.
- The `--self-test` proves one rejection per class.
- The guard and its self-test run in `scripts/verify.sh quick` and in the CI job.

## 8. Failure, cancellation, restart, and contention semantics

Not applicable in the production sense: this milestone changes no runtime behaviour, no persistence, and no concurrency.

Two test-level semantics do matter and must not regress:

- **A test that skips must not be reported as passed** (§3.5). This is a reporting contract between the test and the runner.
- **A generator that writes the repository must not be reachable by a routine `--ignored` run** (§3.3). The opt-in must be explicit and the default path must be a no-op on the working tree.

## 9. Compatibility and migration

No data migration, no protocol change, no configuration change.

The one compatibility surface is **test identity**. Nextest test ids change when a test function is renamed or moved:

- `mcp.inc`'s twenty functions keep their names and their family, so their ids are unchanged; only the function they call changes.
- The four vacuous tests in work package C either keep their names (if given a real assertion) or disappear (if removed). Either way the change is recorded.
- Work package F converts some runtime skips to `#[ignore]`, which changes those tests' ids from `passed` to `skipped` in nextest output. Any workflow that filters on a specific test id — including `.github/workflows/linux-m004-native.yml` — must be checked against the change, and a filter that would now match nothing must be widened rather than left vacuous.

## 10. Required tests

### Focused unit tests

- The scanner's self-tests: raw strings in every hash form, byte strings, character literals, lifetimes (`&'a T`), nested block comments, `async fn` after `#[tokio::test]`, an unbalanced-brace file, and a file whose string literal contains a brace, a quote, and a `//`.
- A scanner test proving that two calls differing only in a string argument are **not** reported as duplicates, and that two calls with an identical argument **are**.
- A scanner test proving the ground-truth count check passes on the repository and fails on a fixture with one removed attribute.

### Integration tests

- `cargo nextest run -p codegg --test config_provider` and `--test security_permission` after work package B.
- `cargo nextest run -p codegg-core` and `-p egglsp` after work package C.

### Restart and recovery tests

None. This milestone changes no durable state.

### Contention and cancellation tests

None. This milestone changes no concurrency.

### Security and negative tests

- The new static guard's `--self-test`, which must inject and reject one violation of each of its four classes.
- A demonstration that the guard distinguishes a repository write from a temporary-directory write, using the two legitimate temporary-directory writers in §3.3 as the negative cases.
- A demonstration that the `pyo3` / Python-execution check fails when the invariant is violated, not only when it holds.

### Migration and compatibility tests

- A `git status --short` before and after `cargo test --workspace --locked -- --ignored` on a clean tree, proving that no tracked file changed.

## 11. Required verification commands

```bash
# narrow tests first
cargo nextest run -p codegg --locked --test config_provider
cargo nextest run -p codegg --locked --test security_permission
cargo nextest run -p codegg-core --locked
cargo nextest run -p egglsp --locked

# the ignored-writer contract, on a clean tree
git status --short
cargo nextest run --workspace --locked -- --ignored
git status --short          # must be identical to the first output

# static guards
python3 scripts/check_test_assertion_quality.py --self-test
python3 scripts/check_test_assertion_quality.py
python3 scripts/check_test_ownership.py
python3 scripts/check_test_target_references.py

# formatting and linting
cargo fmt --all
cargo fmt --check --all
cargo clippy --workspace --all-targets --locked -- -D warnings

# broader suite appropriate to the change
cargo nextest run --workspace --locked --profile ci
```

Do not claim commands that were not actually run in the closure record.

## 12. Documentation updates

- `plans/subsystems/ci-test-throughput-recovery-roadmap.md` §4.5 — correct the two figures that do not reproduce (the "208 test functions" behind the inert gates, and the "twenty-four" skip-and-report-pass tests) and record the measured values and their sources.
- `architecture/testing.md` — record the scanner's validation counts as the source for the repository's test-function totals, and record the corrected pass/skip accounting from work package F.
- `architecture/overview.md` — the "Verified counts" table is the single source of truth for counts. If the milestone changes a count it re-pins, this table is updated in the same change and each number is verified against its own listed `Source`.
- `AGENTS.md` — add the new guard to the `verify.sh quick` list.
- `.opencode/skills/testing-ci/SKILL.md` — document the new guard and the corrected counts.
- `docs/test-ownership.toml` — updated only if a test leaves a family or a member file is removed (§4, I6).

## 13. Acceptance criteria

1. `tests/config_provider/mcp.inc` defines no copy of a production function, and its `fc01::` assertion agrees with `tests/security_permission/ssrf.inc`.
2. The twenty re-pointed SSRF test functions still exist, and the three predicates the copied logic never had are asserted against production.
3. No test body in the repository is byte-identical to an empty body without a recorded disposition.
4. The `pyo3` / Python-execution invariant is enforced by a check that fails on a violation.
5. `cargo test --workspace --locked -- --ignored` changes no tracked file, and the three generators still regenerate their assets byte-identically when explicitly invoked.
6. No `#![cfg(test)]` inner attribute and no redundant `#[cfg(test)]` module gate remains in a test-target source file.
7. Every `skipped:` marker is classified, and no test reaches a skip-and-return before an assertion without being reported as skipped or named as an exception.
8. `scripts/check_test_assertion_quality.py` passes on this milestone's tree, fails on the implementation baseline, and its `--self-test` proves one rejection per class.
9. The guard and its `--self-test` run in `scripts/verify.sh quick` and in the CI job.
10. The test-function count before and after the milestone is reported, and every removed function is recorded against I1 with its reason.
11. The roadmap's §4.5 figures that did not reproduce are corrected, with the measured value and its source.

## 14. Stop conditions

The agent must stop and report rather than improvise when:

- the scanner cannot reproduce the ground-truth test-function counts on any tree, or a residual cannot be explained per file;
- re-pointing an SSRF assertion would change the test's meaning rather than its target — record it and leave that test out of the milestone rather than distorting it;
- the `pyo3` / Python-execution invariant turns out to be enforced somewhere else already, in which case the check is not added and the existing enforcement is recorded instead;
- removing a repository writer's reachability would break a documented asset-regeneration command, and no equivalent command can be provided;
- a work package would change a product source file — that is out of scope for this milestone and must be raised, not done;
- the measured before/after counts contradict the counts recorded in §3 — the measurement is authoritative and §3 is corrected;
- the milestone would require enabling feature-gated targets in CI — that is roadmap §10 D1, an owner decision;
- scope would expand into another subsystem roadmap.

## 15. Closure evidence required

The closure record `plans/closure/ci-test-throughput-recovery/006-status.md` must contain:

- the scanner's validation table: per-tree test-function counts, the ground-truth attribute counts, and every residual explained;
- the before-and-after test-function counts for the whole repository and per affected crate;
- the disposition of each of the four vacuous tests in §3.2, with the reason;
- the `git status --short` pair proving that a routine `--ignored` run changes no tracked file;
- the regeneration command for each of the three repository writers, and the byte-identity check of the assets it produces;
- the skip classification table with counts per class, and the corrected pass/skip accounting;
- the guard's four-class result on the milestone tree, its failure on the implementation baseline, and its `--self-test` output;
- the roadmap §4.5 corrections, with the measured value and its source;
- an explicit statement of what coverage is **preserved**, what is **dropped**, and what is **gained** by this milestone;
- a recommendation: **closed**, **conditionally closed**, **corrective pass required**, or **blocked**.

## 16. Handoff notes

**Uncommitted predecessor work.** The M001–M004 change set is **not committed** at the time of writing — `git status --short` reports 397 paths, and `git log origin/main..HEAD` is empty. This milestone is planned against that working tree. It must be applied **after** the M001–M004 set is committed, and the commit will change the baseline this milestone's counts were measured on. The implementing agent must re-run the scanner's validation and re-measure §3's counts on the tree it actually works on, and record the baseline it used. The counts in §3 are the counts at the stated implementation baseline and are **not** to be quoted as the counts after the rebase.

**Rebase requirement.** `origin/main` has advanced past this worktree's HEAD. The advance adds twelve top-level `tests/*.rs` files and one `tests/common/` helper, and the root package has no `autotests = false`, so those twelve files would auto-discover as twelve new cargo test targets, taking the root from 21 targets to 33 and invalidating the family topology M004 established. That absorption is **not** this milestone's work; it belongs to the hosted-evidence closure milestone. This milestone must not start until the topology question is settled, or its file paths will be wrong.

**Preserved user changes.** The working tree contains 397 modified and deleted paths belonging to the M001–M004 change set. Do not revert, reset, or clean them.

**Resource constraint.** The scanner is a static pass over 1,349 files and takes well under a minute. It does not need a build. The expensive commands in §11 are the workspace-wide ones; cap build jobs (`CARGO_BUILD_JOBS=2`) and run them when the machine is quiet — the machine this plan was authored on carried contention from other worktrees throughout.

**Serial-test requirement.** `cargo nextest run --workspace --locked -- --ignored` runs the ignored generators. The three repository writers must be exercised only with their opt-in supplied, and the before/after `git status` pair in §10 is the evidence that the default path is inert.

**Environment requirement.** The scanner's validation depends only on the repository's contents. The `no-assertion` and `skip-before-assertion` triage depends on reading bodies, not on running the suite.

**Scanner reuse.** The scanner module is a hard prerequisite for M007 and must be written to be imported, not copied. M007's guard must import it; a second implementation would reproduce the failure modes in §3.6.
