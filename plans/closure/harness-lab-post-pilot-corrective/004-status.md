# Harness Lab Post-Pilot Corrective 004 — Closure Status

Status: **conditionally closed — every authoring and live condition is
discharged; the taxonomy's stall-stop control was never exercised by a live
trial, and `ProviderConfig.chunk_timeout` remains a dead config key**

Source implementation plan:

- `plans/implementation/harness-lab-post-pilot-corrective/004-sustained-agent-benchmark-qualification.md`

Source subsystem roadmap:

- `plans/subsystems/harness-lab-post-pilot-corrective-addendum.md`

Repository baseline reviewed: `2a9fc376707ef14dab4a2f9a27f6aba8ac00dd59`

## 1. Executive finding

The live half is no longer blocked on authorization: the operator approved
four low-cost/free routes, three controller-enforced budget policies were
admitted before any provider request, Task 2 was solved against the frozen
fixture, and a 24-trial sustained scenario ran for 34.9 minutes inside its
declared caps.

Two things about the closure changed since the live half ran, and they are now
named precisely rather than folded into one another:

- **The provider pair ran.** The frozen Task 2 fixture completed on one binary
  (`102e9582…`, source `5dda961ce`) under one policy with three allowed routes,
  each passing the fixture's independent verifier; the receipts are preserved at
  `plans/closure/harness-lab/evidence/pilot-pair-ranges-20261022.json`. Getting
  there took a diagnosis: the routes the operator originally named could not
  all be used by the CodeGG **subject**, because `longcat-2.5-preview-free` is
  withdrawn from the provider's own catalog and its upstream answers HTTP 402,
  while `beam-free` and `minimax-m3.1-flash-free` are served and free but
  absent from the shared provider profile's reviewed wire map, so CodeGG fails
  them closed at `wire_unresolved`. That audit is preserved at
  `plans/closure/harness-lab/evidence/route-audit-20261022.json` and the fix it
  names — two entries in a map this repository does not own — is recorded as a
  finding about the shared profile, not as a harness defect.
- **The autonomy claim's dependencies are all discharged, and the comparison
  ran.** C003's accepted-defect receipt, which was the third dependency, is
  discharged: C003 closed with an accepted candidate and no regression
  (`plans/closure/harness-lab-post-pilot-corrective/003-status.md` §1). The
  five unmaterialized verifier bodies are authored, and the paired
  baseline/candidate comparison over the stable groups ran live against the
  autonomy taxonomy on both binaries. Its verdict is **`rejected`**, and
  that is a finding rather than a failure of the instrument: **both binaries
  solved all five task cases**, so the taxonomy had no headroom to show a
  difference. The reason is that the taxonomy exercises the **ACP** path,
  where C003's headless-adapter fix does not apply. What the
  comparison leaves open is narrower and more precise than before: the
  taxonomy's **stall-stop control was never exercised** by a live trial,
  because the agent investigated the missing file instead of repeating the
  identical failing read. The control demonstrated nothing, and it is
  recorded as demonstrating nothing.

Four genuine harness defects were found and fixed by the live runs
themselves: the lane daemon leaked (31 survivors after 24 trials); the macOS
lane socket path exceeded the platform's `sun_path` limit; the verifier ran
in the subject's isolated environment, where a rustup shim resolves no
toolchain; and the reference case's loader resolved the case root only from
its own location, so it could not find the fixture in a live lane.

## 2. Requirement-to-evidence matrix

