# Harness Lab Mainline Qualification H004 — Closure Status

Status: **conditionally closed — measurement complete; sustained-horizon target not demonstrated**
Source implementation plan: `plans/implementation/harness-lab-mainline-qualification/004-single-opencode-go-sustained-qualification.md`
Source subsystem roadmap: `plans/subsystems/harness-lab-mainline-and-long-horizon-qualification-addendum.md`
Repository baseline reviewed: PR #205 merge `9185b9c34ce30fd52a41aacd0aaae75f39e54895`; exact merge-head CI attempt 3 passed.
Implementation commit used for the live candidate: `68699fb63136f32e059f78fffa7de39ac19c10eb` (request-boundary cap and receipt). The live binary was built from this candidate before the branch rebase.
Binary SHA-256: `5df991c4f6564f22647f0ed7b28153057e6882efabd517d476e28b7bd1fa6103`
Current branch head after rebasing onto `origin/main`: `83faa880da201075cbf76f85e08aa3862c58cbe0`. The only tree change after the live candidate is PR #206's documentation-only closure record; no runtime source changed.
Live evidence: `plans/closure/harness-lab-mainline-qualification/evidence/004-live-receipt.json`, `evidence/004-provider-request-admissions.jsonl`, `evidence/004-budget-policy.json`, and `evidence/004-runner-invocation.json`.

## 1. Executive finding

One operator-approved CodeGG native ACP session ran with `opencode_go/muse-spark-1.3-contributor`. The model repaired the frozen Rust fixture, completed the turn, passed the hidden verifier, and produced no tool-event gaps. The live request boundary recorded 18 admissions against a hard cap of 60; the one-hour controller deadline and 10 GiB disk floor were not approached. Provider cost was not reported and is recorded as `null`.

The first controller preflight stopped before ACP or provider admission because its temporary case resolver still pointed at the taxonomy fixture root. It used zero provider requests. The resolver was corrected to the staged fixture before the single live session recorded here.

The run took 112.7 seconds, far short of the plan's 30–60 minute sustained-work target. Its background test job also completed in 3.4 seconds, so this run did not measure prolonged job waiting or concurrent independent work. H004 is therefore conditionally closed as a completed bounded measurement, not as sustained-session reliability qualification. The single-run authorization is consumed; the remaining condition requires a separately designed long-horizon task and fresh approval.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | State |
|---|---|---|
| H001–H003 prerequisites | H001 merged and exact main SHA passed; H002 and H003 closure records remain closed at their recorded scopes | Satisfied |
| One approved provider route | `opencode_go/muse-spark-1.3-contributor`; no second CodeGG provider or route was used | Satisfied |
| Request hard cap and durable count | 18 request admissions; provider-boundary JSONL SHA-256 `3cbf6e494277d3061104a692b47046c7931acd7bb9931d3b29cd79e27e110e51`; cap 60 | Satisfied |
| Wall-clock and disk guardrails | 112.724 seconds against 3,600 seconds; minimum free disk 117,218,144,256 bytes (about 109.1 GiB), above the 10 GiB floor | Satisfied |
| Native daemon/ACP and isolated verifier | Candidate binary SHA above; `workspace-write` sandbox; verifier copied outside the agent workspace; hidden verifier exit 0 with `hidden Rust contract passed` | Satisfied |
| Coherent Rust repair | Terminal `completed`, `end_turn`; final independent verifier passed; fixture's public contract digest remained unchanged | Satisfied |
| Tool/event accounting | 23 tool starts, 23 terminals, 0 unpaired starts and 0 event gaps (`observed_complete`) | Satisfied for this session |
| Real background job lifecycle | Test job `355e6dcb-3597-400e-b3f3-7bbe91fd813f` was created by the daemon, executed by the `test` executor and reached failed terminal state on the fixture's intentional compiler typo; the agent called `job.wait` and `job.output` for that same ID, inspected the failure, and repaired the source. A later foreground test completed. Durable records are in the evidence JSON. | Observed, but only a 3.4-second job; prolonged wait/concurrent work not demonstrated |
| Sustained 30–60 minute task | Session ended after 112.7 seconds once the small fixture was solved | Not demonstrated; requires new task design and authorization |

## 3. Production implementation evidence

Candidate commit `68699fb63136f32e059f78fffa7de39ac19c10eb` adds provider-boundary admission for OpenCode Go request/metadata calls, a fail-closed request receipt, and Harness Lab accounting that uses the receipt rather than assuming one provider request per trial. The supervisor forwards the hard cap only for an explicitly capped run and writes the receipt under the private lane. Provider budget failures remain non-retryable and use the existing provider error taxonomy.

The request receipt records only monotonically increasing request numbers and the approved limit. No credential, prompt body or provider response is in that file. The live record includes token usage reported by CodeGG (31,165 input; 450 output), cost `null`, the candidate source and binary hashes, fixture/verifier digests, policy receipt and event counts.

## 4. Verification executed

