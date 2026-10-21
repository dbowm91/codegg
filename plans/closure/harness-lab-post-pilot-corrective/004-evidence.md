# Harness Lab Post-Pilot Corrective 004 — Verification Evidence

Branch: `plans/harness-lab-post-pilot-correctives`
Baseline reviewed: `2a9fc376707ef14dab4a2f9a27f6aba8ac00dd59`
Verified: 2026-10-21

## 1. The taxonomy

```
$ python3 -c "import json; ..."
read-search-patch-rust             family=read_search_patch      support=supported  protected=False net=none
read-search-patch-python           family=read_search_patch      support=supported  protected=False net=none
compile-debug-rust                 family=compile_debug          support=supported  protected=False net=none
background-job-wait                family=background_job         support=supported  protected=False net=none
repeated-distinct-tool-calls       family=repeated_tool_calls    support=supported  protected=False net=none
repeated-identical-failure-stall   family=repeated_tool_calls    support=supported  protected=True  net=none
delegated-nonconflicting-workstreams family=delegation           support=deferred   protected=False net=none
mcp-lsp-integration-isolation      family=integration            support=deferred   protected=False net=none
```

All eight cases carry `schema_version: 1`, `group_id`, `title`, `prompt`,
`verifier{kind,command,cwd}`, `verifier_scope`, `budget`, `abort`,
`allowed_tools`, `network`, `protected`, `support` and `deferral_reason`.

## 2. The loader's offline tests

```
$ python3 -m unittest discover -s dev/harness_lab/tests -p 'test_autonomy.py'
Ran 52 tests in 1.436s

OK
```

| Requirement | Test |
|---|---|
| The manifest loads, is versioned, and its digest equals `suite_digest()` | `ShippedManifestTests` |
| Every supported family has at least one case | `test_manifest_covers_every_supported_family_with_at_least_one_case` |
| Every supported case declares an independently executable verifier | `test_supported_cases_declare_an_independently_executable_verifier` |
| A deferred case carries a reason and a supported case does not | `test_deferred_cases_carry_a_reason_and_supported_cases_do_not` |
| Supported and deferred partition the suite | `test_supported_and_deferred_partition_the_suite` |
| A nested candidate-editable scope is refused | `IsolationTests` |
| An exactly-equal candidate-editable scope is refused | `IsolationTests` |
| The prefix comparison is path-segment aware | `IsolationTests` |
| The shipped suite is refused when `dev/` is candidate-editable | `IsolationTests` |
| A missing `network` field is an error, not a silent `"none"` | `ValidationTests` |
| An unknown field, a duplicate id, an unsupported schema version are refused | `ValidationTests` |
| An invalid budget is refused | `ValidationTests` |
| A deferred case is excluded from `supported_cases()` and refused by name | `DeferredCaseTests` |
| Changing the manifest bytes changes the digest | `DigestTests` |
| The loader imports no network or subprocess module | `ModuleBoundaryTests` |

## 3. The materialized reference case

`read-search-patch-python` is written out end to end under
`dev/autonomy_fixtures/read-search-patch-python/`.

### The verifier fails while the task is unsolved

```
$ cd dev/autonomy_fixtures/read-search-patch-python
$ python3 -m unittest discover -s verifier
FAIL: test_an_empty_record_is_one_empty_field (test_parser_contract.ParserContractTests...)
AssertionError: Lists differ: [] != ['']
----------------------------------------------------------------------
Ran 6 tests in 0.011s

FAILED (failures=2)
```

A verifier that passed on the broken baseline would prove nothing, so this is
the property that matters.

### The verifier passes once the defect is repaired

```
$ cp -R dev/autonomy_fixtures/read-search-patch-python $TMP/case
$ # remove the trailing-empty-field truncation from fixture/src/parser.py
$ cd $TMP/case && python3 -m unittest discover -s verifier
----------------------------------------------------------------------
Ran 6 tests in 0.004s

OK

$ cd $TMP/case/fixture && python3 -m unittest discover -s tests
----------------------------------------------------------------------
Ran 4 tests in 0.000s

OK
```

### The verifier refuses an edited frozen file

```
$ # append a line to fixture/tests/test_parser.py
$ python3 -m unittest discover -s verifier
FAILED (failures=1)
```

Weakening the provided test is not a way to pass.

### These are standing tests

| Property | Test |
|---|---|
| The verifier fails while the task is unsolved | `test_the_verifier_fails_while_the_task_is_unsolved` |
| The verifier passes once the defect is repaired | `test_the_verifier_passes_once_the_defect_is_repaired` |
| The verifier refuses an edited frozen file | `test_the_verifier_refuses_an_edited_frozen_file` |
| The verifier scope is outside the candidate-editable trees | `test_the_verifier_scope_is_outside_the_candidate_editable_trees` |

## 4. The full harness suite

```
$ python3 -m unittest discover -s dev/harness_lab/tests
..................................................................................................................................................................................
----------------------------------------------------------------------
Ran 178 tests in 6.043s

OK
```

The suite was `Ran 72 tests ... OK` before this line of work.

## 5. Workspace guards

`scripts/verify.sh quick` exits 0 — see `001-evidence.md` §6 for the guard
list. `cargo fmt --check --all` is clean and `cargo clippy -p codegg
--all-targets -- -D warnings` finishes with no warning.

## 6. What was NOT run, and why

- **No live provider call.** No approved budget and no provider-authoritative
  quota exists to verify. The plan's own instruction for this case is to skip
  the live calls and close only the offline preparation as conditional.
- **No Task 2 pair and no sustained 30–60 minute scenario.** Both need the live
  authorization and C003's accepted-defect receipt.
- **No candidate qualification against the new taxonomy.** The C002 score
  contract and hardened comparator are ready; a live candidate run is not.
