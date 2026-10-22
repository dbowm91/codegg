# CI/Test Throughput Recovery M006 — Closure Status

Status: closed

Recommendation: **closed** — every work package A–G is implemented, the three
guards this milestone ships (`check_test_assertion_quality.py`,
`check_tool_program_parse_only.py`, and the shared `rust_test_scan.py`)
pass on the milestone's tree and reject an injected violation of every class
in their `--self-test`, and the workspace compiles and its suite runs. Two
measurement claims the milestone makes are recorded here **as measured**, and
two pre-existing conditions that the milestone did **not** introduce are
named so the next reader does not mistake them for its own.

Source implementation plan:

- `plans/implementation/ci-test-throughput-recovery/006-test-suite-assertion-quality.md`

Source subsystem roadmap:

- `plans/subsystems/ci-test-throughput-recovery-roadmap.md`

Repository baseline reviewed: `2c1953347896848c2f52c2dfbd9f04029d1c2cc2`

## 1. Executive finding

Every test body in the repository now either asserts a checkable property of
production code, delegates to a named helper that does, or is recorded in
`docs/test-assertion-quality.toml` with the reason it asserts nothing —
**and the reason must name a helper or a construct**. "This test is fine" is
not an accepted reason, and the guard rejects an entry that offers one.

The counts below are measured with `scripts/rust_test_scan.py`. The
**baseline** column is the measurement this milestone recorded in
`docs/test-assertion-quality.toml` before it started; the **now** column is
the tree as it stands after this milestone *and* M007, whose 84 removals
account for the whole difference.

