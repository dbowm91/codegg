# Agent Workstream Blackboard M008 — Isolated S0/S1 Results

Evidence status: exploratory partial result; M008 remains active and is not
strictly closed. This record includes a completed frozen shared-interface S0
and one completed isolated S1 pair; it does not establish a repeatable
effectiveness advantage.

Source plan: `plans/implementation/agent-workstream-blackboard/008-comparative-trajectory-qualification.md`
CodeGG source: `a0d7acef8e428d55971514addf685741a25ce047`
Fixture baseline: `f5e27d388255726b0b85226a144fdcce2e91b73a`
Machine-readable measurements: `plans/closure/agent-workstream-blackboard/008-isolated-results.json`

## 1. Protocol

The fixed task implemented the same capped exponential retry-delay contract in
a dependency-free Rust crate and Python package. Each package had five frozen
contract tests. No trial changed tests or dependencies. S0 used one CodeGG
`build` agent for both packages. S1 used two CodeGG `build` invocations at the
same baseline, each in its own linked Git worktree, one package per child;
changes were integrated sequentially and tested again.

All runs used CodeGG 0.1.0 built from the source above with Rust 1.89 for
`aarch64-apple-darwin`, provider/model `opencode_go/gpt-6-luna`, yolo approval,
and a requested `workspace-write` sandbox. The per-invocation output budget
was the same; CodeGG `exec` offered no aggregate token-budget field. There was
no provider seed. Trial order was S0-A, S0-B, S1-A, an S2 availability probe,
then S1-B. This is a small, unrandomized measurement.

The process environment was macOS 26.6.2, 24 GiB RAM, running x86_64 under
Rosetta; the fixture tests used Cargo/Rust 1.98.1. Each headless CodeGG run
logged that no workspace root was available for the requested
`workspace-write` profile. The fixture edits were confined to `/tmp` in the
observed Git diffs, but effective OS sandbox containment was not established.
This makes the runs exploratory evidence rather than a complete M008
qualification.

## 2. Trajectory results

| Trial | Outcome | Agent tokens | Agent/process time | Peak RSS | Integrated result |
|---|---|---:|---:|---:|---|
| S0-A | One agent completed both modules | 14,921 | 71.20 s | 119,635,968 B | Rust 5/5 and Python 5/5 passed; evaluator reran both suites |
| S0-B | One agent completed both modules | 14,784 | 47.67 s | 119,603,200 B | Rust 5/5 and Python 5/5 passed; evaluator reran both suites |
| S1-A | Rust child completed; Python CLI did not return within 300 s | 12,626 reported; Python unknown | 308.37 s until Python client termination | 125,681,664 B Rust; 117,850,112 B Python | Evaluator recovered the staged Python edit, committed it, and explicitly integrated both children; integrated Rust 5/5 and Python 5/5 passed |
| S1-B | Both isolated children completed | 27,140 total | 79.01 s to slower child; about 1.2 s for merge and integrated tests | 221,085,696 B, sum of child individual peaks | Both child suites passed; sequential integration had no conflict; integrated Rust 5/5 and Python 5/5 passed |

S0-A and S0-B results are commits `91c2179f` and `32c48d8d`. S1-A's Rust
child committed `c0672fa7`; the Python client timed out without a final JSON
result after staging its edit. The evaluator committed that edit as
`e1d58a52` after the timeout and produced integrated commit `f906bccd`. S1-B
child commits are `32d565ed` (Rust) and `3518ed73` (Python), integrated at
`0748ecc8`. Full SHAs and per-child measurements are in the JSON file.

The completed S1-B trial used 82.7% more reported tokens than the mean of the
two S0 trials. Its approximately 80.2 s total was about 35% longer than the
S0 mean of 59.4 s. This is one successful S1 repetition against two S0
repetitions, and S1-A required timeout recovery. It does not establish a
repeatable performance difference. Peak memory for S1 is the sum of the two
children's separate maxima, not a sampled concurrent process-tree peak.