| Plan requirement | Evidence | Status |
|---|---|---|
| §4-A: a versioned opt-in manifest with the six separately scoped families | `dev/harness_lab/fixtures/agent_autonomy/cases.jsonl`, schema version 1, eight cases across all six families | Met |
| §4-A: every case has an independently executable verifier | argv, `cwd` and `verifier_scope` are frozen for every case; **all six** supported cases have a materialized body, each proven to fail unsolved, pass solved and refuse an edited frozen file | Met |
| §4-A: every case has a resource/usage budget | `budget` with `max_provider_requests`, `max_tool_calls`, `max_wallclock_seconds`, `max_cost_usd` | Met |
| §4-A: abort/timeout criteria | `abort` with `timeout_seconds` and `stall_seconds`; the loader refuses a stall longer than the timeout | Met |
| §4-A: a clear expected allowed tool surface | `allowed_tools`, non-empty and required | Met |
| §4-A: a representative agent prompt | `prompt` on every case | Met |
| §4-A: a frozen group/suite digest | `suite_digest()` is the SHA-256 of the manifest bytes; `group_id` per case | Met |
| §4-A: protection against changing its own verifier | `verify_isolation()` refuses a `verifier_scope` under any candidate-editable prefix, exact equality included | Met |
| §4-A: a test-only / no-external-network default | `network` is a required field; `"none"` on every case; a missing field is an error | Met |
| §4-A: an internet task must disclose its endpoints | The loader requires an object with a non-empty `endpoints` list; no case speculates about endpoints | Met |
| §4-A: worktree/agent delegation only where the milestones are closed and live-capable | `delegated-nonconflicting-workstreams` is **deferred**, with the registry state cited in the reason | Met |
| §4-A: MCP/LSP only when the configuration can be isolated and its data verified | `mcp-lsp-integration-isolation` is **deferred**, with the isolation gap cited | Met |
| §4-A: a protected negative control for the repeated-call family | `repeated-identical-failure-stall` is `protected: true` and shares `group_id` with the distinct-call task | Met |
| §6 offline: content-addressed fixture loading | `suite_digest()` and `load_autonomy_suite()` agree; a changed byte changes the digest | Met |
| §6 offline: hidden verifier isolation | `verify_isolation()` plus segment-aware prefix matching | Met |
| §6 offline: a fake success without a source change | The materialized verifier re-derives the contract and refuses an edited frozen file; it fails on the unsolved baseline | Met |
| §6 offline: a wrong source SHA | The loader's manifest digest is the source identity; a changed manifest changes the digest | Met |
| §4-B: Task 2 pair under an approved budget | Task 2 solved on `muse-spark-1.3-contributor`: terminal `completed`, 11 tool calls, verifier 11/11, 1 provider request, cost unobservable; the pair was then completed on one binary with three allowed routes (`plans/closure/harness-lab/evidence/pilot-pair-ranges-20261022.json`) | **Met** |
| §4-C: a 30–60 minute sustained scenario | 24 live trials, 34.9 min (2 093 626 ms), 0 unattempted, 11 solved / 13 stopped, inside 24 requests / 256 999 tokens | Met |
| §4-D: a paired baseline/candidate benchmark comparison | A live paired run over the six supported cases on the baseline binary and C003's accepted candidate, one private lane per case per side, scored by the C002 contract (`plans/closure/harness-lab/evidence/autonomy-comparison-20261022.json`). The verdict is **`rejected`**: correctness is 1.0 on both sides, reliability 1.0 on both sides, and the advisory duration moved the wrong way — both binaries solved all five task cases, so the taxonomy had no headroom to show a difference, because it exercises the ACP path where C003's headless-adapter fix does not apply | Met — the comparison ran and decided; it did not decide in the candidate's favour |
| §7: the operator can run the documented commands | `dev/harness_lab/README.md` | Met |

## 3. Implementation and safety evidence

### 3.1 The taxonomy

`dev/harness_lab/fixtures/agent_autonomy/cases.jsonl` is one JSON object per
line, schema version 1, eight cases across the six families:

| Case | Family | Support | Protected |
|---|---|---|---|
| `read-search-patch-rust` | `read_search_patch` | supported | no |
| `read-search-patch-python` | `read_search_patch` | supported | no |
| `compile-debug-rust` | `compile_debug` | supported | no |
| `background-job-wait` | `background_job` | supported | no |
| `repeated-distinct-tool-calls` | `repeated_tool_calls` | supported | no |
| `repeated-identical-failure-stall` | `repeated_tool_calls` | supported | **yes** |
| `delegated-nonconflicting-workstreams` | `delegation` | **deferred** | no |
| `mcp-lsp-integration-isolation` | `integration` | **deferred** | no |

