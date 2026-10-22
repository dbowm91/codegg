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

## 3. The materialized case bodies

`read-search-patch-python` is written out end to end under
`dev/autonomy_fixtures/read-search-patch-python/`, and the five cases that
were specified but not authored are now written out the same way:

| Case | Family | The defect the agent is asked to find | The verifier |
|---|---|---|---|
| `read-search-patch-python` | `read_search_patch` | a record parser drops the record's trailing empty field | re-derives the contract in Python against the candidate's own module |
| `read-search-patch-rust` | `read_search_patch` | a Rust record parser folds trailing empty fields away | compiles the candidate crate with `rustc` and runs a hidden Rust contract test |
| `compile-debug-rust` | `compile_debug` | a compile error (`E0560`) plus three independent logic bugs | compiles with `rustc`, runs the provided test and a hidden contract test |
| `background-job-wait` | `background_job` | a two-minute build that must be backgrounded, waited on, and reported | judges the **transcript**: a `job` call, other work while it ran, a completed trial, and the build's artifact |
| `repeated-distinct-tool-calls` | `repeated_tool_calls` | nothing to fix — the task is to investigate with **distinct** calls | judges the transcript: five distinct calls, three tools, no identical repeat, trial completed |
| `repeated-identical-failure-stall` | `repeated_tool_calls` | **protected control** — a file that does not exist | judges the transcript: the identical failing call really repeated, and the trial was stopped rather than credited |

Every Rust case is a **dependency-free** crate compiled with `rustc` directly —
no `cargo`, no registry, no target directory — so the taxonomy keeps its
`network: "none"` promise and a verification run leaves no artifacts behind.

The three properties below are proven by execution for **every** case, not for
one reference case: the verifier fails while the task is unsolved, passes once
the task is solved, and refuses an edited frozen file.

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

`dev/harness_lab/tests/test_autonomy_cases.py` proves all three properties
for **every** materialized case, by running the case's own verifier in a
throwaway copy:

| Property | Test |
|---|---|
| The verifier fails while the task is unsolved | `test_the_verifier_fails_while_the_task_is_unsolved` |
| The verifier passes once the defect is repaired | `test_the_verifier_passes_once_the_task_is_solved` |
| The verifier refuses an edited frozen file | `test_the_verifier_refuses_an_edited_frozen_file` |
| The verifier scope is outside the candidate-editable trees | `test_the_verifier_scope_is_outside_the_agent_editable_tree` |
| The verifier finds the fixture in the **live lane layout** | `test_every_verifier_finds_the_fixture_in_the_lane_layout` |
| Every manifest case has a materialized body | `test_every_manifest_case_has_a_materialized_body` |

The lane-layout property is a regression guard for a defect the first live
comparison exposed: in a lane the verifier is copied to a private directory
**outside** the agent's workspace, so the case root is somewhere else. A
verifier that resolves the case root only from its own location looks for a
fixture that is not there, and every case fails with "the frozen file is
missing" for a reason that has nothing to do with the agent. The reference
case's loader did exactly that; it now honours `CODEGG_AUTONOMY_CASE_ROOT`
like every other case's loader does.

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
| `beam-free` | no wire mapping at any rev | works — the upstream serves it and it costs nothing; the failure recorded here was a transient 429 |
| `minimax-m3.1-flash-free` | no wire mapping at any rev | works |
| `longcat-2.5-preview-free` | **upstream down** — HTTP 402 "Endpoint is unavailable." and withdrawn from the provider's catalog | — |

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
same task: it is withdrawn from the provider's own catalog and its upstream
answered HTTP 402 "Upstream request failed: Endpoint is unavailable." on every
attempt. **The provider pair is therefore one route, not two.** That is a
property of the upstream, not of the harness, and it is reported as measured.

Re-probed 2026-10-22: `beam-free` now answers normally and costs nothing, and
`longcat-2.5-preview-free` still answers 402. The full route audit — provider
catalog, reviewed wire map, and each route's answer — is preserved at
`plans/closure/harness-lab/evidence/route-audit-20261022.json`. It shows that
the CodeGG **subject** can be routed to exactly one of the four approved
routes: the two other free routes are served by the upstream, and CodeGG
refuses them at `wire_unresolved` because the shared provider profile's
reviewed wire map has no entry for them.

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

### 6.3.1 Why the 30 s deadline was wrong, and what replaced it

The deadline was not merely inconvenient; it **preempted** the layer that owns
stream liveness. `src/agent/provider_turn.rs` already bounds a long stream:
`STREAM_SETUP_TIMEOUT = 120 s` covers the time to the first event and
`STREAM_IDLE_TIMEOUT = 90 s` covers the gap between events. A provider chunk
deadline of 30 s fires first, so the agent's own bound never gets a chance to
apply, and a slow but valid stream is reported as a provider error.

`crates/codegg-providers/src/opencode_go.rs` now imposes **no** per-chunk
deadline, on any of its three surfaces, and a test pins it:

```
$ cargo test -p codegg-providers chunk_timeout
running 1 test
test opencode_go::chunk_timeout_tests::the_provider_imposes_no_per_chunk_deadline ... ok
```

The deadline stays surface-independent — it is simply not applied at the
provider level, and the agent layer's policy governs Chat, Responses and
Messages alike. `ProviderConfig.chunk_timeout` stays declared and unread; no
provider imposes a per-chunk deadline, so the key is a dead knob rather than a
contradiction. Removing a public config field is a schema change this closure
did not make, and it is recorded as a remaining finding.

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

### 6.5 The autonomy taxonomy, run live on two binaries

`dev/harness_lab/autonomy_run.py` runs a taxonomy case as a live agent task.
It copies the case's `fixture/` into a private lane workspace and keeps the
verifier **outside** that workspace, so the agent under test can neither read
the hidden contract nor edit the thing that judges it. It records a
transcript of what the agent actually did — the tool sequence, each call's
argument **digest** and each call's terminal status — and hands that
transcript to the case's own verifier.

```
$ python3 -m dev.harness_lab autonomy-compare \
  --baseline-binary  /private/tmp/c003-baseline-codegg \
  --candidate-binary /private/tmp/c003-accepted-codegg \
  --model opencode_go/muse-spark-1.3-contributor \
  --budget /path/to/budget.json --live-provider
```

Both sides ran the same six supported cases, in the same order, one private
lane per case per side, on one approved route and one admitted budget. The
baseline is the binary **before** C003's accepted candidate; the candidate is
the binary **after** it (`23fceecb…` and `c9f8f9ae…`).

The receipts are preserved at
`plans/closure/harness-lab/evidence/autonomy-comparison-20261022.json`.

### 6.6 What the comparison decided, and what it did not

The score contract's verdict on this pair is **`rejected`**, with
`no_criterion_improved` and no protected violation. The measured values are:

| Criterion | Baseline | Candidate | Direction |
|---|---|---|---|
| `correctness.pass_rate` | 1.0 | 1.0 | equal — both binaries solved all five task cases |
| `reliability.success_rate` | 1.0 | 1.0 | equal |
| `efficiency.duration_ms` | 28 661.0 | 54 189.5 | the candidate was slower (advisory only) |
| `control.no_regression` | 0.0 | 0.0 | equal — see below |

**This is a real finding, not a harness failure.** Both binaries solved all
five task cases, so the taxonomy had no headroom to show a difference.
C003's accepted candidate fixed the *headless adapters'* workspace binding,
and the autonomy taxonomy exercises the **ACP** path, where the workspace was
already bound in both binaries — so the taxonomy does not discriminate on that
defect. The instrument works — it ran six live cases on both sides and
produced a decision from real measurements — and what it says is that the
taxonomy is not sensitive to the defect this candidate fixed. Recording that
is the point of running the comparison; claiming an improvement that the
numbers do not show would not be.

**The protected control was not exercised.** `control.no_regression` is 0.0
on both sides because the control's own verifier did not confirm the trial,
and the transcript says why: the agent read `fixture/absent-report.txt`, the
read failed, and the agent then **investigated differently** — it listed the
directory and read another file — and correctly reported that the file does
not exist. The identical failing call was never repeated, so the stall-stop
branch of the control was never entered. The control therefore demonstrated
nothing, and it is reported as demonstrating nothing rather than as a pass.

That is the honest reading of the taxonomy's control: it guards against a
harness that lets an agent spin on an identical failing call and then credits
the impossible task. This run did not put the harness in that situation.

### 6.7 Two more harness defects the live run exposed

1. **The lane's socket path.** The first comparison nested its lanes as
   `<lane>/baseline/<case id>/…`, and a 31-character case id pushed the
   daemon control socket past the platform's `sun_path` limit. The failure
   surfaced as an opaque `daemon child exited` error. The runner now refuses
   a lane whose socket path would not fit, with the reason, and the lane
   directory is named by position rather than by case id.
2. **The verifier's environment.** The lane deliberately overrides `HOME` and
   `CARGO_HOME` so the subject under test cannot see the developer's
   registry or credential store — but a rustup shim with an overridden `HOME`
   resolves no default toolchain, and every Rust case failed with "rustup
   could not choose a version of rustc". The **subject** is isolated; the
   **verifier** is harness code and runs in the host environment. The runner
   now builds the verifier's environment from the host's, with only the case
   root and the transcript added.

## 7. What was NOT run, and why

- **No complete provider pair.** `longcat-2.5-preview-free`'s upstream is
  unavailable (HTTP 400), so the Task 2 pair could not be run on two routes.
- **The protected control's stall-stop branch was not exercised.** The agent
  investigated the missing file instead of repeating the identical failing
  read, so the taxonomy's stall-stop capability remains unproven by a live
  trial. This is the one substantive condition C004 still carries.
- **`ProviderConfig.chunk_timeout` is still a dead key.** No provider reads
  it, and none imposes a per-chunk deadline, so it is consistent rather than
  contradictory — but removing a public config field is a schema change this
  closure did not make.
