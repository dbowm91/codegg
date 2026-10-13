# Linux Execution Isolation M002 — Composable Linux Launch and Obtained Enforcement

Status: active (internal contract/probe only; production Linux backend promotion still requires ADR-0023 acceptance)
Primary class: infrastructure
Interface dependency: M001/C001 trusted Git capability matrix; M001 strict closure is NOT a hard dependency.
Repository baseline: `481d5077de516758795084a860a15c747a889170` (rebased Linux branch, 2026-10-13). Source roadmap: `plans/subsystems/linux-execution-isolation-roadmap.md`. Long-term requirements: `plans/000-long-term-specification.md` §§4,17,27,29 and `plans/001-terminology-and-domain-model.md` §8. Applicable accepted ADRs: ADR-0001, ADR-0002, ADR-0004, ADR-0016. Proposed Linux backend gate: ADR-0023; do not treat it as accepted. Accepted cross-tool security authority: ADR-0024. The infrastructure interface and legacy/fake adapter work may close without backend promotion. Target closure: `plans/closure/linux-execution-isolation/002-status.md`.

## 1. Objective

Add one Linux-specific composable sandbox **launch** abstraction behind `ManagedProcessService` that can model filesystem views, network mode, process/IPC scope, syscall constraints and optional resource limits without falsely reporting them enforced. Provide backend feature probes and a test-only no-op/denial adapter; preserve existing Landlock-only launches and default behavior. No completed namespace/network/resource security capability is claimed in this infrastructure slice.

## 2. Why ready; current evidence and authority

`src/security/sandbox/backend.rs` selects one process-local `probe/apply` entry and assumes the helper mutates itself then `exec`s a target. `src/managed_process.rs` owns required helper launch, private setup status and process group cleanup. `src/security/sandbox.rs::SandboxLaunchSpec` contains path read/write/deny rules; `codegg-core/src/approval.rs` separately reports requested sandbox and obtained filesystem/network enforcement. ADR-0016 already mandates platform-owned launch semantics rather than pretending every mechanism fits `apply`. This milestone must not insert bubblewrap into `BACKENDS` as a simple additional Landlock-style `apply` callback.

## 3. Invariants

The daemon and all existing Unix/macOS/Windows default child paths retain their behavior. User-specified executable/argv/cwd/env are never flattened; trusted helper identity is derived from owned installation, not cwd, PATH or untrusted env. Required setup failures produce no target marker. `FullHost` stays explicit; `WorkspaceWrite` default remains current Landlock-only with unrestricted network. Parent/child authority and worktree leases cannot be widened by backend selection. A launch receipt cannot be written/forged by user code. Network-only success must not imply filesystem success or vice versa.

## 4. Scope

In: internal typed Linux launch request and feature probe (Linux-only cfg), descriptor ownership and stepwise status, feature matrix, backend selection criteria, negative feature-gate tests, confined helper interface ready for next milestones. Out: implementing the actual protected bind mounts (M003), seccomp/proc/namespace policy (M004), network policy switch (M005), proxy (M006), cgroup limits (M007), schema migration/default rollout (M008).

## 5. Production changes and design contract

Represent `FilesystemView` (readable mount roots, writable lease/scratch, protected descendants), `NetworkIntent` (legacy unrestricted, offline required, mediated later), `ProcessScope` (legacy vs namespace-required), `SyscallConstraints`, and `ResourceBudget`. Every dimension carries required/optional/disabled demand, parent ceiling and an obtained outcome; provider authorization and shell classification do not choose these.

Decide the trusted launcher shape after inspecting existing `ManagedProcessService`, private status/spec ownership and child bootstrap. A launcher adapter can spawn bubblewrap as a trusted owned helper, but must handle a second process, PID1/reaping, signal forwarding and status timing. It may retain and compose Landlock after view construction only if inheritance/ABI/FD ordering is proven; otherwise report which backend actually enforced the view. Enumerate dependency versions/locations, verify executable provenance and avoid a setuid helper, privileged namespaces or insecure PATH lookup.

Feature probes must separately return namespace-user/mount/pid/net/ipc availability, enforced seccomp filtering ability, required `/proc` construction, bubblewrap trusted executable availability, Landlock version, cgroup-v2 delegation, and incompatible host LSM conditions. Classify no-userns hosts as unavailable for enhanced tier with actionable diagnostics; do not implicitly choose uncontained mode. Probe is not proof that child policy applied.