The S2 headless probe returned without starting children because tool
discovery exposed no `task.spawn_many` or `wait_group` action. No substitute
coordination was used. S3 was therefore not run. A separate TUI availability
probe failed before any M008 prompt was submitted and is excluded from the
measured trials.

## 3. Cargo target-directory contention

Two cold, offline `cargo check --locked -p codegg-core` processes were run
with two Cargo jobs each against the exact CodeGG source above. Both arms
passed:

| Target layout | Wall | User CPU | System CPU | Time-reported max RSS | Target disk |
|---|---:|---:|---:|---:|---:|
| Two Cargo processes, one shared target | 211.23 s | 286.09 s | 44.25 s | 1,088,335,872 B | 1,159,948 KiB |
| Two Cargo processes, separate targets | 233.69 s | 630.37 s | 94.17 s | 1,435,226,112 B | 1,167,808 KiB each |

The shared-target process waited on Cargo's build-directory lock and reused
the first process's completed artifacts. Separate targets compiled
independently, took 10.6% longer end to end, used 2.19 times the combined CPU
time, and occupied about twice the target disk. These are single cold runs;
the time-reported RSS is not a sampled sum of concurrent process memory.
This measurement does not qualify a shared checkout, does not establish a
general Cargo policy, and does not change M005's gate.

## 4. Limits and next work

This task only exercised independent files and clean sequential integration.
There is no live evidence here for S2 peer observations, S3 claims, coupled
Rust API/schema changes, overlapping-path recovery, prerequisite chains,
branch drift, reviewer/producer coordination, interrupted build recovery, or
S4 shared-checkout execution. The existing M009 and M010 closures remain
deterministic correctness evidence; they do not replace the missing live
comparison. The previous S0/S1 pilot in `008-pilot-results.md` remains a
separate experiment and is not pooled with these trials.

No effectiveness improvement is established. M008 remains active. A valid
S2/S3 run needs an execution context that exposes the production TaskTool and
its group-scoped peer/claim surfaces, plus a verified workspace root for the
requested sandbox. Complete the remaining task strata and repeat S1 after the
timeout before writing the M008 strict closure record. S4 remains unavailable
pending M005. No downstream plan is unblocked by these partial results.

## 5. Live headless and Darwin terminal sandbox smoke (2026-10-16)

Using the source-built arm64 binary, an isolated daemon/data root, a resolved
temporary project registration, and a managed worktree session, a harmless
provider-backed `headless` turn completed. Provider metadata and the model
catalog were staged into the trial database by opaque reference only; no
credential material was read or copied.

The first terminal-tool smoke wrote a unique probe under `/tmp` despite the
session's `workspace_write` policy. Source tracing found that the legacy
`terminal` tool did not propagate the tool execution authorization or build a
required sandbox request. The file was removed. After the corrective change
at source commit `473579066803469e130b21d3daf2bf645658a115`, focused terminal
tests passed 5/5 and the sandbox-policy and execution-ownership guards passed.
A fresh daemon-backed terminal call then wrote/read/deleted a temporary file
in the managed worktree and received `Operation not permitted` for the `/tmp`
write; a host check confirmed the outside probe was absent. The observed limitation is
Darwin Seatbelt plus the tested terminal path/session root only. It is not
shared-checkout or all-writer evidence.

No provider-backed comparative trial has yet started against this corrected
source. S0 and a multi-agent isolated arm must still use the frozen
Rust/Python shared-interface task. The pilot and earlier S0/S1 results remain
separate and are not pooled.

## 6. Frozen shared-interface workload v1 (2026-10-16)

The new fixture at
`plans/closure/agent-workstream-blackboard/008-shared-interface-fixture/`
contains 17 frozen files. Its relative-path/content SHA-256 stream is
`b0f0ff1b3939c95a2113934e3669d26fee403c37ca302d28759ab3b78d864bf1`. The
Rust starter baseline is 0/7 tests and the Python starter baseline is 0/7
tests, with failures reaching the intentional TODO implementation points.
These local evaluator baselines are not coding-agent trials. No provider-backed
trial has started. Recompute the tree digest before each trial and reject any
copy whose digest differs.