Before the live request, the corrected pinned Rust 1.89 toolchain was used. `scripts/verify.sh quick` passed, including the guard suite, Harness Lab Python suite (357 passed, 1 skipped) and workspace check. Workspace Clippy passed with `-D warnings`; `cargo nextest run --workspace --locked --profile ci` passed 16,550 tests (9 skipped); the feature-gated command `CARGO_BUILD_JOBS=2 cargo nextest run -p codegg --locked --features server,plugins,lsp-test-support --profile subset --lib --test lsp_test_support --test server_feature` passed 7,925 tests (4 skipped). The combined `verify.sh full` wrapper did not return a clean exit in the captured session after those phases, so this record claims the individually completed gates rather than a single successful wrapper invocation.

Focused request-budget/provider tests and Harness Lab supervisor/accounting tests passed. `python3 -m dev.harness_lab doctor --binary target/debug/codegg` passed with binary SHA above. `python3 -m dev.harness_lab smoke --binary target/debug/codegg --subjects 2` passed, reported zero provider calls, and verified the two-subject lane behavior. The exact controller argv, budget file location and its non-secret contents are frozen in the invocation evidence. The one live run then passed its independent hidden verifier.

Exact H001 merge-head CI is recorded in `001-status.md`: CI attempt 3 and all applicable platform workflows passed at merge SHA `9185b9c34ce30fd52a41aacd0aaae75f39e54895`.

## 5. Invariant review

The live candidate and binary hashes match the recorded run. The immutable fixture manifest SHA-256 was `aacc9fbc2d31304ff6e362128b09d42b05fcddd881d48ffe5bd4db73c9aef5c6`; the hidden verifier SHA-256 was `8ec486e216bb0826b7e2af0e0309901b093d422fd37c76e1c30d411e774e1d05`; the public contract SHA-256 was `7f65eaf0b371069e816acc4e0d3ec8af2ad8c50749d8ebbe0feffb6a2f58df74`. The verifier was outside the agent's workspace and returned success. The model repaired the fixture's source without changing its public tests.

The fixture manifest declares no network requirement and the Rust fixture has no external dependency. The observed provider requests went through the approved OpenCode Go adapter. The run does not claim an OS-level network-denial proof.

## 6. Failure and recovery review

The first scoped background test job failed with exit code 101 on the fixture's intentional `pacth` compiler error. The model observed that same CodeGG-owned job through `job.wait` and `job.output`, inspected the compiler evidence, corrected the typo, fixed the independent normalization/parser behavior, and passed a later foreground test and the hidden verifier. The two `job` tool events were marked terminal errors because the observed job itself had failed; durable job state and the test log confirm the expected fixture failure, not a missing or foreign job.

The model read the README after the 3.4-second job had already ended. The job wait was therefore terminal-result retrieval, not a prolonged wait; the run did not exercise meaningful work concurrent with a running job. The record also does not show a separately delivered automatic terminal-notification event. These are measurement limits, not evidence that the durable job executor failed.

## 7. Migration and compatibility review

The request cap is opt-in through the Harness Lab environment; ordinary provider callers have no new cap unless the controller supplies it. The changes do not alter provider routing, model catalogs or ACP protocol shape. The OpenCode Go provider is the only CodeGG adapter in the live path. No database migration was added.

## 8. Security review

The subject ran under `workspace-write`; the credential was resolved by the existing private lane path and was not printed or committed. The provider receipt contains counts only. The hidden verifier remained outside the writable root and its digest matched the frozen manifest. The controller stopped below neither the 10 GiB floor nor the one-hour cap. No provider fallback occurred.

## 9. Documentation and operations

The durable evidence files contain the sanitized run receipt, the 18-line request-admission JSONL, the approved cap values and the controller invocation record. The run's temporary lane was stopped: `daemon_stop_error` was `null`, its control socket was absent after shutdown, and no CodeGG ACP or Cargo fixture process remained. Both the completed lane and the zero-request preflight scratch were removed after the evidence files were frozen. Free disk was 109.1 GiB at the lowest observed point and 105 GiB after verification and cleanup.

## 10. Unresolved findings

- **Medium / operational qualification:** this run solved the task in 112.7 seconds and did not exercise the planned 30–60 minute work horizon, a prolonged live job wait, or meaningful concurrent independent work. A separately designed larger coherent task and fresh explicit live-run authorization are required before claiming sustained-session reliability.
- **Low / measurement coverage:** the run observed job completion through an explicit wait/output call but did not capture automatic terminal-notification delivery as a separate event. No product defect was established by this run.
- Provider cost was not reported. No dollar amount is inferred.

## 11. Roadmap disposition

H004 is conditionally closed as a bounded measurement. The exact observed outcome is retained; the duration shortfall is not converted into a reliability pass. H001 is closed on its exact resulting-main evidence. H002 and H003 retain their previous closure records. A future sustained qualification requires a new plan and explicit authorization rather than reuse of this approval.

## 12. Registry updates

`plans/registry.md` and the subsystem roadmap record H001 closed and H004 conditionally closed with the duration and evidence limitation. No other provider integration, model route, or live budget is implied.
