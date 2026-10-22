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
Ran 232 tests in 28.659s

OK
```

The suite was `Ran 72 tests ... OK` before this line of work. The count above
is the one observed at this closure; the suite has since grown with the C003
score-contract correction, so a re-run reports more tests. Nothing in C004's
measurements depends on the score contract — it measures agent behaviour over
the sustained scenario and does not score candidates.

## 5. Workspace guards

`scripts/verify.sh quick` exits 0 — see `001-evidence.md` §6 for the guard
list. `cargo fmt --check --all` is clean and `cargo clippy -p codegg
--all-targets -- -D warnings` finishes with no warning.

## 6. The live half

The live authorization arrived: the operator approved four low-cost/free
routes — `longcat-2.5-preview-free`, `muse-spark-1.3-contributor`,
`minimax-m3.1-flash-free` and `beam-free` — and three controller-enforced
budget policies were admitted before any provider request.

### 6.1 What each route can and cannot do

| Route | As a CodeGG subject | As an OpenCode proposer |
|---|---|---|
| `muse-spark-1.3-contributor` | **works** — live ACP turn completed | works |
| `beam-free` | no wire mapping at any rev | proposal **failed mid-run** with a retryable `429` "Upstream request failed: Endpoint is unavailable." |
| `minimax-m3.1-flash-free` | no wire mapping at any rev | works |
| `longcat-2.5-preview-free` | **upstream down** — HTTP 400 "Endpoint is unavailable." | — |

`codegg models --provider opencode_go` lists 27 bare ids. The shared
`eggpool-provider-profile` carries 30 `model_wire` entries and maps
`longcat-2.5-preview-free` → `openai_chat_completions` and
`muse-spark-1.3-contributor` → `openai_responses`. `minimax-m3.1-flash-free`
and `beam-free` are absent from that profile, and the pinned profile revision
is identical at eggpool `main`, so bumping the pin cannot produce them.
`https://opencode.ai/zen/go/v1/models` returns no rate-limit headers, so there
is still **no provider-authoritative quota** to verify: only the
controller-enforced caps exist.

### 6.2 Task 2 — one route of the pair

Task 2 (`ranges`) was solved by `muse-spark-1.3-contributor` against the
frozen fixture, whose digests match the M005 receipts (`worklog`
`5adfbae3…`, `ranges` `89075ca1…`):

- terminal `completed`, 11 tool calls, `ranges.py` only, no protected file
  changed;
- the independent verifier exited 0 with 11/11 tests passing;
- usage: input 23 700, output 322, with a `turn_id`; 45.4 s; 1 provider
  request; **cost unobservable** on this route, so no dollar figure is claimed.

The run was reproduced twice. `longcat-2.5-preview-free` could not serve the
same task: the upstream answered HTTP 400 "Upstream request failed: Endpoint
is unavailable." on two separate attempts. **The provider pair is therefore one
route, not two.** That is a property of the upstream, not of the harness, and
it is reported as measured.

### 6.3 The sustained scenario

24 live trials, **34.9 min** (2 093 626 ms), 0 unattempted, 11 solved and 13
stopped, inside a budget of 24 provider requests / 256 999 tokens with the cost
recorded as `null` rather than zero.

- `ranges`: 8/8 clean trials completed.
- `worklog`: 3/12 completed. The 9 failures were `provider error: stream
  error: stream chunk timeout`.
- Two `cancel` and two `abrupt_acp_drop` fault injections behaved as designed.
- `cleanup_complete` was **false**: 31 lane daemons survived the scenario.

The 9 `worklog` failures reproduce M005's native turn failure. The cause is
`crates/codegg-providers/src/opencode_go.rs:35`, `CHUNK_TIMEOUT =
Duration::from_secs(30)`: `opencode_go`, `openai_compatible` and
`opencode_zen` hard-code a 30 s chunk timeout while `openai`, `anthropic`,
`google`, `openrouter` and `azure` pass `None`. `ProviderConfig.chunk_timeout`
is documented and read by **no** provider, so the knob is dead.
`ProviderError::Stream` maps to `RetryDisposition::Transient` and is retryable,
but `src/agent/provider_turn.rs:248-283` refuses to replay after visible
output has been produced, so the stall is terminal.

### 6.4 The daemon leak, and its fix

The 31 surviving daemons were a harness defect, not a product one: the live
pilot closed only the ACP client and left the lane's daemon running. The pilot
now stops the lane daemon after every trial and records a failure to stop as
`daemon_stop_error` instead of swallowing it. A re-run of the scenario left **0
survivors**.

A second macOS-only harness defect surfaced on the way: the lane lived under
the system temporary directory, and the daemon socket path exceeded the
platform's `sun_path` limit, so the lane failed with `Failed to bind daemon
socket: path must be shorter than SUN_LEN`. The lane root now lives under
`~/.codegg-harness-lanes/<short>` and the limit is asserted at 96 bytes.

## 7. What was NOT run, and why

- **No complete provider pair.** `longcat-2.5-preview-free`'s upstream is
  unavailable (HTTP 400), so the Task 2 pair could not be run on two routes.
- **No candidate qualification against the new taxonomy.** The C002 score
  contract and hardened comparator are ready; a live candidate run against the
  autonomy taxonomy is not.
- **Five verifier bodies remain unmaterialized.** Their contracts are frozen;
  their bodies are the next authoring step.
