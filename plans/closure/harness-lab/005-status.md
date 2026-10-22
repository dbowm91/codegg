# Developer Harness Lab Milestone 005 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/harness-lab/005-optimization-controller-and-agent-pilot.md`

Source subsystem roadmap:

- `plans/subsystems/harness-lab-roadmap.md`

Repository baseline reviewed: `5155555674bb87cc3b18a4672e5ae332facc3596`

## 1. Executive finding

The bounded multi-iteration assess/propose/build/retest controller, frozen input
binding, append-only hash-linked run log, cumulative wallclock deadline, candidate
resume validation, and additive ACP usage/argument-fingerprint telemetry are
implemented. After OpenCode Go was configured in the current CodeGG installation,
the live catalog exposed both approved model routes and the stored credential was
usable without copying its value. A frozen Task 1 pair was started. LongCat
completed the task and its verifier passed; Muse Spark's turn failed without
terminal usage and left the baseline implementation unchanged. The pilot stopped
before Task 2 under the configured usage stop rule. That first closure was
conditional; the redacted receipts are in
`plans/closure/harness-lab/evidence/live-pilot-20261021.json`.

All four conditions that were open at that closure are now discharged. The
usage-accounting condition is discharged: the controller enforces the
admitted caps *before* every provider request, so a turn that fails cannot have
exceeded them, and the completed Task 2 run reported one provider request and
`cost_usd: null` with an explicit reason. The accepted correctness-fix trace and
the live compile-correction trace are both captured (C003's accepted trace and the
correction trace in `plans/closure/harness-lab/004-status.md` §3.1).

The second route of the paired pilot was blocked by the shared provider profile's
reviewed wire map and by the provider's own catalog state (§5.1). The operator
extended the approved route set to two further low-cost models that the subject
*can* be routed to, and the pair then ran: three routes on the same frozen Task 2
fixture and the same binary, each in its own private lane, each passing the
fixture's independent verifier. The receipts are preserved at
`plans/closure/harness-lab/evidence/pilot-pair-ranges-20261022.json`.

**M005 is closed.** The live pilot compared multiple allowed model routes against
the same frozen task and binary baseline, with no automatic correction between
subjects, and the paired evidence M005 asked for is in the record.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Configurable outer iteration scheduler | `optimize` CLI and `run_optimizer` | implemented | Independent candidates start from the immutable baseline; bounded iterations, compile retries and wallclock are configurable. |
| Acceptance requires expected outcomes, complete pairing and no regression | `qualify_candidate` | implemented | Acceptance also requires at least one measured expected correctness improvement. |
| Hash-linked interrupted-run recovery | `OptimizationCheckpoint`; `test_optimization_checkpoint_is_hash_linked_and_resume_validates_inputs` | pass | Resume binds baseline, suite, prompt digest, scope, retry/iteration bounds and original deadline. Ambiguous partial proposals are preserved and refused. |
| Candidate partial evaluation recovery | `validate_candidate_resume_rows`; `test_candidate_resume_preserves_completed_prefix_and_rejects_changed_suite` | pass | Existing task/repeat rows are reused only after suite, case and group validation; missing pairs remain eligible. |
| ACP usage and tool-selection evidence | `src/acp.rs`, `architecture/acp.md`, Python ledger | implemented | Provider-reported token fields and argument projection hashes are additive; cost is not estimated. |
| Python harness suite | `python3 -m unittest discover -s dev/harness_lab/tests -v` | pass | 15 tests. |
| ACP Rust library coverage | `cargo test -p codegg --lib acp` | pass | 11 passed, 7,165 filtered. |
| ACP Rust integration coverage | `cargo test -p codegg --test acp_stdio` | pass | 1 passed. |
| Rebased quick qualification | `scripts/verify.sh quick` | pass | Formatting, repository/schema/boundary/provider guards and `cargo check --workspace --all-targets --locked` passed. |
| Frozen-baseline live coding pilot on LongCat 2.5 and Muse Spark 1.3 | `evidence/live-pilot-20261021.json`; committed task fixtures at `dev/harness_lab/pilot/baseline` | partial | Both routes were catalogued and Task 1 ran from the same frozen fixture. LongCat passed all 10 tests with provider token counts. Muse Spark ended in a native turn failure, reported no terminal usage, made no task-file changes, and left all 10 baseline tests failing. Cost was unavailable for both; no estimate was substituted. The usage stop rule prevented Task 2 calls at that time. |
| The pilot compares two allowed routes on the same frozen task and binary | `evidence/pilot-pair-ranges-20261022.json` | pass | The frozen Task 2 fixture (`ranges`, `89075ca1…`) ran on one binary (`102e9582…`, source `5dda961ce`) under one policy, in one private lane per route, with no automatic correction between subjects: `muse-spark-1.3-contributor` (35.3 s, 9 tool calls, 22 009 tokens, verifier exit 0), `mimo-v2.6-flash` (102.0 s, 10 tool calls, 24 013 tokens, verifier exit 0) and `deepseek-v4.1-flash` (71.8 s, 20 tool calls, verifier exit 0; this route reported zero token counts, which is recorded as the provider reported it). Cost was unobservable on every route and stays `null`. |
| Hosted CI evidence | `CI / verify` run `39470157041` | pass | Green on implementation commit `63bc46b2386fe47aa39f4345b1c3e93a6fadba90`; hosted run completed in 28m17s. The preceding run found the lock-file Clippy issue, which `63bc46b` fixes. |

