# Harness Lab Post-Pilot Corrective 004 — Closure Status

Status: **conditionally closed — the live half ran; the provider pair and the
autonomy-claim dependency did not**

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

Two things did **not** close, and both are properties of the outside world
rather than of the harness:

- **The provider pair is one route, not two.** `longcat-2.5-preview-free` is
  withdrawn from the provider's own catalog (`https://opencode.ai/zen/go/v1/models`
  no longer lists it) and its upstream answers HTTP 402 "Upstream request
  failed: Endpoint is unavailable." on every attempt, so the frozen Task 2
  pair could not be run on two routes. `muse-spark-1.3-contributor` served the
  task; `longcat` could not. The full route audit — provider catalog, reviewed
  wire map, and what each approved route actually answers — is preserved at
  `plans/closure/harness-lab/evidence/route-audit-20261022.json`: the CodeGG
  **subject** can be routed to exactly one of the four approved routes, because
  the two other free routes are served and free but absent from the shared
  provider profile's reviewed wire map, and that map is owned outside this
  repository.
- **The autonomy claim still depends on C003's accepted-defect receipt.**

Two genuine harness defects were found and fixed by the live run itself: the
lane daemon leaked (31 survivors after 24 trials) and the macOS lane socket
path exceeded the platform's `sun_path` limit.

## 2. Requirement-to-evidence matrix

| Plan requirement | Evidence | Status |
|---|---|---|
| §4-A: a versioned opt-in manifest with the six separately scoped families | `dev/harness_lab/fixtures/agent_autonomy/cases.jsonl`, schema version 1, eight cases across all six families | Met |
| §4-A: every case has an independently executable verifier | argv, `cwd` and `verifier_scope` are frozen for every case; **one** case's verifier body is materialized | **Partial** — five bodies are specified but not authored |
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
| §4-B: Task 2 pair under an approved budget | Task 2 solved on `muse-spark-1.3-contributor`: terminal `completed`, 11 tool calls, verifier 11/11, 1 provider request, cost unobservable | **Partial** — one route served it; `longcat` is withdrawn from the provider's catalog and its upstream is down (HTTP 402), and the other two approved free routes are not routable by the subject, so the pair is not a pair |
| §4-C: a 30–60 minute sustained scenario | 24 live trials, 34.9 min (2 093 626 ms), 0 unattempted, 11 solved / 13 stopped, inside 24 requests / 256 999 tokens | Met |
| §4-D: a paired baseline/candidate benchmark comparison | The C002 score contract and hardened comparator are in place; no live run | **Partial** — mechanics ready, no live run |
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

1. **A second serving route for the Task 2 pair.** `muse-spark-1.3-contributor`
   serves Task 2; `longcat-2.5-preview-free` is withdrawn from the provider's
   catalog and its upstream answers HTTP 402 "Endpoint is unavailable." on
   every attempt, so the pair could not be run on two routes. The two other
   approved free routes are served and cost nothing, but CodeGG's subject
   cannot be routed to them: the shared provider profile's reviewed wire map
   has no entry, so every request fails closed at `wire_unresolved` before a
   request is built — the intended behaviour, and not operator-overridable by
   config. The audit is preserved at
   `plans/closure/harness-lab/evidence/route-audit-20261022.json`. This is an
   upstream and shared-profile condition and is reported as measured.
2. **C003's accepted-defect receipt.** C004's final autonomy claim needs it.
3. **The five unmaterialized verifier bodies.** Their contracts are frozen;
   their bodies are the next authoring step.
4. **The paired baseline/candidate comparison** over stable groups, with the
   C002 score contract doing the decision. No live run against the autonomy
   taxonomy was made.
5. **The `opencode_go` 30 s chunk timeout.** `crates/codegg-providers/src/opencode_go.rs:35`
   hard-codes a chunk timeout that `openai`, `anthropic`, `google`,
   `openrouter` and `azure` do not have, and `ProviderConfig.chunk_timeout` is
   read by no provider at all. It caused 9 of the 12 `worklog` failures in the
   sustained scenario. Fixing it is a provider change, outside this closure.

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
- The provider pair is reported as **one serving route**, because the second
  route's upstream is unavailable. It is not recorded as a completed pair.
- The cleanup evidence is **observed**: the first sustained run reported
  `cleanup_complete: false` with 31 surviving daemons, and the harness defect
  behind it was fixed rather than the observation being dropped.
- The harness-lab Python suite remains wired into `scripts/verify.sh quick`
  and the CI workflow.

## 7. Status of M004 and M005

Both stay **conditionally closed**.

- **M004** required a live interrupted qualification → restart → completion
  without duplicate receipts. The mechanism is proven with real SIGTERM/SIGKILL
  subprocesses in C002; C003's live qualification carries the interruption
  proof itself.
- **M005** required a sustained pilot with a two-route provider pair and an
  accepted defect. The sustained pilot ran (24 trials, 34.9 min) and the
  accepted-defect dependency is C003's; the **two-route pair did not** close,
  because one upstream is unavailable.

Neither has been weakened: the conditional dispositions are carried forward
with the precise remaining conditions named above, and the parts that *did*
run are recorded with their measurements.