## 7. Provider-backed shared-interface follow-up (2026-10-16)

Fixture v1 was copied to an isolated repository with a resolved repository
binding and launched through the source-built daemon-backed `headless` path.
The S0 `build` agent completed both packages in 171.14 seconds. Its final
Rust suite passed 7/7, Python suite passed 7/7, and Rust-produced JSON passed
the Python consumer's golden check. The agent initially had a Python UTC
validation failure, corrected the implementation, and reran the evaluator
successfully. This is a completed S0 outcome, with the initial failure retained
as part of the trajectory. It does not provide a comparison without a
completed multi-agent arm.

S1 orchestration attempts after the M011 repository-identity correction
created children with separate managed worktree IDs and the resolved
repository ID. The latest group (`7fa853fe-0c67-4fcd-968f-93ecb923d8db`)
contained one Rust producer and one Python consumer. Both journals ended at `safe_boundary: before_provider_turn`. No provider usage or
completion evidence followed. The retained evidence cannot distinguish a request
not submitted from a provider request that stalled before completion. The parent headless turn returned before the child
group completed. Both child jobs were cancelled through normal scheduler
`JobCancel`, and no changes were integrated. These are incomplete
orchestration/provider attempts, excluded from success and performance
denominators. Session/run/worktree identifiers and trial data are in
`008-isolated-results.json`.

M011 (`plans/closure/agent-workstream-blackboard/011-status.md`) records the
corrective that enabled child worktree allocation: root `TurnRunInput` now
carries the resolved repository identity from the session workspace binding.
The S1 stalls remain unresolved and do not demonstrate whether multi-agent
work is effective. M008 remains active; S2/S3 and the other task strata remain
open. No effectiveness improvement is claimed.

## 8. Same-session group continuation finding (2026-10-16)

A bounded retry created group
`ab10021a-82a8-4c43-8120-3286c54408f2` with two isolated child worktrees and
the resolved repository identity on both tasks. The parent turn performed six
`wait_group` calls; each wait is capped at 30 seconds. The parent turn ended
with the group still running. A later headless turn in the same durable session
then received `group actor is not authorized` for `wait_group`: turn-owned
groups require the exact creating turn ID even though the same session is
authorized to submit a new turn. The durable group remained running and could
not be resumed from the later turn.

M012 (`plans/closure/agent-workstream-blackboard/012-status.md`) closes this bounded lifecycle gap. The creating turn remains provenance while same-session top-level turns can access group lifecycle operations; different sessions and child run actors remain unauthorized. Focused service coverage passes. The old daemon was restarted on the corrected build; recovery marked its children interrupted, and a later headless turn in the same session successfully inspected the recovered group.

## 9. M012 daemon continuation and fresh S1 retry (2026-10-16)

A new S1 group (`ff40bb51-6960-4a67-a01d-a0e6b01435ec`) was launched through
the corrected isolated daemon. Its Rust producer and Python consumer have
distinct managed worktrees and the same resolved repository identity. After
the parent turn ended, later headless turns in the same durable session
successfully invoked `wait_group`; both returned without an authorization
error and observed the two child runs still running. This verifies that a
live group can be observed across turns on the corrected daemon.

After roughly ten minutes, both child runs still lacked terminal results or
journal progress beyond `safe_boundary: before_provider_turn`; the managed
worktrees contained no edits. The group was cancelled through
`task.cancel_group`. Its children remained `cancelling` while the provider
calls unwound, so the isolated daemon was stopped and restarted; scheduler
recovery then marked both children `interrupted` and returned both clean
worktrees to `ready`. The captured usage rows are session scoped and include
parent follow-up turns, so they cannot be attributed to child progress.

This retry is classified incomplete and excluded from S1 success and
performance denominators. It provides live evidence for same-session group
continuation and normal cancellation, but no child completion or effectiveness
result. M008 remains active; S2/S3 and the remaining acceptance strata remain
open, and S4 remains gated on M005.

## 10. Single-child runtime diagnostics (2026-10-16)