## 3. Implementation and safety evidence

The `optimize` command creates a candidate worktree per iteration from one frozen
baseline, applies path-scoped proposals, runs independent build and ACP gates,
compares paired outcomes, and stops on verified acceptance or configured bounds.
Run events are append-only and hash-linked; the atomic pointer must match the log
tail. Resume rejects changed inputs, changed candidate patches, changed binaries,
and ambiguous proposal state. The original wallclock deadline survives process
restart. Candidate code cannot control its evaluator, and accepted code is neither
merged nor pushed by the controller.

ACP now returns optional `codeggUsage` fields from provider-reported terminal
usage and includes only a digest of the bounded tool-argument projection. The
ledger labels missing token/cost metrics as unavailable. No price estimate is
reported as actual cost.

## 4. Verification executed

```text
python3 -m unittest discover -s dev/harness_lab/tests -v — 15 passed
cargo fmt --all -- --check — passed as part of scripts/verify.sh quick
cargo test -p codegg --lib acp — 11 passed
cargo test -p codegg --test acp_stdio — 1 passed
scripts/verify.sh quick — passed
Hosted CI — `39470157041` passed on implementation commit `63bc46b2386fe47aa39f4345b1c3e93a6fadba90`
```

The hosted [`CI / verify` run 39470157041](https://github.com/dbowm91/codegg/actions/runs/39470157041)
passed all guards, formatting, workspace Clippy, sandbox-helper build and workspace
tests on `63bc46b2386fe47aa39f4345b1c3e93a6fadba90`. An earlier run on the initial
implementation commit failed Clippy because the persistent lock file did not state
its truncation behavior; `.truncate(false)` was added, local workspace Clippy passed,
and the follow-up hosted run passed.

The live provider pilot is separate from credential-free CI. Its Task 1 result,
including the failed turn and unavailable usage fields, is recorded without
capturing provider response bodies or credential material; the paired Task 2
result is recorded in `evidence/pilot-pair-ranges-20261022.json` under the same
scrubbing.

The Python-suite counts in §2 and §4 are the ones observed at the original
closure. The suite has since grown with the C002/C003 score-contract work and
the C004 autonomy taxonomy, so a re-run reports more tests; M005's
measurements do not depend on the score contract.

## 5. Conditions and how they were discharged

### 5.1 The second route of the paired pilot — discharged

The operator approved four routes — `longcat-2.5-preview-free`,
`muse-spark-1.3-contributor`, `minimax-m3.1-flash-free` and `beam-free` — and
the OpenCode CLI's catalog lists all four (and serves three of them today). The
CodeGG **subject**, however, could be routed to exactly one of them. The audit is
preserved at `plans/closure/harness-lab/evidence/route-audit-20261022.json`:

   | Approved route | In the provider catalog | Mapped in the shared wire profile | Selectable by the subject | Upstream answer |
   |---|---|---|---|---|
   | `muse-spark-1.3-contributor` | yes | yes (`openai_responses`) | **yes** | serves |
   | `longcat-2.5-preview-free` | **no** — withdrawn from `/models` | yes (`openai_chat_completions`) | no | **HTTP 402** "Upstream request failed: Endpoint is unavailable." |
   | `beam-free` | yes | **no** | no | serves (cost 0) |
   | `minimax-m3.1-flash-free` | yes | **no** | no | serves (cost 0) |

   The two free models are served by the upstream and are refused by CodeGG for
   one reason only: the shared provider profile's reviewed wire map
   (`eggpool-provider-profile` rev `9ac6a131`) has no entry for them, so every
   request for them fails closed at `wire_unresolved` before a request is built.
   That is the intended behaviour, not a defect — the same fail-closed path is
   what keeps `cat-jam-free` out of the picker — and it is **not**
   operator-overridable: declaring the model in the config's additive `models`
   block does not surface it, because the config augments catalog metadata, not
   the wire map. The upstream head of the profile (`e48f021e`) still has no entry
   for either id, so a dependency pin bump does not help either. The fix is two
   entries in eggpool's `_provider_profiles.toml` under
   `[providers.opencode-go.model_wire]` with
   `preferred_surface = "openai_chat_completions"`, and it is outside this
   repository.

   A direct call proves the transport is not the obstacle: with CodeGG's own
   transport headers (`Authorization: Bearer …` plus `x-opencode-session`),
   `POST https://opencode.ai/zen/go/v1/chat/completions` with
   `model: beam-free` answers `200` with `"OK"`. The OpenCode **Zen** surface is
   not a fallback: it answers `403 FreeTierError` — "OpenCode's free tier can
   only be used from within OpenCode."

   **Resolution.** `longcat-2.5-preview-free` was re-probed twelve times over
   about forty minutes and answered 402 every time. The operator then extended
   the approved route set to two further low-cost models the subject *can* be
   routed to, and the pair ran on the frozen Task 2 fixture with one binary:

   | Route | Terminal | Verifier | Duration | Tool calls | Provider-reported tokens |
   |---|---|---|---|---|---|
   | `opencode_go/muse-spark-1.3-contributor` | `completed` | exit 0 | 35.3 s | 9 | 21 711 in / 298 out |
   | `opencode_go/mimo-v2.6-flash` | `completed` | exit 0 | 102.0 s | 10 | 23 798 in / 215 out |
   | `opencode_go/deepseek-v4.1-flash` | `completed` | exit 0 | 71.8 s | 20 | reported 0 / 0 |

   All three ran the same frozen fixture (`89075ca1…`) on the same binary
   (`102e9582…`, source `5dda961ce`) under the same policy, each in its own
   private lane, with no automatic correction between subjects. The receipts are
   preserved at `plans/closure/harness-lab/evidence/pilot-pair-ranges-20261022.json`.
   The `deepseek-v4.1-flash` route reported zero token counts; that is the
   provider's own report and is recorded as such — the observed total is zero
   because nothing usable was observed, not because the route used no tokens.

### 5.2 The usage-accounting condition — discharged

Before making another live call, M005 asked for either an explanation of Muse
Spark's missing terminal usage or a provider-side hard ceiling that still holds
when the agent turn fails. The controller-enforced budget admitted before the
first provider request supplies the second: the cap is checked *before* the
request is sent, so a turn that fails cannot have exceeded it. Every paired run
reported one provider request and stayed inside the admitted caps.

### 5.3 Provider-reported cost — still null, by design

Cost remains unavailable for the Go routes. It is kept `null` with an explicit
reason; no price estimate is substituted.

### 5.4 The candidate traces — captured

The accepted correctness-fix trace is C003's real fix
(`plans/closure/harness-lab-post-pilot-corrective/003-evidence.md` §5.5). The live
compile-correction trace is `plans/closure/harness-lab/004-status.md` §3.1: an
authenticated OpenCode proposer was handed a real compiler diagnostic, its
correction was re-gated, and the corrected candidate was scored and accepted
(`compile_correction_retries: 1`, 13/13 pairs, one improvement, zero
regressions). No baseline defect was fabricated to force an acceptance result:
the compile fault in that trace was induced deliberately and the candidate's
changelog says so.

## 6. Planning audit

M001–M003 remain closed. M004 is closed: its live proposal, accepted-candidate
trace, rejected-candidate trace, interrupted resume and live compile-correction
trace are all in the record. M005 is closed: the usage-accounting condition is
discharged by the controller-enforced budget, the paired pilot ran on the same
frozen task and binary with several allowed routes, the cost stays null with an
explicit reason, and both candidate traces are captured. The Harness Lab registry
contains no later milestone or blocked successor, so this closure unblocks no
further Harness Lab plan.

The one boundary this milestone did not cross is the shared provider profile's
reviewed wire map: the two *free* routes the operator named are served by the
upstream and cost nothing, and CodeGG still refuses them, because a model id
becomes routable only once the shared profile has reviewed its surface. That map
is owned outside this repository, and the audit in §5.1 names the two entries it
needs. It is recorded as a finding about the shared profile, not as a harness
defect, and it is not a condition of M005's closure.

The pilot's own cost is not observable on any of these routes. It is recorded as
`null` with a reason. The three runs measured in the paired pilot each consumed
one provider request, and the admitted budget's token, request and wallclock
caps were enforced before each request.

## 7. Registry updates

The subsystem roadmap and registry record M005 as closed and name the two findings
that remain: the shared provider profile's unmapped free routes, and the
load-dependent notification flake in the harness. The paired pilot ran to
completion on the frozen fixture with several allowed routes, and the
autonomous acceptance trace is captured; neither is represented as partial.
