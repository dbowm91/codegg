# Runtime Safety, Resource Control, and Footprint — C001/M003 Promotion Disposition

Status: accepted

Applies to:

- `plans/closure/runtime-safety-resource-footprint/009-status.md`
- `plans/implementation/runtime-safety-resource-footprint/003-typed-argv-and-shell-routing-convergence.md`
- `plans/subsystems/runtime-safety-resource-footprint-roadmap.md`
- `plans/registry.md`

Repository baseline reviewed: `0f3bf0b78e5de2dd03742f542d990590f0a32833`

## Decision

Corrective C001 remains **conditionally closed** because its supported-Linux Landlock fixture has not yet produced an accepted enforcement result. That condition is retained as an operational requirement for strict M001/C001 closure and final M008 workstream closure.

The missing Linux result is not a hard implementation dependency for M003. M002's canonical managed-process executable/argv interface is implemented and accepted, and C001 has corrected the helper identity, private setup-status transport, target-stderr isolation, and target-cwd specification defects. Independent review reported no unresolved critical, high, or product-correctness medium defect in that boundary.

Therefore:

- M003 — typed argv and shell-routing convergence — is promoted to **ready**;
- M007 remains blocked until M003 closes;
- M008 remains blocked until M007 closes and the supported-Linux Landlock result is recorded;
- M005/M006 hosted cache or unavailable-dispatch conditions remain operational evidence and do not independently block M007.

## Scope of supersession

This disposition supersedes only these stale control statements in the M003 implementation plan:

- `Status: blocked on M002` is replaced by `Status: ready`;
- the requirement that M002 reach strict closure before implementation is replaced by acceptance of M002's stable executable/argv interface;
- the stop condition `M002 is not closed` applies only if that interface is absent, materially changed, or found defective.

All M003 objectives, invariants, work packages, compatibility requirements, focused tests, static guards, acceptance criteria, and closure evidence remain authoritative.

## Security boundary

This promotion does not claim that Landlock enforcement has been validated on a supported Linux kernel. It does not weaken required-mode fail-closed behavior, helper trust requirements, or final closure criteria. M003 must consume the existing sandbox request/result contract without redesigning or bypassing it.

## Required remaining Linux evidence

Run the existing bounded fixture on one Landlock-capable Linux host:

```bash
cargo test --test sandbox_landlock -- --test-threads=1
```

Record the kernel, effective Landlock ABI, and fixture outcomes in `plans/closure/runtime-safety-resource-footprint/009-status.md` or a compact evidence update. No new CI lane, platform matrix, evidence-transfer milestone, or duplicate full-workspace verification is required.
