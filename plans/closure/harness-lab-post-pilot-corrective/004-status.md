# Harness Lab Post-Pilot Corrective 004 — Closure Status

Status: **conditionally closed — offline preparation closed; the live half is blocked**

Source implementation plan:

- `plans/implementation/harness-lab-post-pilot-corrective/004-sustained-agent-benchmark-qualification.md`

Source subsystem roadmap:

- `plans/subsystems/harness-lab-post-pilot-corrective-addendum.md`

Repository baseline reviewed: `2a9fc376707ef14dab4a2f9a27f6aba8ac00dd59`

## 1. Executive finding

The offline half of C004 is closed: a versioned, opt-in benchmark taxonomy with
its frozen cases, a strict loader that refuses everything the taxonomy
contract forbids, a verifier-isolation guard, and one fully materialized case
whose verifier is proven to fail while the task is unsolved and pass once the
defect is repaired.

The live half is blocked, and §7 of the plan anticipates exactly this: "If
hard cap cannot be met, **skip live calls** and close only offline fixture
preparation as conditional." A CodeGG provider route exposes **no
provider-authoritative spend quota at all** — the `quota` concept in the
config schema is a remote-node CPU quota, and the provider crate has no quota
surface whatsoever — so the only enforceable caps available are the
controller-enforced ones C001 provides. The Task 2 provider pair and the
30–60 minute sustained scenarios additionally need C003's accepted-defect
receipt, which is itself blocked on the same live-provider authorization.

No live provider call was made. No Task 2 receipt exists. No sustained
scenario was run.

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
| §4-B: Task 2 pair under an approved budget | — | **Blocked** — no approved budget; no provider quota exists to verify |
| §4-C: a 30–60 minute sustained scenario | — | **Blocked** — needs the live authorization and C003's accepted defect |
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

## 5. Remaining conditions

1. **An approved live-provider budget.** The plan requires it for every live
   call. CodeGG exposes **no provider-authoritative spend quota** — the
   `quota` in the config schema is a remote-node CPU quota and the provider
   crate has no quota surface — so the only enforceable caps are the
   controller-enforced ones C001 provides. A live run needs a budget policy
   whose cap is real: a token limit, a provider-request limit, a wall-clock
   limit, or a retry-safe provider quota.
2. **C003's accepted-defect receipt.** C004's final autonomy claim needs it,
   and C003 is blocked on the same authorization.
3. **The five unmaterialized verifier bodies.** Their contracts are frozen;
   their bodies are the next authoring step.
4. **The Task 2 pair** (LongCat/Muse) from identical committed baseline copies,
   with the chosen provider route recorded.
5. **At least one 30–60 minute sustained scenario**, with the independent
   limits C001's guard enforces and the per-lane cleanup proven.
6. **The paired baseline/candidate comparison** over stable groups, with the
   C002 score contract doing the decision.

## 6. Planning audit

- No plan file was edited to make this implementation look complete.
- The frozen 12-case suite and the stored pilot receipts were not changed, and
  `canonical_digest` was not touched.
- The two families that cannot be honestly verified today are **deferred with
  a precise reason**, not recorded as a pass.
- No live provider call was made and no model route was exercised.
- The C001 budget guard was extended so a declared cap is **enforced** during
  a run, not merely declared: the provider-request cap refuses work before it
  starts, and a controller-enforced cost cap now requires an observable
  provider cost, which the OpenCode Go route does not report.
- The harness-lab Python suite remains wired into `scripts/verify.sh quick`
  and the CI workflow.

## 7. Status of M004 and M005

Both stay **conditionally closed**. Neither C003's live acceptance nor C004's
sustained pilot has produced the evidence their original closure records
required, and neither has been weakened: the conditional dispositions are
carried forward unchanged, with the precise remaining conditions named above.
