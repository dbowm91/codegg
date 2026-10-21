# Harness Lab Post-Pilot Corrective 002 — Verification Evidence

Branch: `plans/harness-lab-post-pilot-correctives`
Baseline reviewed: `2a9fc376707ef14dab4a2f9a27f6aba8ac00dd59`
Verified: 2026-10-21

## 1. Python — the full harness suite

```
$ python3 -m unittest discover -s dev/harness_lab/tests
................................................................................
----------------------------------------------------------------------
Ran 112 tests in 3.186s

OK
```

The suite was `Ran 72 tests ... OK` before this line of work. It is now 112
because C001 added the budget preflight, the supervisor failure decoding and
the fixture fault injection, and C002 added the score contract, the ledger
hardening, the resume gates, and the crash matrix.

## 2. The crash matrix — real subprocesses and real signals

This was the explicitly identified gap in the pilot: no test anywhere killed a
real subprocess to simulate a crash. These four tests use `subprocess.Popen`
and `os.kill(pid, SIGKILL | SIGTERM)` at a deterministic cut point, then
assert the observable end state.

| Test | Signal | What it asserts |
|---|---|---|
| `test_sigkill_between_checkpoint_append_and_pointer_is_recorded` | `SIGKILL` | The event tail holds seqs `[1, 2]` while the pointer still points at seq 1. The resume raises `CheckpointInterrupted` naming `interrupted_unknown` and seq 2. An `INTERRUPTED_UNKNOWN` event (seq 3) is **recorded**, so the checkpoint loads again with the pointer reconciled to the tail. A repeated resume raises again and appends **no** duplicate event or seq. |
| `test_sigterm_between_optimization_append_and_pointer_is_recorded` | `SIGTERM` | A `run_interrupted` event with `outcome == "interrupted_unknown"` is recorded and the pointer is reconciled to the tail. A repeated resume refuses **without** appending a duplicate event; the seqs stay `[1, 2, 3]`. |
| `test_sigkill_during_ledger_write_leaves_no_duplicate_trial_rows` | `SIGKILL` | The ledger still loads. The orphan is never reported as `passed`, is `interrupted_unknown` when surfaced, and is not a scored trial (`index_trials` returns `{}`). The resumed attempt commits **exactly one** index row for `(task-one, 0)`. |
| `test_resumed_run_does_not_append_a_second_row_for_a_recorded_pair` | — | `validate_candidate_resume_rows` returns only the recorded pair, the pending set is exactly the missing pairs, the resume adds one row per pair (3 rows, 3 distinct pairs), and a duplicated completed pair is refused by `index_trials` instead of being scored twice. |

## 3. The score contract

```
$ python3 -m unittest dev.harness_lab.tests.test_scoring
Ran 39 tests in 0.001s

OK
```

| Requirement | Test |
|---|---|
| A correctness fix is accepted with a `correctness_fix` objective | `test_correctness_fix_is_accepted_with_a_correctness_objective` |
| A reliability-only improvement is accepted with a `reliability` objective | `test_reliability_only_improvement_is_accepted_with_a_reliability_objective` |
| A protected-control regression is rejected even with other improvements | `test_protected_control_regression_is_rejected_even_with_other_improvements` |
| An unmeasured required criterion yields `insufficient_evidence` with no score | `test_unmeasured_required_criterion_yields_insufficient_evidence_without_a_score` |
| A partially evaluated candidate is `insufficient_evidence` | `test_partially_evaluated_candidate_is_insufficient_evidence` |
| A fully matched candidate is not accepted | `test_fully_matched_candidate_is_not_accepted` |
| An unmeasured trial never maps to zero | `test_unmeasured_trial_never_maps_to_zero` |
| The control set is derived without touching the suite schema | `test_protected_controls_are_derived_without_touching_the_suite_schema` |
| A correctness fix is not rejected for being slower | `test_a_correctness_fix_is_not_rejected_for_being_slower` |
| A correctness regression still blocks acceptance | `test_a_correctness_regression_still_blocks_acceptance` |
| A reliability regression still blocks acceptance | `test_a_reliability_regression_still_blocks_acceptance` |
| Efficiency is the only advisory floor in the default contract | `test_efficiency_is_the_only_advisory_floor_in_the_default_contract` |

## 4. The hardened comparator

| Requirement | Test |
|---|---|
| A partial pairing, a self-comparison and a zero denominator are refused | `test_compare_refuses_partial_pairing_same_candidate_and_zero_denominator` |
| A subject-identity mismatch is refused | `test_compare_refuses_subject_identity_mismatch` |
| A revision mismatch between paired trials is refused | `test_compare_refuses_revision_mismatch_between_paired_trials` |
| Pairing is by `(task_id, repeat)` and repeats are counted | `test_compare_pairs_by_task_and_repeat_and_counts_repeats` |
| A different case digest is refused | `test_paired_comparison_refuses_different_case_digest` |

## 5. Ledger versioning and recovery

| Requirement | Test |
|---|---|
| A stored v1 ledger loads and reports | `test_stored_v1_ledger_loads_and_reports` |
| A v2 ledger round-trips and reports | `test_v2_ledger_round_trips_and_reports` |
| The supported version range rejects other versions | `test_supported_schema_version_range_rejects_other_versions` |
| An unsupported index or event version is refused | `test_unsupported_index_and_event_versions_are_refused` |
| An orphan run directory is reconciled as `interrupted_unknown` | `test_orphan_run_directory_is_reconciled_as_interrupted_unknown` |
| A torn final index line is tolerated; middle corruption is fatal | `test_torn_final_index_line_is_tolerated_and_middle_corruption_is_fatal` |

## 6. Resume gates

| Requirement | Test |
|---|---|
| A stale binary fingerprint is refused | `test_resume_with_a_stale_binary_fingerprint_is_refused` |
| A non-resume call at `PATCHED` cannot enter `BUILDING` twice | `test_non_resume_call_at_patched_cannot_enter_building_twice` |
| Repeat expansion drives the completeness check | `test_repeats_expansion_drives_the_completeness_check` |

## 7. Rust and workspace guards

Unchanged by this plan; see `001-evidence.md` §1 and §5. `cargo fmt --check
--all` is clean and `cargo clippy -p codegg --all-targets -- -D warnings`
finishes with no warning:

```
$ cargo fmt --check --all && echo "fmt: ok"
fmt: ok

$ cargo clippy -p codegg --all-targets -- -D warnings
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 52.81s
```
