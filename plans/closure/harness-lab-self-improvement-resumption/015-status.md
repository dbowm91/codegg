# Harness Lab Self-Improvement S015 — Closure

Status: **conditionally closed — the provider stream stall was diagnosed and its narrow timeout correction was live-confirmed; H007 sustained qualification remains unachieved**.

## Finding and correction

The stall was in CodeGG's provider-turn event-idle policy. The agent loop timed out after 90 seconds without a decoded `ChatEvent`; it did not know that the corresponding HTTP response was still delivering bytes. S015 added safe, attempt-correlated lifecycle observations across request setup, response headers, body progress, SSE-decoded events, turn consumption, EOF, and timeout. The diagnostic logs retain counts, sizes, elapsed times, event kinds, and status, but no prompts, tool arguments, credentials, raw SSE, or secret-bearing URLs.

Run 2 independently reproduced the failure: one response had HTTP 200 headers and body chunks continuing at roughly 30-second intervals, while no `ChatEvent` was decoded by the 90.009-second idle timeout. This locates the abort at CodeGG's decoded-event guard. Since raw SSE was intentionally not retained, the evidence does not classify the semantics of those body chunks or attribute their contents to a particular upstream event type.

The narrow correction changed the finite decoded-event idle bound from 90 to 120 seconds, based on the previously observed 83-second first-event latency and the run-2 reproduction. Run 4 used the corrected ARM64 binary and independently confirmed the boundary: a correlated response had body progress at 90.000 seconds and 101.196 seconds, its first decoded `text_delta` at 101.521 seconds, then a decoded tool call, Finish, and normal body/decoder/turn EOF at 110.388 seconds. The corrected 120-second guard did not fire. The old 90-second guard would have cancelled this response before its first decoded event. Run 3 is retained as an inconclusive confirmation attempt because no response approached the old timeout boundary.

## Evidence

- Implementation and instrumentation: `543042070407ec5d2a93642e583f89fb0333ab56`; timeout correction: `e99d3c1fe4aeb8f03b5cf2ad11edb5d9ad35cdc8`.
- Corrected binary: ARM64 SHA-256 `7d06fc68058e1963a7a81dee54a581c9bce01063b17a90bd62940b9857a99f92`.
- Run 2 reproduction: [`evidence/s015-run-02-summary.json`](evidence/s015-run-02-summary.json).
- Run 3 inconclusive post-correction observation: [`evidence/s015-run-03-summary.json`](evidence/s015-run-03-summary.json).
- Run 4 live confirmation and separate task result: [`evidence/s015-run-04-summary.json`](evidence/s015-run-04-summary.json).
- Authorization and per-run accounting: [`evidence/s015-live-authorization-20261031.json`](evidence/s015-live-authorization-20261031.json).
- Run 4 receipt SHA-256: `588e1192c5696aeb27836f850829269a4ed4eb467920d29a887c948c031bda66`.
- Run 4 diagnostic log SHA-256: `9877051732bc38f76b2c40d9e058db18cb9b86d6c80268bc1afef8388185941c`.
- Run 4 request-admission receipt SHA-256: `384253fb2a1375db82fab09fe933f686b3b96941449806e1ae98a44c03e87c63`.
- Run 4 causal trace SHA-256: `1afb92a0a97f034f976e71d5623fb5013621e21b9b329343767306bacb23e277`.

## Separate task and horizon result

The run-4 agent completed after 600.774 seconds of case time, with 590.658 seconds of active work, 43 paired tool calls, complete causal observation, and no activity gap beyond 111.625 seconds. The independent H007 verifier failed because the agent recorded zero of the required two repair cycles. The run also did not meet the 30-minute active-work threshold. Therefore the task outcome is failed and the sustained horizon is **not qualified**, even though the provider stream completed normally. Run 3 separately failed the exact-ID job-lifecycle requirement. Neither task result is evidence that this stream correction failed, and neither qualifies H007.

## Authorization and resource accounting

The user authorized up to 20 sequential runs through only `opencode_go/muse-spark-1.3-contributor`, capped at 60 provider requests and 3,600 seconds per run with a 10 GiB free-disk floor. Four runs started and used 167 requests in total; the first and fourth provider-reported USD amounts plus run 3 total $0.060306, while run 2's amount is unknown after proactive cancellation. Run 4 used 34 requests, reported $0.018788, and its minimum observed free disk was 14,343,712,768 bytes. No disk stop was triggered. The remaining 16 run slots are intentionally unused: the campaign stopped once a narrow correction was independently confirmed, as required by the early-stop rule. S013's separate authorization remains consumed and unchanged. S014 remains blocked on its independent-repeatability objective.

## Verification and condition

- `scripts/verify.sh quick` passed on the diagnostic instrumentation commit before the 90-to-120-second correction.
- After the correction, `cargo check -p codegg --lib --tests --locked --target aarch64-apple-darwin` passed with the ARM64 Rust 1.89 `RUSTC` and `RUSTDOC` pinned. This typechecked production and test code; it did **not** execute the new tests.
- The focused `provider_turn` test binary was not run. A first test-link attempt encountered mixed x86_64/ARM64 cached artifacts; a target-pinned attempt was stopped when free disk approached the safety reserve. The corrected production binary was subsequently built for ARM64 and used for run 4. Do not report the focused unit tests as passed.
- The production-shaped response lifecycle ordering/redaction test and the delayed-event/silent-stream tests are present in source. Their execution and exact-head hosted CI remain required before merge/promotion. This is the condition on closure; no further live run is needed to confirm the already-observed timeout boundary.

## Recommendation

Keep the 120-second timeout correction and body-free diagnostics for normal review. Run focused Rust tests and exact-head hosted CI when disk permits. Do not broaden provider retry, decoder, daemon, scheduler, ACP, or job-lifecycle architecture based on this evidence. Keep the H007 task/horizon failure visible and pursue it as a separate workload/agent behavior question only with its own scoped work.