Two read-only `explore` child diagnostics were run through the same
daemon-backed TaskTool path. One standalone child remained running after 30
seconds; a later top-level turn could not call run-scoped `task.wait` because
run-control remains bound to the originating turn. A one-member group then
remained running across two same-session `wait_group` calls over about 90
seconds. Its journal reached seven `safe_boundary: before_provider_turn`
events, then stopped advancing. Normal group cancellation was accepted, and
isolated-daemon recovery marked the child interrupted. No source edits or
worktree mutations occurred.

Source tracing shows this boundary is recorded before AgentStart hooks and
request preparation in `src/agent/loop.rs`; it does not establish that the
provider stream was invoked or that a tool call began. The diagnostics
therefore reproduce a child-run stall but do not isolate provider streaming
from hooks, request preparation, or tool handling. They are not benchmark
trials. Section 11 adds provider-attempt journal milestones and records the
subsequent completed Rust/Python S1 pair. Tool-execution phase tracing remains
useful for localizing future stalls. No performance conclusion follows from
these diagnostics.

## 11. Approval-mode propagation and completed isolated S1 (2026-10-16)

Phase-level provider journal milestones identified the earlier child-run
stall: delegated children ran under `Interactive` even when the parent
headless session had explicitly selected `Yolo`. A child `git_query status`
therefore entered `permission_pending` in its synthetic session, where a
headless parent could not answer the request. The durable subagent payload now
carries the approval mode from the authorized parent TaskTool invocation into
the scheduled child loop. Historical payloads without the field default to
`Interactive`. This changes only escalation handling: the child retains its
own agent tool/path rules and the independently narrowed sandbox ceiling.
Provider-attempt start/success/failure milestones were also added so future
stalls can be localized without inferring provider activity from the earlier
`before_provider_turn` boundary.

The correction was built into the isolated arm64 daemon. A bounded `build`
child successfully ran `git_query status` and read the fixture under the
parent's `Yolo` mode. A fresh two-child S1 pair then completed from the frozen
fixture digest `b0f0ff1b3939c95a2113934e3669d26fee403c37ca302d28759ab3b78d864bf1`:

| Arm | Run | Worktree | Result | Provider attempts | Elapsed |
|---|---|---|---|---:|---:|
| S1 Rust producer | `cda79ddf-6fdc-4bb5-8793-ce7bd85fdafc` | `cc2c2851-3e08-404a-9c9d-0249c6c35aef` | Completed; Rust workspace tests 7/7 | 23 started, 23 succeeded | 134.29 s |
| S1 Python consumer | `5913d600-3234-404b-94d6-e70ee0e3fc57` | `351ea5e1-2821-4ae5-a1c9-634673afa95b` | Completed; Python tests 7/7 | 22 started, 22 succeeded | 129.18 s |

Group `ecfc4e68-c4fc-4985-a1b0-d9063287858a` accepted both children and
completed with two accepted, zero rejected. Both worktrees had the same
resolved repository identity and a shared immutable base, while each child
owned a separate managed checkout. A disposable evaluation copy combined only
the Rust producer outputs and Python consumer outputs; its fixture matched the
frozen digest. On that combined result, `cargo test --locked --workspace`
passed all 7 integration tests, `python3 -m unittest discover -s python/tests
-v` passed all 7 tests, and `python3 scripts/check_cross_language_golden.py`
passed. The integrated result was evaluated only; child changes were not
applied to the CodeGG source checkout.

The group completed in 135.18 seconds from acceptance to terminal completion.
The parent session contains 147 provider usage rows across the entire
experiment and later follow-up turns. Synthetic child usage is not separately
persisted in the usage table, so child-attributed token and cost totals are
unavailable; the parent-session aggregate is not a valid substitute. The
Rust child initially could not run the golden evaluator because its sibling
had not finished; the combined evaluator copy resolves that dependency after
both child results were available. No shared-checkout writes were used.

