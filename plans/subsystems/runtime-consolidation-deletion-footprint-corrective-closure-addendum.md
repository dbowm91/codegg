# Runtime Consolidation, Deletion, and Footprint — Corrective Closure Addendum

Status: active

Source roadmap: `plans/subsystems/runtime-consolidation-deletion-footprint-roadmap.md`

Controlling corrective plan: `plans/implementation/runtime-consolidation-deletion-footprint/009-final-corrective-runtime-consolidation-closure.md`

Baseline: `f1e4c16f1bfe16cad57fb6fc290d48ab03974072`

This addendum supersedes only the current execution and closure disposition of M006 and M007. Completed M001–M005 history and the provisional evidence in `plans/closure/runtime-consolidation-deletion-footprint/007-status.md` remain preserved.

## Corrective findings

The post-implementation audit found that the active TUI still uses legacy task requests that M001 now rejects; the provider-turn adapter still delegates its implementation back into `AgentLoop`; M006 remains blocked pending final-tree measurements; and M007 was advanced before its hard predecessor and final evidence completed.

## Controlling execution order

1. M009 is the sole ready corrective handoff for this workstream.
2. M009 restores TUI scheduling through the existing durable schedule API and completes the remaining provider-turn physical ownership move.
3. M006 measurements are repeated only after those production corrections define the final tree.
4. M007 strict integration and hosted closure occurs only after M006 strictly closes.

Current disposition:

- M001–M005: closed historical predecessor work; do not reopen unrelated scope.
- M006: blocked from strict closure until M009 production corrections land and final-tree measurements are accepted.
- M007: corrective pass required; its current closure record is provisional historical evidence, not accepted strict closure.
- M009: ready for handoff.

## Closure rule

The source roadmap may close only when M009 has an accepted closure record, the active TUI uses the durable schedule path, provider-turn ownership is no longer façade-only, M006 records final-tree default and production-feature measurements, the original M007 verification contract passes on the exact final candidate, and one normal existing hosted CI run is green on that candidate.

Verification remains minimal. Do not add CI lanes, matrices, scheduled audits, size or coverage gates, dependency bots, release automation, or a fixed release cadence.