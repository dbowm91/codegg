# Runtime Consolidation, Deletion, and Footprint — TUI Closure Addendum

Status: active

Source roadmap: `plans/subsystems/runtime-consolidation-deletion-footprint-roadmap.md`

Historical corrective closure: `plans/closure/runtime-consolidation-deletion-footprint/009-status.md`

Controlling implementation plan: `plans/implementation/runtime-consolidation-deletion-footprint/010-tui-durable-schedule-identity-and-label-closure.md`

Audit baseline: `376f0041a0f759805152fb0eaaa293bc57b24fdd`

## Purpose

This addendum reopens only the current closure disposition of the runtime-consolidation workstream after a post-M009 audit found one supported TUI contract defect. It does not reopen the scheduler architecture, provider-turn ownership, M006 footprint measurements, or other predecessor implementation work.

M009 correctly moved the TUI to the durable `Schedule*` API, but its accepted regression evidence did not exercise deletion using the identifier actually presented to the user. `/tasks` and the creation toast display only the first eight characters of the opaque durable schedule UUID, while `/task-del` forwards that shortened token as the exact `ScheduleDelete.schedule_id`. The durable store deletes by exact full ID. `/tasks` also loses the scheduled prompt because the list summary does not carry the job template, making multiple interval schedules difficult to distinguish.

## Controlling disposition

- M001–M009: preserved historical predecessor work; do not reopen unrelated implementation.
- M006 dependency/footprint evidence: remains accepted; no remeasurement is required for this TUI-only correction.
- M009: remains the historical architectural closure record and must not be rewritten to conceal the later-discovered TUI defect.
- M010: sole ready corrective handoff for this addendum.

## Required result

M010 must establish one coherent human-facing schedule contract:

1. the short identifier shown by `/tasks` and the schedule-created toast resolves to exactly one full durable schedule ID in the active workspace before deletion;
2. exact full durable IDs remain accepted;
3. ambiguous/unknown/too-short prefixes fail closed and delete nothing;
4. backend/store deletion remains exact-ID only;
5. `/tasks` obtains meaningful user-facing prompt/label information through the existing durable schedule record path rather than another persistence model;
6. a regression test exercises create -> list -> displayed token -> delete -> list absent through the durable client/daemon path.

## Verification posture

Keep verification minimal and change-specific. M010 requires focused TUI/durable-schedule tests, `cargo check -p codegg --locked`, formatting, `scripts/verify.sh quick`, and `git diff --check`. Do not add CI lanes, matrices, static scanners, coverage/benchmark/size gates, dependency bots, release automation, or a fixed release cadence. Do not rerun binary-size measurements unless the implementation unexpectedly changes dependencies/features/profile/topology.

## Closure rule

This addendum may close only when `plans/closure/runtime-consolidation-deletion-footprint/010-status.md` proves that the exact identifier presented by the supported TUI can delete its schedule safely, meaningful task labels are restored, the existing durable scheduler remains the sole authority, and no critical/high/medium finding remains in M010 scope.

After accepted M010 closure, `plans/registry.md` may return runtime consolidation to `closed` with M010 as the final corrective control point.