The two `repeated_tool_calls` cases share one `group_id`: a task whose calls are
all **distinct** must not be stopped as a repeated-call stall, paired with a
**protected negative control** whose identical repeated failure must stop the
trial as a stall. A fixed terminal match is a control, not an optimization
objective, so the control never counts toward a score.

### 3.2 The two deferred families are deferred, not faked

`delegated-nonconflicting-workstreams` is deferred because the delegated
multi-workstream contract is not closed and live-capable in this branch. The
reason cites the actual registry state: the agent workstream blackboard line
is still active, M005 is active with shared-write unavailable and Q1–Q5
unqualified, M006 and M007 are only ready and not closed, and M008 is blocked
on them. Without a closed worktree-placement and build-admission contract
there is no honest independent verifier for a delegated run.

`mcp-lsp-integration-isolation` is deferred because no isolated per-case MCP
or LSP server configuration with verifiable returned data exists. The harness
does isolate a lane-private config and daemon root, but the in-repo fake LSP
server only exercises the LSP client under `cargo test` and never the
agent-facing `lsp` and `mcp_resource_*` tools, and the external search
provider needs network endpoints this manifest will not disclose speculatively.

Recording these as deferred is the plan's instruction, not a shortcut: "If no
genuine defect exists, the agent must stop and mark live-acceptance evidence
blocked rather than plant a bug."

### 3.3 The loader refuses; it never repairs

`dev/harness_lab/autonomy.py` parses and validates. It rejects an unsupported
schema version, a missing required field, an unknown field, a duplicate
`case_id`, a family outside the six, an unknown `support` mode, a deferred case
without a reason, a supported case carrying one, a non-boolean `protected`, an
empty `allowed_tools`, a non-positive or non-integer budget cap, a negative,
non-finite or boolean cost, a `network` value that is neither `"none"` nor an
object with a non-empty `endpoints` list, a verifier without a real argv
`command`, and an abort window whose stall exceeds its timeout.

Two of these deserve their own note:

- **A missing `network` field is an error.** "No network" is never inferred,
  because the plan requires an explicit disclosure.
- **The loader performs no I/O beyond the manifest.** It imports no
  `socket`, `http`, `urllib`, `requests`, `subprocess` or `asyncio`, and never
  runs a verifier. A test asserts the boundary by AST inspection.

### 3.4 Verifier isolation

`verify_isolation(suite, candidate_editable_prefixes)` refuses a case whose
`verifier_scope` falls under a prefix the candidate may edit, **including
exact equality** — a scope that is exactly a candidate-editable prefix is a
violation too, not only a scope nested below one. The comparison is
path-segment aware, so `crates-extra/` is not under `crates/`.

The shipped scopes live under `dev/autonomy_fixtures/<case_id>/verifier`, which
is outside `dev/harness_lab/`, `plans/` and `tests/` — the trees
`improve.check_scope` already forbids a candidate from changing. The guard
must be run with the same `--allow-prefix` list the candidate patch was
admitted under, before any trial starts.

### 3.5 The materialized reference case

`read-search-patch-python` is written out end to end and is the reference for
the remaining five:

- the fixture holds a small Python package whose `parse_record` drops trailing
  empty fields, contradicting its own stated contract;
- the fixture's provided test fails against that baseline;
- the **independent** verifier re-derives the contract instead of trusting the
  fixture's test, and checks the frozen files' digests so editing the
  provided test is not a way to pass.

Its three properties are proven by standing tests, not asserted:

| Property | Test |
|---|---|
| The verifier **fails** while the task is unsolved | `test_the_verifier_fails_while_the_task_is_unsolved` |
| The verifier **passes** once the defect is repaired | `test_the_verifier_passes_once_the_defect_is_repaired` |
| The verifier **refuses** an edited frozen file | `test_the_verifier_refuses_an_edited_frozen_file` |