This is one successful S1 against one completed S0 on the same frozen
shared-interface task, in addition to earlier exploratory trials and
incomplete attempts. It establishes that the daemon-backed isolated S1 route
can complete both sides and integrate to a passing result after the
approval-mode correction. It does not establish a repeatable improvement in
quality, time, cost, or agent effectiveness. S2 peer-observation and S3
claim-enforced comparisons, further task strata, resource attribution, and
full sandbox writer/path qualification remain open. S4 remains unavailable
pending M005.

## 12. Registered-agent S2 retry and exact-head CI (2026-10-18)

The frozen shared-interface fixture v1 was independently confirmed at digest
`b0f0ff1b3939c95a2113934e3669d26fee403c37ca302d28759ab3b78d864bf1` before
this retry. An initial group request selected the unregistered `default` agent;
both child runs failed before provider work. A corrected S2 request used the
registered `build` agent for both isolated child worktrees, `join_policy=all`,
and factual peer observations without claims.

| Assignment | AgentRunId | Outcome | Input/output tokens | Wall time | Tests / commit |
|---|---|---|---:|---:|---|
| Python consumer | `26128ee3-bc39-47b8-8f50-2f032d51bf6c` | Completed; child worktree dirty | 420,176 / 8,993 | 103.47 s | `python3 -m unittest discover -s python/tests -v`: 7 passed; branch/stage/commit denied; no commit |
| Rust producer | `6634f466-cef3-475c-86e9-d718e7f7758c` | Interrupted by scheduler recovery; child worktree dirty | 1,650,511 / 14,466 | 273.51 s | No terminal test report; no commit |

The provider returned no cost for either child; stored `cost_usd=0.0` is not
reported as zero cost. Rust was still generating provider turns with no finite
tool-call limit, so the isolated daemon was stopped after 273.5 seconds. The
Python child could not create a branch or commit under its worktree policy.
Neither child result was integrated, and no acceptance evaluator ran. Startup
recovery marked Rust interrupted but left group
`ccae0678-08d8-4548-a9bb-30244538a9d0` in `running`; the two dirty managed
worktrees are preserved outside the fixture for diagnosis. This is a failed
coordination/economics attempt, not a comparative S2 effectiveness result.

The TaskTool creation path supplies `max_tool_calls: None` to delegated jobs.
A further provider-backed comparison needs an enforced finite tool-call or
wall-clock budget and child instructions to use the typed Git mutation
surface for branch/stage/commit. Until those conditions exist, the remaining
S2/S3 provider arms and the overlapping Rust/dependency-heavy strata remain
open and unrun. S4 remains separately unavailable under M005.

