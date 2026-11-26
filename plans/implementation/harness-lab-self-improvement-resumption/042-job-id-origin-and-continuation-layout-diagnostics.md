# Harness Lab Self-Improvement Resumption S042 — Job-ID Origin and Continuation Layout Diagnostics

Status: **implemented; attempt 52 completed unqualified; S043 active for the remaining lifecycle mismatch**

Repository baseline: `ac584c886f03847ff6b829f8509667637e42a052` (shared dirty worktree; preserve unrelated edits)

Source roadmap: `plans/subsystems/harness-lab-self-improvement-resumption-roadmap.md#36-s042--job-id-origin-and-continuation-layout-diagnostics`

Primary class: infrastructure

## 1. Objective

Make the next live attempt distinguish agent input from ACP job-result projection for failed unbound job calls, and record enough public continuation-file layout evidence to classify the remaining continuation-boundary verifier failure without reading hidden verifier content.

## 2. Evidence motivating the plan

Attempt 51 completed 50.48 minutes of active work and every currently projected public task gate. The unchanged hidden verifier passed 3/5 checks, with one continuation-boundary and one transcript/job-lifecycle failure. S041 correctly made transcript lifecycle incomplete: the sole unbound action was a failed `job.status` call, but the ACP event recorded only an arguments digest, so the receipt cannot tell whether `job_id` was missing on input or absent from the projected result. All existing continuation checks were true while the verifier still reported a boundary failure. Redacted verifier diagnostics retain only safe aggregate terms, whose `line`/`index` categories make footer placement a plausible missing public predicate, not a proven cause.

## 3. Scope

### In scope

- Project only an enum for job-ID argument shape (`missing`, `non_string`, `empty_string`, `string`, malformed/non-object, or unavailable); never retain the ID or arguments.
- Include bounded counts by action, argument shape, and ACP call status for unbound job calls.
- Project continuation section order, pointer-footer placement after progress sections, pointer ordering, and whether the footer is terminal; retain only counts, booleans, and line indexes.
- Include line-layout predicates in the public continuation reconciliation so an incomplete public layout is named rather than reported as a projection disagreement.
- Build paired ARM64 binaries from the resulting source and run the existing v97 H007 task/verifier once under the remaining authorization.

### Explicitly out of scope

- Reading or executing hidden verifier source independently, retaining verifier text/test names, changing hidden assertions, or weakening H007 qualification.
- Changing the task stages, test gates, independent verifier, model route, or run ceilings.
- Reopening daemon, scheduler, or provider architecture.

## 4. Bounds and authorization

Use only `opencode_go/muse-spark-1.3-contributor`. The active grant authorizes at most 32 additional starts after attempt 51, within the 200 combined-start ledger. Keep the 1200 provider-request ceiling, 1600 native tool-call ceiling, 3300-second prompt-work limit, 3600-second wall hard stop, 300-second inactivity cancellation, 100 same-session followups, 10-GiB runtime disk floor, and 30-GiB preflight reserve. External billed USD remains `unknown` when the provider does not report it. Require a fresh qualifying binary/helper identity and exact disk preflight before any provider start. Stop the loop on qualification or exhausted grant.

## 5. Acceptance criteria

- Unit tests distinguish a missing input `job_id` from a string-shaped input without exposing its value; invalid enum metadata is refused.
- An unbound failed job action projects a bounded action/argument-state/status counter and contains no raw identifiers, tool arguments, or output.
- Continuation diagnostics expose footer placement relative to completed progress sections, expected pointer ordering, and terminal-footer state without retaining note text.
- The line-layout scan is bounded by a 256-Ki-character note and 4096 lines; oversized notes produce unavailable layout evidence without retaining content.
- The public reconciliation marks malformed/incomplete line layout incomplete and retains disagreement when every projected predicate is true but the independent verifier still fails.
- Attempt 52 uses the unchanged v97 fixture/verifier and Muse route. Its safe evidence records the new diagnostics, complete run identity, disk monitoring, and authorization accounting. H007 qualifies only if the unchanged independent verifier passes every check and the other stated horizon gates hold.

## 6. Required verification

- Focused Python tests for ACP projector metadata, job-sequence aggregation, continuation line layout, and public reconciliation.
- Focused ACP Rust unit test plus formatting and relevant source checks.
- Test assertion-quality and duplication guards; `git diff --check`.
- Before a live start, check paired binary and helper hashes, v97 manifest identity, provider-request admission, request-cap boundary, current authorization balance, and free disk at or above 30 GiB.
- During the run, retain the external disk monitor and stop only on declared cancellation conditions. Do not inspect raw runner logs, transcripts, prompts, tool arguments, or verifier output.

## 7. Local ARM64 build note

The shell reports `x86_64` and Homebrew Rust 1.98, while the machine has an ARM64-qualified Rust 1.89 toolchain and the installed LZMA libraries are arm64-only. The default Cargo invocation therefore attempted an x86 test link and failed before the new test ran. Calling `rustup run` alone also left `/usr/local/bin/rustc` ahead of the selected toolchain on Cargo's PATH. The confirmed working shape is to invoke the ARM64 toolchain's `cargo` and set `RUSTC`/`RUSTDOC` to that same toolchain explicitly, with `--target aarch64-apple-darwin`; a tiny isolated Cargo test passed under that shape. The full focused build was stopped before the 10-GiB floor when unrelated disk activity reduced free space to 13.4 GiB, and its owned partial target was removed. No provider start occurred. Resume the full ARM64 test/build only after disk headroom is sufficient to preserve the 10-GiB floor, then restore the 30-GiB reserve before live preflight.

On 2026-11-11, the exact ACP regression `acp::tests::job_id_argument_telemetry_keeps_only_shape_and_presence` passed with Rust 1.89 for ARM64 (1/1). Attempt 52 then completed about 39.2 active minutes with 214/214 tool events paired, 22/22 submitted test jobs terminal, three correlated repairs, exact-ID overlap, reviewer completion, and continuation. The unchanged verifier passed 4/5; the only failure was `transcript_job_lifecycle`, while every S042-era public lifecycle check was true and no job call was unbound. S042's argument-shape and continuation-layout diagnostic scope is implemented, but its string-shape evidence did not prove requested-ID/result-ID equality and its public projection was not tied to the persisted transcript file. Those two gaps are assigned to S043. Attempt 52 safe evidence is `plans/closure/harness-lab-self-improvement-resumption/evidence/s019-attempt52-safe-diagnostic-20261111.json`; provider allowance was USD 0.046783 and external billed USD remains unknown.