## 4. Verification executed

See `plans/closure/harness-lab-post-pilot-corrective/004-evidence.md`.

```
$ python3 -m unittest discover -s dev/harness_lab/tests
Ran 178 tests in 6.043s

OK
```

That count is the one observed at this closure. The suite has since grown with
the C003 score-contract correction, so a re-run reports more tests; C004's
measurements do not depend on the score contract.

## 5. Remaining conditions

1. **A second serving route for the Task 2 pair — discharged.** The pair ran on
   the frozen Task 2 fixture with one binary and one policy, and the receipts
   are preserved at
   `plans/closure/harness-lab/evidence/pilot-pair-ranges-20261022.json`:
   `muse-spark-1.3-contributor` (35.3 s, 9 tool calls, verifier exit 0),
   `mimo-v2.6-flash` (102.0 s, 10 tool calls, verifier exit 0) and
   `deepseek-v4.1-flash` (71.8 s, 20 tool calls, verifier exit 0). The routes
   the original approval named are still subject-routable only where the
   provider's catalog and the shared wire map allow; the route audit
   (`plans/closure/harness-lab/evidence/route-audit-20261022.json`) records
   that, and the pair was completed with the routes the operator extended the
   approval to.
2. **C003's accepted-defect receipt — discharged.** C003 closed with an
   accepted candidate: a genuine, reproducible defect was corrected by an
   authenticated OpenCode proposer and the paired score accepted it with the
   objective `correctness_fix` and no regression
   (`plans/closure/harness-lab-post-pilot-corrective/003-status.md` §1). The
   receipt C004's final autonomy claim depends on therefore exists.
3. **The five unmaterialized verifier bodies — discharged.** All six supported
   cases now have a body under `dev/autonomy_fixtures/<case_id>/`: a `fixture/`
   tree the agent edits and a `verifier/` tree that never lives inside it. Each
   verifier re-derives its contract rather than trusting the fixture's own
   test, and each one is proven by execution to **fail** on the unsolved case,
   **pass** on the repaired case and **refuse** an edited frozen file
   (`dev/harness_lab/tests/test_autonomy_cases.py`). The Rust cases are
   dependency-free crates compiled with `rustc` directly, so the taxonomy
   keeps its `network: "none"` promise and leaves no build artifacts behind.
4. **The paired baseline/candidate comparison over stable groups — discharged
   as an instrument, with a result that is not a win.**
   `dev/harness_lab/autonomy_run.py` runs a taxonomy case as a live agent
   task: it copies the case's `fixture/` into a private lane, keeps the
   verifier **outside** the agent's workspace, records a transcript of what
   the agent actually did, and lets the case's own independent verifier decide
   whether the task was solved. The comparison ran the same six supported
   cases on both the baseline binary and C003's accepted candidate, one
   private lane per case per side, on one approved route and one admitted
   budget; the receipts are preserved at
   `plans/closure/harness-lab/evidence/autonomy-comparison-20261022.json`.

   The score contract's verdict on that pair is **`rejected`**:
   `correctness.pass_rate` is 1.0 on both sides, `reliability.success_rate`
   is 1.0 on both sides, and the advisory `efficiency.duration_ms` moved the
   wrong way (28 661 ms against 54 190 ms). **This is a real finding.** Both
   binaries solved all five task cases, so the taxonomy had no headroom to
   show a difference: C003's accepted candidate fixed the *headless adapters'*
   workspace binding, and the taxonomy exercises the **ACP** path, where the
   workspace was already bound in both binaries. The instrument ran six live
   cases on both sides and decided from real measurements; claiming an
   improvement the numbers do not show would not be an improvement.

   The **protected control was not exercised**. The agent read the missing
   file, the read failed, and the agent then investigated differently and
   correctly reported that the file does not exist — the identical failing
   call was never repeated, so the stall-stop branch of the control was never
   entered. The control demonstrated nothing and is recorded as demonstrating
   nothing, not as a pass. That is the one substantive condition C004 still
   carries.