For exact-head qualification, hosted CI run
[39103958556](https://github.com/dbowm91/codegg/actions/runs/39103958556) passed
on source SHA `671e4c0ec2a9c9091d50cc601031b205181fa02d`. The Windows managed
worktree run
[39103958482](https://github.com/dbowm91/codegg/actions/runs/39103958482) also
passed on that SHA, including deep-path linked-worktree materialization. Local
`verify.sh full` passed with 14,390 tests and 6 skipped; the additional
`server,plugins,lsp-test-support` nextest run passed with 11,335 tests and 6
skipped. These source checks were completed before this documentation-only
evidence update.

M005 remains separate. Its candidate writer-path inventory is complete, and
Darwin native lifecycle qualification passed its tests while exposing the
negative property that a `setsid()` descendant can write after managed job
completion. That failure keeps shared-write serialization unavailable; M008's
isolated S2/S3 work does not depend on M005.


## 13. Bounded S2 retry and child tool wiring (2026-10-18)

A new registered-agent S2 retry used the same frozen fixture, isolated
worktrees, factual board observation, and `max_tool_calls=32` per child. The
budget persisted on both durable runs. The retry exposed two missing runtime
capabilities: child registries did not receive the daemon-owned
`JobSubmissionService`, and `BrokerInvocationContext` dropped authenticated
origin attribution needed by group actions. Both paths are now wired and have
focused regression coverage.

| Assignment | AgentRunId | Queue / wall | Usage rows | Input/output tokens | Result |
|---|---|---:|---:|---:|---|
| Python consumer | `35523fc1-f471-4d56-9bb0-1d4ed70f4d3f` | 1.505 / 91.177 s | 9 | 143,025 / 8,157 | Workspace write denied; stopped; no tests or commit |
| Rust producer | `bb906993-b108-4335-ac27-081f494db972` | 2.623 / 135.656 s | 20 | 376,316 / 13,512 | Completed with no usable text; no tests or commit |

The Python child observed both members active and no completed results or
claims on the production group board. Both worktrees remained clean. No changes
were integrated and no acceptance evaluator ran. Provider-reported cost was
unavailable; persisted `cost_usd=0.0` is not evidence of zero cost. The parent
session aggregate reached 803,723 input and 9,221 output tokens across 48 rows,
which is not a valid substitute for child-attributed usage. The parent turn
continued after group completion, so the isolated daemon was stopped.

This attempt remains non-qualifying. The child write denial requires a
deterministic root-cause check under the isolated trial policy before another
provider run. S2/S3 and overlapping Rust/dependency-heavy trials remain open;
M005 stays independent and S4 remains unavailable.

## 14. Exact-head CI after receipt-schema reconstruction (2026-10-18)

The frozen M003 tool-schema receipt failure was resolved by restoring the
historical opaque `spawn_many` item schema during receipt recomputation while
asserting the current expanded schema separately. The immutable receipt itself
was not changed. The resulting source head is
`7bdfaa18dfb5bbec6bb887adb140d51573e15c76`.

Hosted CI run
[39112674196](https://github.com/dbowm91/codegg/actions/runs/39112674196)
passed on that exact source SHA, including workspace Clippy and workspace
tests. Windows managed-worktree qualification
[39112675703](https://github.com/dbowm91/codegg/actions/runs/39112675703)
also passed on the same SHA, including deep-path linked-worktree materialization.
The feature branch contains `origin/main` as an ancestor. This evidence update
changes documentation only; no implementation source changed after the
qualified SHA.

These results qualify the integrated source head, not the provider-backed
comparative trials. The available child result still lacks a permission/tool
error trace for the first Python write, so its cause is not established and a
safe S2 rerun is not yet justified. S2/S3, overlapping Rust API and
dependency-heavy strata remain open. M005 remains separate with its Darwin
escaped-descendant failure; no shared-worktree capability or downstream plan
was unblocked.

## 15. Exact-head qualification after audit fixture correction (2026-10-18)

The exact-head CI run exposed one remaining test-fixture omission:
`bash_shell_success_emits_one_structural_event` constructed a native
`BashTool` without its canonical workspace root. The fixture now supplies the
same explicit root used by neighboring audit cases. No runtime behavior
changed. The integrated branch retains `origin/main` as an ancestor.

On integrated source head `a5977dd7342945990d7749ebaeb163374125ec8f`, hosted
CI [39132947871](https://github.com/dbowm91/codegg/actions/runs/39132947871)
passed formatting, guards, workspace Clippy, and all 14,446 workspace tests
(6 configured skips). The Windows process launcher workflow
[39132947859](https://github.com/dbowm91/codegg/actions/runs/39132947859)
passed, including managed process, scratch lifecycle, ACL, AppContainer, and
native shell/policy checks. Desktop E2E passed on scratch source head
`65e857abeb3389815a86afb7edf47701dee4f6ce` in run
[39132905786](https://github.com/dbowm91/codegg/actions/runs/39132905786);
this is corroborating branch evidence, not a claim that that run tested the
integrated M008 SHA.

This qualifies the integrated implementation source for hosted CI and Windows
platform suites. It does not change the S2/S3 outcomes or satisfy M008's
Git-verified comparative acceptance gate. The workstream remains active, M005
remains separately gated, and no downstream plan is unblocked.

## 13. Matched frozen-interface S0–S3 and follow-up strata (2026-10-18)

After the child-start readiness and Git-result publication fixes at source
commits `9e109e7b` and `475ce4a5`, a complete S2/S3 pair was qualified against
fixture v1. The successful comparison round used the unchanged fixture digest
`b0f0ff1b3939c95a2113934e3669d26fee403c37ca302d28759ab3b78d864bf1`, the same
`opencode_go/gpt-6-luna` model, and a persisted 128 tool-call cap on every
child. The complete per-run data is in the adjacent JSON entry
`matched_s0_s3_2026_10_18`.

| Arm / child | Group or run | Result commit | Child tests | Input / output tokens | Usage rows | Child wall |
|---|---|---|---|---:|---:|---:|
| S0, one agent | group `6718bf56-19b3-49b0-8d59-47b64a5d02c6`; run `c52835b8-1923-4c5c-8882-31dc1ba30ded` | `dfb581be7b6328fe66848f9b3a0bc3891fa3ef19` | Rust 7/7; Python exact committed worktree 7/7; frozen evaluator passed | 689,591 / 11,561 | 27 | 145.330 s |
| S1 Rust | run `b8998f8b-3225-4bd3-9d4a-a665ac1f0e33` | `b87396ba5c757bd884ddfa5aed3e9c76f594bb47` | Rust 7/7; integration evaluator passed | 297,808 / 5,551 | 16 | 80.804 s |
| S1 Python | run `4b0e9390-c89b-4904-9e36-8969d07da88f` | `651152a8e4ec8287fe286611bc89f85bf8dbb031` | Child pytest 7/7; exact committed worktree unittest 7/7; integration evaluator passed | 631,134 / 9,320 | 31 | 134.679 s |
| S2 Rust | group `19b5f3b6-cef9-4f90-9311-fec33dc89ef0`; run `560f9404-a25d-4523-86bf-e1308c6454ed` | `f414263b461b4aa188c32887fa3684ef3b1ff381` | Rust workspace 7/7; integration evaluator passed | 332,196 / 6,232 | 17 | 84.253 s |
| S2 Python | run `b64c5e21-a945-4dd3-beab-516fc0b65c8e` | `e01bbb09f2d918a3710fbe77f9db31d522077437` | Child pytest passed; exact committed worktree unittest 7/7; integration evaluator passed | 688,779 / 5,898 | 32 | 114.563 s |
| S3 Rust | group `518b479c-e72d-48f4-8379-a4fdac422f5d`; run `bfb7d0f2-83b6-4c01-a847-f8aaa0dc0b12` | `9df3625dc5384d7287d056e4cfabe92df3d3aad2` | Rust workspace 7/7; integration evaluator passed | 365,322 / 6,488 | 16 | 263.301 s |
| S3 Python | run `8eb27adf-74e0-40d2-9cca-1ae2717480ed` | `4d607168bc4e7de048a613b5129bbfec599ea687` | Exact committed worktree unittest 7/7; integration evaluator passed | 594,338 / 5,154 | 29 | 89.791 s |

Integrated S1, S2, and S3 each passed Rust 7/7, Python 7/7, and
`scripts/check_cross_language_golden.py`. S0's single result passed the same
frozen evaluator. Group wall times were S0 146.857 s, S1 135.197 s, S2
115.345 s, and S3 264.903 s. The 128 cap is per child; multi-agent arms could
therefore spend up to twice S0's calls. Two attempted 64-call sensitivity
rounds did not produce complete accepted pairs and are excluded from success
comparisons. The provider has no supported matching price, so stored 0.0 USD
is unavailable pricing, not zero cost. One sample per arm, unequal aggregate
caps, and unavailable USD prohibit a performance or cost-effectiveness claim.

The intentionally overlapping Rust API S1 stratum used group
`82828a6c-a0cf-4240-a7c9-9f41ab0e4e71`. Children `1572e914-21e7-422e-89b3-84d8fab1e4cb`
and `52427b11-8335-4144-b822-0d38c5b1ffef` published commits
`154b178d9042ba5aba8775e565cb59c502773063` and
`88dae394f360a7b63d029307f0eba6cca61a74d5`. Each added two tests and passed
those new tests, but the seven existing producer tests still failed on the
frozen starter TODO. Because both children edited `rust/src/lib.rs`, the
second cherry-pick conflicted. Manual integration preserved both APIs at
`7033e5b74bcbf6cb603d67e32fe83aad1b79acf3`; the capable S0 implementation plus
both additions passed 11/11 Rust tests and the frozen cross-language
evaluator. This is an integration recovery result, not independent child
full-suite success.

For a sequential dependency, the two API prerequisite commits were first
integrated at `bda9d756824f09feed581d2c0c022200839a8d0a`; dependent child run
`67c36c67-8bc5-4ea7-8db1-4a7bf581cab4` then published
`e79976ac31ff34115d5a31fc0be8c889866e780a`. The child and parent rerun both
passed the full 13-test Rust workspace suite, and the frozen evaluator passed.
The child reported 258,929 input and 2,529 output tokens across 16 usage rows.

The full-workspace Rust compilation stratum completed from two clean worktrees
on source `803b5865`, using `cargo check --locked --offline --workspace`,
`CARGO_BUILD_JOBS=1`, and one shared target directory. Both checks passed. The
cold build took 691.64 s wall, 530.50 s user CPU, 68.70 s system CPU and
reported 2,684,329,984 B maximum RSS; the target occupied about 2.1 GiB after
it. The second request spent most of its 807.00 s wall time waiting on Cargo
cache/build locks, then reused dependencies and rebuilt workspace-path-specific
crates; it used 105.66 s user CPU, 8.06 s system CPU, and reported
2,409,062,400 B maximum RSS. Final target size was 2,943,560 KiB. The host
reported Darwin 25.6.0, `uname -m=x86_64`, and Rust 1.89.0 host triple
`aarch64-apple-darwin`. This measures cold workspace compilation and serialized
shared-target reuse, not a parallel build speedup or provider-backed code-change
trajectory. The machine-readable record contains log paths and the full run
metadata. M005 remains active independently; S4 remains unavailable because
writer quiescence is not qualified. No downstream plan is unblocked by these
results.

## 16. Token-cap and coordinator calibration (2026-10-19)

Two setup trajectories were excluded before scoring. The first sent the IM-1
retry-policy task to a project-summary fixture by mistake; the child made no
changes. The corrected IM-1 S0 calibration used one isolated child with a
120,000-token ceiling, but the root coordinator remained uncapped. The child
used 129,451 input plus 2,653 output tokens (132,104 total, 12,104 over its
ceiling) and stopped with modified Rust/Python sources but no result commit or
test report. The coordinator used 244,452 input plus 4,589 output tokens
(249,041 total). Reported cost was `0.0` for both runs, but no local pricing
rate was available, so cost is unknown. All 381,145 observed agent tokens are
preserved in `008-isolated-results.json`; this is calibration evidence, not a
budget-matched effectiveness result.

The evaluator ran both frozen suites on the child worktree (Rust 5/5, Python
5/5), committed only the two implementation files, merged that candidate into
the isolated parent worktree, and reran both suites successfully. The child
had also created `.codegg/` and Rust `target/` artifacts; they were not
included in the candidate commit. The agent did not commit or report its test
results, and integration time was not instrumented, so this trajectory is not
scored as autonomous success. It exposed the need to cap coordinator usage and
measure each child's actual overrun. The repeated protocol now fixes an
all-agent total `B` per condition: S0 spends it in the coding turn, while
S1/S2/S3 reserve 300,000 tokens for one coordinator turn and allocate the
remainder across the workers. Additional trials must use the frozen allocation
in `008-task-matrix-v1.md`; these setup runs do not count toward their
repetitions.

## 17. Headless budget-cap protocol smoke (2026-10-19)

The rebuilt headless binary accepted `--max-model-tokens 10000`, submitted a
response-only prompt through the isolated daemon, and completed with `READY`.
The usage journal recorded 10,845 input plus 5 output tokens (10,850 total),
an 850-token overrun from the final in-flight response. Cost is unknown because
the local price table has no matching rate. This used the existing calibration
session and is excluded from all scored task and condition cells. It confirms
the CLI-to-daemon cap path and the need to classify actual overrun explicitly;
it does not qualify an effectiveness trial.