Typed obtained evidence should include backend/version/ABI; filesystem read/write; protected descendants; process/PID/IPC/FD; network mode; resource bounds; each with applied/absent/unsupported/failure and limitations. Preserve public DTO meaning; if adding persisted fields or changing wire representations, add an explicitly versioned migration and demonstrate older consumers do not interpret an unsupported dimension as applied.

## 6. Ordered work packages

A. Freeze a baseline test: original Landlock path, Seatbelt path, constrained unsupported host refusal, FullHost explicit, read-only/writable allowed operations and no change to network field.

B. Define Linux launch requirements, typed internal adapter contract, feature probe and obtained enforcement as *facts*, with no fallback mutation on probe failure; document where in parent/child startup the status becomes authoritative.

C. Add a test-only fake adapter for success, unsupported, malformed/forged receipt, duplicate status, failed setup, exec error and cancellation. Test that simulated success alone never implies actual containment outside tests.

D. Integrate adapter dispatch under `ManagedProcessService`; reserve actual namespace adapter registration as unavailable until M003/M004 prove the individual dimensions. Keep privileged spawn owners (Git/LSP/test/Python) inventoried, not automatically marked covered.

E. Update `architecture/security.md`, `architecture/jobs.md`, `docs/security-semantics.md`, scoped tests and static guard. No new daemon service, socket or database.

## 7. Failure, cancellation, restart, contention

Every required policy dimension either completes setup before target exec or aborts without executing target code. Read status via existing private pipe, never stdout/stderr or target-controlled file. Error paths close private FDs, spec files, owned helper processes and scratch; preserve existing bounded stdout/stderr, cancellation and process-group ownership. For namespace helpers that fork, reap PID1/descendants and account for parent/helper death. Multiple concurrent jobs do not share status handles or mount roots. Restart cannot declare an orphan process "contained" solely because a previous setup receipt existed.

## 8. Compatibility and migration

No default behavior change, CLI flag or persisted policy required. Additive internal interfaces first. Prohibit code that changes `SandboxProfile` or `ApprovalMode` interpretation. A later opt-in path must have its own operator admission and versioned audit fields.

## 9. Tests / verification

Unit: required vs optional dimensions, immutable ceiling, feature-probe outcomes, obtained status matrix, unknown backend/field rejection, misordered/malformed receipt. Integration: existing Landlock real helper and backend absence; test-only adapter injection; helper timeout and cleanup; cross-platform compilation of unchanged macOS/Windows paths. Security: executable substitution via PATH/env/cwd, forged status through stderr, untrusted FD inheritance, target marker on failed launch.

```bash
cargo fmt --all -- --check
cargo test -p codegg --lib security::sandbox
cargo test -p codegg --lib managed_process
cargo test -p codegg --test sandbox_containment -- --test-threads=1
cargo test -p codegg --test sandbox_landlock -- --test-threads=1
python3 scripts/check_sandbox_contract.py
python3 scripts/check_sandbox_policy_wiring.py
scripts/verify.sh quick
```
Document off-platform skips and native probe results, not merely mocked success.

## 10. Acceptance criteria

One canonical launch owner and an internal typed request/obtained status boundary are demonstrably in use. Landlock-only legacy behavior still passes real helper tests. Required unsupported mechanisms fail without target execution. A malicious target cannot forge or inherit the private status writer. Every unimplemented enhanced capability is unavailable and cannot be represented as enforced. No new default network denial or authorization change occurred.

## 11. Stop conditions

Do not implement/publish this milestone before ADR-0023 is accepted. Stop for incompatible launcher trust model, setuid requirement, unreviewed protocol migration, inability to observe actual child enforcement, or discoverable alternate spawn paths that could bypass the canonical process owner. Escalate conflict rather than claiming interface-only tests prove isolation.

## 12. Independence from M001 Git compatibility corrective

This milestone is **ready** under already accepted ADR-0016 and ADR-0024 because it defines internal typed requests, required/obtained outcomes, feature probes and test-only adapters while leaving existing Landlock unchanged. It **must not** register a real Bubblewrap/namespace backend or advertise new mount/network/process security until proposed ADR-0023 is accepted and M003–M005 deliver real native proof. M001's 9/12 failing mutation suite is tracked by correctives C001–C003 and is not an input-hard dependency on defining the launch interface. Preserve failure evidence in every closure; do not flip M001 to closed to unblock this work.