5. **The `opencode_go` 30 s chunk timeout — discharged.**
   `crates/codegg-providers/src/opencode_go.rs:35` hard-coded a 30 s chunk
   timeout that `openai`, `anthropic` and `google` do not have, and it caused
   9 of the 12 `worklog` failures in the sustained scenario. The cause is now
   understood rather than merely observed: **stream liveness is owned by the
   agent layer**, which already bounds a long stream with
   `STREAM_SETUP_TIMEOUT` (120 s) and `STREAM_IDLE_TIMEOUT` (90 s). A
   provider-level deadline *below* the agent's own idle timeout fires first,
   so the agent's bound never applies and a slow but valid stream is reported
   as a provider error. The provider now imposes **no** per-chunk deadline on
   any of its three surfaces, exactly like its siblings, and a test pins that
   (`opencode_go::chunk_timeout_tests::the_provider_imposes_no_per_chunk_deadline`).

   `ProviderConfig.chunk_timeout` remains declared and unread. That is now
   consistent rather than contradictory — no provider imposes a per-chunk
   deadline, because the agent layer owns the bound — but the key is still a
   dead knob, and removing a public config field is a schema change that this
   closure did not make. It is recorded as a remaining finding, not as a
   pass.

## 6. Planning audit

- No plan file was edited to make this implementation look complete.
- The frozen pilot fixture digests were not changed: `worklog` is
  `5adfbae3d68ea0857a5ebbb212e62333d76aaecd6ef178f3bc3f14d612b12135` and
  `ranges` is `89075ca1e92c7e1b829d5f6db99320d5c7bb437830cd2c6d1840b4b7a1ad0d53`,
  matching the M005 receipts.
- The two families that cannot be honestly verified today are **deferred with
  a precise reason**, not recorded as a pass.
- The live routes were exercised inside admitted, controller-enforced budget
  caps. Where a route reported no cost, the cost is recorded as **null**, not
  zero; no dollar figure was invented for a route that does not report one.
- The provider pair is reported as **three serving routes** on the frozen
  fixture, one binary, one policy, each in its own private lane, each passing
  the fixture's independent verifier. It is recorded with its measurements, and
  the routes the original approval named that the subject cannot reach are
  recorded as a finding about the shared profile rather than as a pass.
- The cleanup evidence is **observed**: the first sustained run reported
  `cleanup_complete: false` with 31 surviving daemons, and the harness defect
  behind it was fixed rather than the observation being dropped.
- The taxonomy's manifest bytes were **not** changed to make the comparison
  look better: the cases, their prompts, their tool surfaces and their
  verifier commands are the frozen ones, and the comparison ran against them.
- The comparison's verdict is reported as it came out. The candidate was
  **not** credited with an improvement the numbers do not show, and the
  protected control is recorded as *not exercised* rather than as a pass.
- The harness-lab Python suite remains wired into `scripts/verify.sh quick`
  and the CI workflow.

## 7. Status of M004 and M005

Both are now **closed**.

- **M004** required a live interrupted qualification → restart → completion
  without duplicate receipts. The mechanism is proven with real SIGTERM/SIGKILL
  subprocesses in C002; C003's live qualification carries the interruption
  proof itself, and C004's compile-correction trace closes M004's remaining
  live-correction condition
  (`plans/closure/harness-lab/004-status.md` §3.1).
- **M005** required a sustained pilot with a two-route provider pair and an
  accepted defect. The sustained pilot ran (24 trials, 34.9 min), the
  accepted-defect dependency is C003's and is discharged, and the provider pair
  closed on the frozen Task 2 fixture with three routes
  (`plans/closure/harness-lab/005-status.md` §5.1).

Neither has been weakened: the parts that ran are recorded with their
measurements, the costs that no route reports stay `null`, and the routes the
subject cannot reach are recorded as a finding about the shared provider
profile rather than as a completed measurement.