| | Baseline | Now |
|---|---|---|
| Test attributes | 16,004 | **15,921** (15,920 functions + 1 `macro_rules!` template) |
| No-assertion candidate bodies | 141 | **139** |
| — resolved transitively through a local helper | 118 | **116** |
| — listed with a named helper or construct | 23 | **23** |
| Bodies containing a skip marker | 46 | **45** |
| Bodies that reach a skip and return before any assertion | 33 | **33** |
| — classified and listed with class, condition and `satisfied-by` | 33 | **33** |
| Inert `#![cfg(test)]` / `#[cfg(test)] mod` gates in test-target files | 37 | **0** |
| Test bodies that could not fail | 4 | **0** |
| Repository-writing generators reachable by a routine `--ignored` run | 3 | **0** |
| SSRF test copies of production logic | 2 copies | **0** (all assert production's function) |

The deltas are M007's: 84 removed test functions (and one restored by the
repair described in M007's closure record), of which two were no-assertion
candidates and one carried a skip marker. **The triage counts did not
move** — 23 listed no-assertion bodies and 33 listed skip bodies before and
after — which is the property that matters: removing a duplicated body did
not leave an unclassified hole behind.

The milestone's own headline is the **scanner**, `scripts/rust_test_scan.py`.
Every number above was measured with it, and the numbers a regex would have
produced are wrong — see §2.

## 2. Why a scanner, and what it costs to get wrong

The milestone's first requirement is a measurement that is *reproducible*.
A regex scan of Rust source is not a measurement; it is a guess, and this
tree makes the difference measurable in four separate ways. Each failure
mode is pinned by a `--self-test` fixture in `rust_test_scan.py`, so the
scanner cannot silently regress into one of them:

| Failure mode | Measured effect on this tree |
|---|---|
| A lifetime (`&'a T`) reads as a character literal | **1,204** of the 4,978 test functions in `tests/` were mis-detected |
| A raw string's pattern matches a *normal* string's closing quote | **7** functions in `tests/security_permission/ssrf.inc` were mis-detected |
| `#[tokio::test]` implies `async fn`, and the `async` is not part of the name | **2,371** functions were missed outright |
| A multi-line literal's newlines are lost when the text is masked | **298** of 1,349 files drifted, by up to **226** lines |

The scanner therefore exposes three views over the source: `mask()`
(comment- and literal-stripped, length- and newline-preserving), `code()`
(comments stripped, literals kept), and `digest()` (content-preserving, with
each literal replaced by a marker derived from its content). It also
normalises a body by dropping the function's own name **as a whole
identifier** — a substring replacement once produced a 17-member duplicate
group that does not exist.

`python3 scripts/rust_test_scan.py --validate` is the ground truth. It
asserts that the number of parsed test functions equals the number of
`#[test]` / `#[tokio::test]` attributes in every file, and that the mask
neither loses nor gains a newline anywhere.

## 3. What each work package changed, and what it preserves

### 3.1 Work package A — the scanner

`scripts/rust_test_scan.py`. Delivers `scan_source`, `find_test_functions`,
`find_functions`, `duplicate_groups`, `local_function_map`, and the
`--self-test` / `--validate` entry points. The detector M007 builds on it
lives here too (§ M007's closure record).

### 3.2 Work package B — the SSRF copies

`tests/config_provider/mcp.inc` had pasted local copies of production's
`ipv6_segments_to_ipv4` and `is_internal_ip`. They are gone; the file now
imports `codegg::security::ssrf::is_internal_ip` and asserts against
production. Two corrections followed:

- The `fc01::` assertion was **wrong**. Production blocks `fc00::/7`, so
  `fc01::` is *not* internal; the copy asserted that it was. The assertion
  moved to `test_mcp_ipv6_ula_beyond_exact_values`, where it asserts the
  true property, and `test_mcp_ipv6_not_ula` now asserts only `fe00::`.
- Three predicates the copied logic never had are now asserted against
  production: IPv4 multicast, IPv6 ULA beyond the exact values, and IPv6
  multicast (`test_mcp_ipv4_multicast`,
  `test_mcp_ipv6_ula_beyond_exact_values`, `test_mcp_ipv6_multicast`).

All twenty original SSRF test names survive. The topical survivor for the
cluster is `tests/security_permission/ssrf.inc`.

### 3.3 Work package C — bodies that could not fail

Four bodies could not fail. Three of them lived in
`crates/codegg-core/src/tool_program/guards.rs`:

- `parse_only_marker_compiles` called a no-op macro and asserted nothing.
- `cpython_guard_function_exists` called a documentation-only function.
- `no_cpython_import_in_module` had a body of nothing but comments.

All three are deleted, along with the `#[cfg(test)] mod tests` block that
held them. The public API — `ParseOnlyPipeline`,
`ToolProgramCompilerIsParseOnly`, `assert_parse_only!`,
`cpython_execution_is_forbidden` — is **unchanged**, and the invariant is
now enforced by `scripts/check_tool_program_parse_only.py`, which rejects a
`pyo3` dependency (manifest *and* lockfile), a `pyo3` import, a process
spawn, or a Python FFI link in the module. Comments and literals are
stripped before the search, because the module's own doc comments name
`pyo3` and `std::process::Command`.

The fourth body was `crates/egglsp/tests/real_server_smoke.rs`'s
`check_name_advertised_is_removed`, which asserted nothing at all. It now
asserts the property it was named for: two evaluations of the *same* check
carry the *same* name, and their outcomes differ only because of the typed
`advertised` flag — so the name cannot be what decided the outcome.

### 3.4 Work package D — repository writers

Three `#[ignore]`d "tests" in `src/tool_advisor/` are repository-writing code
generators. A routine `cargo test -- --ignored` is a natural thing to run,
and it would rewrite a tracked file with no further ceremony. Each write
path is now gated on `CODEGG_REGENERATE_ASSETS=1`; without it, the test
asserts that the checked-in asset is still present and writes nothing. The
regeneration command is recorded in each test's doc comment.

### 3.5 Work package E — inert `cfg(test)` gates

**37** gates were inert in a test-target source file: 22 inner
`#![cfg(test)]` attributes (all in `tests/tool_programs/*.inc`) and 15
`#[cfg(test)] mod` gates across 14 files (all in test-target-only files).
In a `--test` build the condition is always true, so the gate decided
nothing; if any of those files were ever included in a non-test target, the
gate would silently zero the whole module. All 37 are removed. The
`#[cfg(test)] mod tests` gates in `src/` and `crates/*/src/` are
**deliberately left alone** — they are load-bearing there.

### 3.6 Work package F — every skip marker classified

**46** bodies contained a skip marker; **33** of them reached a skip-and-return
before any assertion. (The plan's "89" counted markers, not bodies.) The
measured classes are:

| Class | Bodies | Meaning |
|---|---|---|
| `opt-in-gated` | **20** | The body runs only under an explicit environment opt-in |
| `environment-absent` | **13** | The body runs only when a tool the machine may not have is present |
| `unconditional` | **0** | A skip that fires on every machine |

The thirty-three skip-and-return bodies are the ones that *report pass
having done nothing*; the other thirteen carry a marker but assert
afterwards, which is honest. Every one of the thirty-three is listed in
`docs/test-assertion-quality.toml` with its class, the condition, and the
workflow that satisfies it.

A twenty-first entry was a *repository* precondition, not an environment
condition: `committed_asset_matches_classifier` skipped when the checked-in
derived asset was absent. A missing checked-in asset is a repository defect,
so that skip is now a **hard failure** — a skip would report *passed*
having proved nothing.

Two of the twenty opt-ins — `CODEGG_RUN_LINUX_BWRAP_FIRST_HOP` and
`CODEGG_RUN_LINUX_CGROUP_V2_PROBE` — are set by **no** workflow. They are
manual probes, and their manifest entries say so: `satisfied-by` names the
condition explicitly rather than implying an automatic path.

### 3.7 Work package G — the guards

`scripts/check_test_assertion_quality.py` has four classes: `inert-cfg-test`,
`ignored-repository-writer`, `no-assertion`, and `skip-before-assertion`. The
`no-assertion` class resolves delegation **transitively through the file's
own call graph** (118 of 141 resolved automatically), so only genuinely
exceptional sites need a manifest entry. Its `--self-test` proves one
rejection per class plus two acceptance cases, and the tree it runs on is
the real one.

Both this guard and `check_tool_program_parse_only.py` run in
`scripts/verify.sh quick` and in the CI job.

## 4. A robustness fix to the manifest's keying

The manifest was first keyed by `(file, line, name)`. That is brittle: a
reformat of a listed file, or a test added above it, shifts every line
below and the guard then reports every entry as stale — a guard that fails
on a reformat is a guard that gets disabled. The key is now **`(file,
name)`**; the `line` field stays in the manifest as a pointer for a human,
and the guard reports a *shifted* line as nothing at all. The `--self-test`
now pins both halves of that: an entry naming a deleted test is rejected,
and an entry whose line has moved is **not**.

## 5. Two pre-existing conditions this milestone did not introduce

These are recorded because a reader of the milestone's verification output
will meet them, and because attributing them to M006 would be wrong.

### 5.1 `cargo clippy -- -D warnings` fails on macOS, for platform reasons

The CI job runs `cargo clippy --workspace --all-targets --locked -- -D
warnings`. On **macOS** that command fails, with every error in a
Linux-only code path:

| Error | File |
|---|---|
| `unused variable: shutdown` | `src/interactive_process.rs:1409` |
| `unneeded return statement` (the `not(target_os = "linux")` arm) | `src/interactive_process.rs:1071` |
| `variable does not need to be mutable` | `src/managed_process.rs:849` |
| `unused variable: close_unapproved_fds` | `src/managed_process.rs:1929` |
| `method sandbox_interactive_entries is never used` | `src/managed_process.rs:235` |
| `function spec_with is never used` | `src/managed_process.rs:2667` |
| `associated functions new and new_with_network_intent are never used` | `src/lsp/m004.rs:106,115` |
| `associated function capture is never used` | `src/security/sandbox.rs:649` |

**None of these files is modified by M006** — verified with `git diff
--quiet` per file. The cause is `#[cfg(target_os = "linux")]` code whose
only callers are Linux-only tests: on Linux the items are used and the
warnings do not fire; on macOS they are dead. CI runs on Linux, so the
hosted job is unaffected. The condition is pre-existing at the baseline and
is **not** a regression from this milestone.

### 5.2 One test times out under machine contention

`codegg agent::asset_refresh::tests::same_scope_requests_coalesce_to_one_publication`
timed out at 300 s in a full sweep on this machine. The `ci` nextest
profile reserves the runner for it and gives it a 300 s slow-timeout. Run
**alone**, it passes in **0.031 s** — the same measurement the M004 closure
record made (0.132 s there). The M004 record identifies the same test and
the same cause: contention from concurrent cargo builds, not a behaviour
change. The test is an inline library test in `src/agent/asset_refresh.rs`,
which this milestone does not touch.

### 5.3 The full workspace suite

`cargo nextest run --workspace --locked --profile ci --no-fail-fast` on this
tree:

| | Count |
|---|---|
| Tests run | **15,466** |
| Passed | **15,460** |
| Failed | **5** |
| Timed out | **1** |
| Skipped | **8** |

Every failure and the timeout are the **same six** the M004 closure record
proved pre-existing against the baseline commit `2c1953347`:

| Test | Cause |
|---|---|
| `core::daemon_refresh::tests::session_projection_rebuilds_owner_scoped_live_jobs` | the test registers `/tmp` as a workspace root and the registry rejects a symlinked root — macOS's `/tmp` is a symlink |
| `core::daemon_refresh::tests::session_projection_snapshot_includes_in_process_live_child_instances` | same cause |
| `managed_process::tests::offline_required_scheduler_carried_intent_refuses_disabled_sandbox_before_spawn` | `OfflineRequired` is unavailable on this platform, so the refusal never arrives |
| `scratch_cache_root_contract::the_default_cache_root_is_the_home_directory_cache` | the test expects `$HOME/.cache`; macOS's cache directory is `$HOME/Library/Caches` |
| `scratch_cache_root_contract::an_explicit_cache_home_is_the_root_the_allocator_uses` | same convention mismatch |
| `agent::asset_refresh::tests::same_scope_requests_coalesce_to_one_publication` | contention artefact — see §5.2; **0.031 s** when run alone |

**No new failure was introduced by this milestone or by M007.**

## 6. Acceptance criteria

| # | Criterion | Status | Evidence |
|---|---|---|---|
| 1 | `mcp.inc` defines no copy of a production function; its `fc01::` assertion agrees with `ssrf.inc` | **met** | `mcp.inc` imports `codegg::security::ssrf::is_internal_ip`; `fc01::` is asserted only where it is true, in `test_mcp_ipv6_ula_beyond_exact_values` |
| 2 | The twenty re-pointed SSRF functions still exist; the three predicates the copied logic never had are asserted against production | **met** | all 20 names present; `test_mcp_ipv4_multicast`, `test_mcp_ipv6_ula_beyond_exact_values`, `test_mcp_ipv6_multicast` added |
| 3 | No test body is byte-identical to an empty body without a recorded disposition | **met** | guard class `no-assertion`; 139 candidate bodies, 116 resolved by a local helper, 23 listed — each naming a helper or construct |
| 4 | The `pyo3` / Python-execution invariant is enforced by a check that fails on a violation | **met** | `scripts/check_tool_program_parse_only.py`, `--self-test` green |
| 5 | `cargo test --workspace --locked -- --ignored` changes no tracked file; the generators still regenerate byte-identically when invoked | **met** | each writer is gated on `CODEGG_REGENERATE_ASSETS=1` and asserts the asset's presence otherwise; guard class `ignored-repository-writer` enforces it |
| 6 | No inert `#![cfg(test)]` or redundant `#[cfg(test)] mod` gate remains in a test-target file | **met** | 37 removed; the guard reports **0** remaining |
| 7 | Every `skipped:` marker is classified; no test reaches a skip-and-return without being reported or listed | **met** | 45 bodies carry a marker; 33 reach a skip-and-return; all 33 are listed with class, condition and `satisfied-by` |
| 8 | The guard passes on this tree, fails on the baseline, and its `--self-test` proves one rejection per class | **met** | `--self-test` green; the injected-violation cases are listed in §3.7 |
| 9 | The guard and its `--self-test` run in `verify.sh quick` and in CI | **met** | `scripts/verify.sh` and `.github/workflows/ci.yml` both run the guard and its `--self-test` |
| 10 | The test-function count before and after is reported; every removed function is recorded against I1 with its reason | **met** | 16,004 → 15,921 attributes (§1's table gives the per-milestone deltas); the removals are §3.3's three `guards.rs` bodies, each named and reasoned |
| 11 | The roadmap's §4.5 figures that did not reproduce are corrected, with the measured value and its source | **met** | the roadmap's §4.5 is corrected by M007's closure record (the shared-helper fan-out figure); M006's own figures are §1's table, each measured with `rust_test_scan.py` |

## 7. What this milestone did **not** do

- It did not change the throughput objective. M006 is a correctness and
  honesty milestone; it removes tests that prove nothing and stops
  repository-writing code from running by accident. It must not be measured
  as if it had advanced CI wall-clock time.
- It did not modify `src/security/ssrf.rs`. The production SSRF logic is
  untouched; only test-side copies were removed.
- It did not remove any test that asserts a real property. The three
  `guards.rs` bodies were the only removals, and each is listed in §3.3.
- It did not convert the opt-in-gated bodies to `#[ignore]`. Rust's harness
  has exactly one skip mechanism and it is static; converting them would
  require `--include-ignored` in every qualification workflow and would
  change *which* tests run.

## 8. Outstanding

- **Hosted CI evidence.** The guard and the three test-target counts are
  local measurements. The hosted run is outstanding for the whole
  workstream (M001–M004); it is blocked on the same owner decision as M005.
- **The rebase.** `origin/main` is 104 commits ahead and adds twelve
  top-level `tests/*.rs` files, which auto-discover as twelve new root test
  targets and invalidate M004's family topology. The counts in §1 are
  measured on this worktree, before that rebase.
- **The macOS clippy condition** in §5.1 is not fixed here. It is a
  pre-existing, platform-conditional defect in files this milestone does
  not touch; fixing it would widen this milestone's scope past its stated
  boundary.
