# Windows Native Sandbox Milestone 007 — MXC Feasibility Spike

Status: blocked/optional (M001 contract required)

Repository baseline: `8e9d8b01e5c229715c8e4dea929e050b391e252c`

Source roadmap: `plans/subsystems/windows-native-sandbox-roadmap.md#7-milestones`

Long-term requirements:
- `plans/000-long-term-specification.md#4-architectural-principles`
- `plans/000-long-term-specification.md#17-job-scheduling-and-execution-backends`
- `plans/000-long-term-specification.md#27-security-requirements`
- `plans/000-long-term-specification.md#29-system-invariants`
- `plans/001-terminology-and-domain-model.md#8-scheduler-and-execution-terms`

Applicable ADRs: `plans/adrs/ADR-0004-approval-routing-sandbox-and-runtime-preferences.md`; `plans/adrs/ADR-0016-cross-platform-sandbox-launch-authority.md`

Primary class: infrastructure

## 1. Objective

Run an isolated, non-production comparison of the Microsoft MXC Windows ProcessContainer / Rust SDK against Codegg's security contract. Produce a go/defer/reject finding; do not activate an MXC backend.

## 2. Why this milestone is ready / dependency gate

Interface dependency: M001 accepted/closed guarantee matrix and launcher seam. This experiment can proceed separately from M002–M006 and does not block release. Microsoft MXC preview is explicitly not yet an approved security boundary.

## 3. Current implementation evidence

Microsoft `https://github.com/microsoft/mxc` exposes an early preview with native Windows ProcessContainer, AppContainer-related capabilities and an SDK, but warns generated policies can be overly permissive and should not be used as security boundaries. Current documented Windows denied paths are not fully supported. Windows 11 24H2/build 26100 is the documented default ProcessContainer floor.

## 4. Invariants that must not regress

- No MXC-generated 'Enforced' status without independent proof.
- No new default dependency, network or elevation requirements for normal Codegg.
- Preserve M001 policy and `ManagedProcessService` ownership.
- Pin exact MXC revision/API used for reproduced findings.

## 5. Scope

### In scope

- Make a standalone experimental harness that maps read/write/deny/network/process policy to MXC APIs and measures actual child behavior on eligible Windows versions.
- Evaluate Rust SDK licensing, API stability, DLL/bundle size, deployment, no-admin availability, startup latency and cleanup.
- Compare to M004 native backend against same adversarial fixture dataset.

### Explicitly out of scope

- Linking MXC into Codegg production, changing default sandbox mechanism, migration, adding public protocol, relaxing deny-path requirements.

## 6. Required production changes

### Research/evidence
Freeze MXC git revision and available Windows SDK docs. Implement outside production path (e.g., `research/windows-mxc/`) with its own dependency graph and reproducible script.

### Adapter matrix
Map `SandboxLaunchSpec` read/write/deny to MXC's documented schema. Treat unsupported `deny_paths` as missing guarantee, not ignore; evaluate application-container identity/network, direct socket and named-pipe access.

### Runtime measurement
On Windows 11 24H2+/other host as available, run exact same child probes and measure p50/p95 cold start, steady-state cost and packaging.

### Disposition
One explicit accept/defer/reject report and the conditions for a future ADR/production plan if MXC graduates.

## 7. Ordered work packages

Any experimental failure is contained to temporary test workdirs and disposable identity; harness cleans up on cancellation, exits and interrupted setup. No registration in `BACKENDS`. If test host/SDK unavailable, record blocked and no security conclusions.

## 8. Failure, cancellation, restart, and contention semantics

No Codegg production dependency or installed artifact change. An approved future MXC backend would require a separate ADR and implementation plan with equivalent observed tests.

## 9. Compatibility and migration

- Narrow executable harness security matrix, including denied read, outside write, descendant Job/cleanup, offline network and IPC.
- Version/build matrix, backend API diagnostics, overhead and license/security review.
- No changes to existing unit/Unix tests expected.

## 10. Required tests

`cargo fmt --all -- --check`
`python3 scripts/check_sandbox_contract.py`
Experiment: record exact pinned MXC build/run commands and captured observations per tested Windows version (do not invent a repo target until harness exists).

## 11. Required verification commands

Create `architecture/windows-mxc-feasibility.md` or equivalent research findings; roadmap/registry status updated only on evidence.

## 12. Documentation updates

- One reproducible feature/guarantee gap and overhead matrix with exact SDK revision and Windows build.
- Explicit recommendation with test results, source documentation and decision about future production backend.
- Production backend selection and compilation unchanged.

## 13. Acceptance criteria

STOP if the only available demonstration depends on unsupported denied-path behavior, preview API is unbuildable, or experiments mutate global firewall/ACL without safe rollback. Record blockers, not a feature completion.

## 14. Stop conditions

Pinned MXC revision/license, Windows build/feature availability, runnable harness path, observed security matrix, cold start measures, go/defer/reject decision and downstream planning implications.

## 15. Closure evidence required

Reference: https://github.com/microsoft/mxc and https://github.com/microsoft/mxc/blob/main/src/core/mxc-sdk/README.md ; MXC's own README warns no current profile should be treated as a security boundary.

## 16. Handoff notes

undefined
