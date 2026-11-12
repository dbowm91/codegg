# Harness Lab Self-Improvement Resumption S050 — Event Measurement Cause Projection

Status: **active**

Repository baseline: `26c0aebcfa5168b015491b9ef4254f945feec124`

Source roadmap:

- `plans/subsystems/harness-lab-self-improvement-resumption-roadmap.md#44-s050--event-measurement-cause-projection`

Long-term requirements:

- `plans/000-long-term-specification.md` §§26–29
- `plans/002-long-term-roadmap.md`

Applicable ADRs:

- `plans/adrs/ADR-0040-evidence-based-agent-recovery-termination.md`

Primary class: **diagnostic observability**

## 1. Objective

When H007 event measurement is incomplete, safely report which existing gate input failed: explicit ACP event-gap count, observer-drain state, or calls without terminal status. Attempt 61 exposed `measurement_incomplete`, but its safe diagnostic omitted the individual inputs. The next corrective owner cannot determine whether the observer lost events or the tool-call projection left a call unpaired.

## 2. Invariants

- Keep H007 v97 fixture text, hidden verifier, route, task, and all qualification requirements unchanged.
- Use only `opencode_go/muse-spark-1.3-contributor` for attempt 62.
- Preserve 1,200 provider requests, 1,600 native calls, 3,600-second hard wall, 3,300-second prompt-work ceiling, 300-second verifier reserve, 300-second inactivity cancellation, 10-GiB runtime floor, and 30-GiB preflight reserve.
- Never retain event payloads, tool-call IDs, arguments, prompts, transcripts, raw output, hidden verifier text, credentials, private receipt, or runner log.
- Record only fixed observation state and bounded counts no greater than 1,600.

## 3. Scope

### In scope

- Project `event_gap_count`, `event_gap_dropped`, `event_observation_state`, tool start/terminal counts, projected-call count, unpaired-start count, and tool calls with unknown terminal status as bounded numeric/enum fields.
- Make H007 measurement completeness require a drained observer with state `observed_complete`, zero explicit gaps, and zero calls with unknown terminal status.
- Carry this fixed diagnostic through the existing safe task and campaign projections.
- Test explicit gaps, an undrained observer, unmatched starts, unknown call status, complete measurement, malformed counts, and privacy filtering.
- Run the unchanged v97 task once as attempt 62 after focused offline verification and fresh preflight.

### Explicitly out of scope

- New event protocol, background observer, daemon or scheduler changes, provider changes, altered task/verifier, revised caps, or hidden-verifier inspection.
- Assuming attempt 61 had an explicit gap or an unpaired call. Its safe record leaves that distinction unavailable.

## 4. Required implementation

The existing `event_observation.observe_notifications` producer already emits fixed state and bounded counts. Do not retain its `event_gaps` array or source strings. Derive `projected_tool_call_count` and `unknown_terminal_tool_call_count` only from projected calls. H007 measurement is complete only when the observer was drained (`event_observation_state == observed_complete`), gap and dropped counts are zero, observer start and terminal counts equal the projected-call count, no observer start is unpaired, and no projected call has an unknown terminal status.

Campaign output must preserve only the fixed keys, enum values, and integer counts within 0..=1,600. Boolean-as-integer, oversized, malformed, and unknown values are discarded. Keep the underlying event-observation receipt private and uninspected.

## 5. Acceptance evidence

- Unit tests prove the event-measurement Boolean and its diagnostic counts agree for complete, gap, undrained, and unknown-terminal cases.
- Safe campaign projection rejects raw event arrays, arbitrary source strings, booleans-as-counts, and counts above the native-call cap.
- Focused H007/autonomy/campaign tests, assertion-quality and duplication guards, Python compilation, and `git diff --check` pass.
- Attempt 62 uses the unchanged v97 manifest, paired ARM64 binary/helper identity, Muse route, and all existing caps. H007 qualifies only if the unchanged verifier and every current gate pass.

## 6. Ordered work packages

### A — Project bounded event measurement evidence

Thread existing aggregate event counters and the unpaired-call count through the H007 projection. Retain no event bodies or identifiers.

### B — Align the completeness gate

Require observer drain, zero event gaps, and zero unknown tool statuses. Add truth-table tests so a missing drain cannot be reported as a measured complete stream.

### C — Offline verification and attempt 62

Run focused tests and guards; verify exact manifest and paired binary/helper identity, active grant and task headroom, credential availability without printing its value, and fresh disk reserve. Start only after every check passes.

## 7. Stop conditions

- Stop if fixed counts require retaining raw events, tool IDs, or event source strings.
- Stop if the live preflight is below 30 GiB, authorization is exhausted, or any input identity differs.
- Preserve evidence if request, wall, inactivity, native-call, runtime-disk, or cancellation caps fire.
- Do not call H007 qualified from duration, stage count, or partial verifier success.

## 8. Closure evidence required

Record test results, attempt 62 preflight and safe evidence paths, route/model, campaign/grant/task counters, provider requests, measurement-state counts, terminal cause, minimum sampled free disk, and the qualification decision. State explicitly that private raw artifacts were not inspected.
