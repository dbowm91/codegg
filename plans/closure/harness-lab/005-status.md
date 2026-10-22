# Developer Harness Lab Milestone 005 — Closure Status

Status: conditionally closed

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
before Task 2 under the configured usage stop rule. M005 remains conditionally
closed; the redacted receipts are in
`plans/closure/harness-lab/evidence/live-pilot-20261021.json`.

Two of the four conditions that were open at that closure are now discharged, and
the third is blocked by something outside this repository rather than by the
harness. The usage-accounting condition is discharged: the controller enforces the
admitted caps *before* every provider request, so a turn that fails cannot have
exceeded them, and the completed Task 2 run reported one provider request and
`cost_usd: null` with an explicit reason. The accepted correctness-fix trace and
the live compile-correction trace are both captured (C003's accepted trace and the
correction trace in `plans/closure/harness-lab/004-status.md` §3.1).

What remains is the second route of the paired pilot, and the reason is measured
rather than assumed: the CodeGG **subject** can be routed to exactly one of the
four operator-approved routes. The other three are blocked by boundaries this
repository does not own — the shared provider profile's reviewed wire map, and the
provider's own catalog and upstream state. §5.1 records the audit.

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
| The pilot compares two allowed routes on the same frozen task and binary | `evidence/route-audit-20261022.json`; `plans/closure/harness-lab-post-pilot-corrective/003-evidence.md` §5.4 | partial | Task 2 was completed by `muse-spark-1.3-contributor` (11/11 verifier, 45.4 s, one provider request, cost unobservable) on the frozen `ranges` fixture. The second route could not be run: the CodeGG **subject** can be routed to exactly one of the four approved routes, and the other three are blocked outside this repository — see §5.2. |
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

The live provider pilot is separate from credential-free CI. Its partial result,
including the failed turn and unavailable usage fields, is recorded without
capturing provider response bodies or credential material.

## 5. Remaining conditions

1. **The second route of the paired pilot is still missing, and the reason is
   measured rather than assumed.** The operator approved four routes —
   `longcat-2.5-preview-free`, `muse-spark-1.3-contributor`,
   `minimax-m3.1-flash-free` and `beam-free` — and the OpenCode CLI's catalog
   lists all four (and serves three of them today). The CodeGG **subject** can
   be routed to exactly one. The audit is preserved at
   `plans/closure/harness-lab/evidence/route-audit-20261022.json`:

   | Approved route | In the provider catalog | Mapped in the shared wire profile | Selectable by the subject | Upstream answer |
   |---|---|---|---|---|
   | `muse-spark-1.3-contributor` | yes | yes (`openai_responses`) | **yes** | serves |
   | `longcat-2.5-preview-free` | **no** — withdrawn from `/models` | yes (`openai_chat_completions`) | no | **HTTP 402** "Upstream request failed: Endpoint is unavailable." |
   | `beam-free` | yes | **no** | no | serves (cost 0) |
   | `minimax-m3.1-flash-free` | yes | **no** | no | serves (cost 0) |

   The two free models are served by the upstream and are refused by CodeGG
   for one reason only: the shared provider profile's reviewed wire map
   (`eggpool-provider-profile` rev `9ac6a131`) has no entry for them, so
   every request for them fails closed at `wire_unresolved` before a request
   is built. That is the intended behaviour, not a defect — the same
   fail-closed path is what keeps `cat-jam-free` out of the picker — and it
   is **not** operator-overridable: declaring the model in the config's
   additive `models` block does not surface it, because the config augments
   catalog metadata, not the wire map. The upstream head of the profile
   (`e48f021e`) still has no entry for either id, so a dependency pin bump
   does not help either. The fix is two entries in eggpool's
   `_provider_profiles.toml` under `[providers.opencode-go.model_wire]` with
   `preferred_surface = "openai_chat_completions"`, and it is outside this
   repository.

   A direct call proves the transport is not the obstacle: with CodeGG's own
   transport headers (`Authorization: Bearer …` plus `x-opencode-session`),
   `POST https://opencode.ai/zen/go/v1/chat/completions` with
   `model: beam-free` answers `200` with `"OK"`. The OpenCode **Zen** surface
   is not a fallback: it answers `403 FreeTierError` — "OpenCode's free tier
   can only be used from within OpenCode."

   **Action:** when the shared profile maps the two free models — or when
   `longcat-2.5-preview-free` returns to the provider's catalog and its
   upstream recovers — run the frozen Task 2 pair from the identical
   committed fixture with one binary on both sides, and record both
   outcomes. Preserve the existing Task 1 and Task 2 receipts; do not apply
   automatic corrections between subjects.

2. **The usage-accounting condition is discharged.** Before making another
   live call, M005 asked for either an explanation of Muse Spark's missing
   terminal usage or a provider-side hard ceiling that still holds when the
   agent turn fails. The controller-enforced budget admitted before the first
   provider request supplies the second: the cap is checked *before* the
   request is sent, so a turn that fails cannot have exceeded it. The
   completed Task 2 run reported one provider request, 24,022 tokens and
   `cost_usd: null` with an explicit reason — an absent cost stays absent.

3. **Provider-reported cost remains unavailable for the Go routes.** Keep it
   null with an explicit reason; do not substitute a price estimate.

4. **The accepted correctness-fix trace and the live compile-correction
   trace are both captured.** The accepted trace is C003's real fix
   (`plans/closure/harness-lab-post-pilot-corrective/003-evidence.md` §5.5).
   The live compile-correction trace is
   `plans/closure/harness-lab/004-status.md` §3.1: an authenticated OpenCode
   proposer was handed a real compiler diagnostic, its correction was re-gated,
   and the corrected candidate was scored and accepted
   (`compile_correction_retries: 1`, 13/13 pairs, one improvement, zero
   regressions). No baseline defect was fabricated to force an acceptance
   result: the compile fault in that trace was induced deliberately and the
   candidate's changelog says so.

## 6. Planning audit

M001–M003 remain closed. M004 remains conditionally closed: the deterministic
candidate gate and bounded retry infrastructure remain in place, while its live
proposal/correction and accepted-candidate evidence conditions are still open.
M005 is conditionally closed on the usage-accounting and remaining paired-task
conditions above. The live model routes and secret-safe credential access were
verified in the current installation. The Harness Lab registry contains no later
milestone or blocked successor; therefore this closure unblocks no further
Harness Lab plan. No eligible plan in this line was left unstarted.

Updated 2026-10-22: the usage-accounting condition and the two candidate-trace
conditions are discharged (M004's accepted trace and live compile-correction
trace, and C003's accepted trace). M005's remaining condition is the second route
of the paired pilot, and the audit in §5.1 shows the blocker is the shared
provider profile's wire map and the upstream's own catalog state, neither of which
this repository owns. The usage-accounting condition was discharged by the
controller-enforced budget rather than by explaining the missing terminal usage.

## 7. Registry updates

The subsystem roadmap and registry record M005 as conditionally closed and name
the remaining operational blockers. The pilot is partial and the autonomous
acceptance trace remains open; neither is represented as complete.
