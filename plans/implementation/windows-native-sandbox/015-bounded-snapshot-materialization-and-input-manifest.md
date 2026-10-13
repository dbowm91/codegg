# Windows Native Sandbox Corrective C015 — Bounded Snapshot Materialization and Input Manifest

Status: blocked (C014 strict positive closure)

Repository baseline: `d00f1274c844bde7055559f240c020ffb2f1ff46` (`work/windows-native-sandbox-foundation`), 2026-10-12.

Source roadmap: `plans/subsystems/windows-native-sandbox-roadmap.md#adr-0019-projection-successor-milestones`

Accepted decision (experiment only): `plans/adrs/ADR-0019-windows-sandbox-staged-workspace-projection.md`.
Research basis: `research/windows-native-sandbox/adr-0019-successor-assessment.md`.
Inherited blocked security gate: `plans/implementation/windows-native-sandbox/011-m004-native-adversarial-qualification.md`; `plans/closure/windows-native-sandbox/011-status.md`; C013 `plans/closure/windows-native-sandbox/013-status.md`.

Long-term requirements:
- `plans/000-long-term-specification.md#4-architectural-principles`
- `plans/000-long-term-specification.md#17-job-scheduling-and-execution-backends`
- `plans/000-long-term-specification.md#27-security-requirements`
- `plans/000-long-term-specification.md#29-system-invariants`
- `plans/001-terminology-and-domain-model.md#8-scheduler-and-execution-terms`

Other applicable ADRs: `ADR-0004` approval/profile independence; `ADR-0016` platform-owned launches and truthful obtained guarantees.

Primary class: infrastructure / content integrity.

## 1. Objective

Materialize an **independent** per-run projection snapshot from an authorized host workspace without carrying file object identity, links, NTFS extra streams, security descriptors, or secrets into the new tree. Produce a content/address/path manifest with bounded source freshness evidence and no host ACL mutations.

## 2. Readiness and hard gates

Hard: C014 must prove a usable, isolated AppContainer runtime with narrow grants. No assumption that C014 passes. Snapshot builder can otherwise be unit-tested cross-host using synthetic files, but its Windows-specific identity policy needs real Windows handle tests.

## 3. Repository evidence and security problem

Native ``CopyFileExW`` copies alternate data streams and some metadata; NTFS hardlinks share security descriptors; C013 found a credential hardlink reachable inside an allowed workspace. `std::fs::canonicalize` and textual denies are not sufficient when a path can be swapped. Current Codegg `SandboxLaunchSpec` is path-root based; this plan introduces a private immutable `ProjectionManifest` rather than changing the public agent protocol.

## 4. Invariants and boundaries

- Windows production `SandboxRequest::Required` remains **unavailable**; experiment/test-only runs must not return `SandboxExecutionOutcome::Enforced`, update public capability or select FullHost.
- Never mutate real source workspace ACLs, SACLs, integrity labels, original hardlinks, user profile permissions or daemon named-pipe DACLs. No elevation or new account without a separate accepted design.
- `ManagedProcessService` owns restricted process creation, token/Job before resume, all descendants, stdout/stderr and status; authenticated daemon retains authorization and workspace ownership.
- AppContainer package SID, runtime permissions and file projection are *obtained* access facts only when the native tests prove them. No network isolation claimed without separate actual tests.
- Inputs are untrusted when copied into the projection; outputs are untrusted after the child writes them. Neither is a direct authority to modify the host worktree.
- All file paths and resource ownership must be bound to a stable workspace/worktree/job identity; stale paths, unsupported metadata, security conflict or partial setup fail closed.

## 5. Production-code boundaries and intended modules

Add Windows-only `src/security/sandbox/windows_projection` or `src/managed_process/windows_projection` responsible for host snapshot **on the workspace node**. Capture workspace identity (stable project/workspace/lease identifiers from canonical context), original root opened handle, source file ID/volume/size/timestamps/hash, relative path, original authority, staged file ID/hash, and exclusion outcome. Keep `read_paths`/`deny_paths` security interpretation at the trusted daemon policy boundary. Create new destination files with narrow DACLs, no hardlink/reparse/ADS carry-over and protected staging root. No process execution integration in C015.

No unrelated UI, editor, human shell, distributed daemon, branch-wide merge or protocol refactor is permitted in this plan. Add public protocol/DB fields only after recording an explicit compatibility decision; tests and private types are preferred at this stage.

## 6. Ordered work packages

### WP-A — Data contract and bounded inclusion policy
Define explicit include/exclude list, allowlisted regular-file types, secret and deny-path precedence, plugin-friendly error taxonomy, maximum file count/bytes/depth/time, and sparse/EFS/cloud/ADS rejection. Do not indiscriminately copy entire source trees or hidden config. If any source has `nNumberOfLinks > 1`, **reject for initial milestone** even when one alias is inside workspace. Deny ambiguous casing/collisions and file IDs that alias forbidden objects.

### WP-B — Handle-bound safe reads
Walk starting from a pinned authorized root; open each child with reparse-point suppression as appropriate and verify path components/volume/file IDs. Refuse symlink/junction/mount point, outside volume, NTFS ADS, device/UNC, unsupported sparse or encrypted streams. Read only approved unnamed data stream from an opened handle and prevent (or detect and reject) file identity/content drift between initial manifest, copy and final hash. Stop for concurrent source replacement.

### WP-C — Independent staging writes
Create files as brand-new objects with explicit least-privilege DACL; never link to originals, never use a copying API that carries NTFS streams or source ACLs by default. Verify staged file ID differs from source and staged hardlink count equals one; copy content with bounded streaming and fsync/flush policy. Stage workspace root is private and no child access is granted until all manifests and permissions validate.

### WP-D — Journal and recovery
Atomically publish a versioned immutable manifest with snapshot generation and source/staged hashes. Owner-scoped per-run directories, idempotent cleanup and crash recovery; fail or quarantine changed partial content. No global user ACL or source file operation with write access.

## 7. Acceptance criteria

- Inputs with hardlinks, ADS, reparse point or ambiguous metadata are **refused**, not silently copied; no secret content or source NTFS ACL metadata may escape through the stage.
- Every accepted file is an independently created destination object with different file ID, fresh DACL, no extra streams/links, reproducible hash and pinned source identity.
- No host workspace file or ACL changes from snapshot; permission denied/changed source aborts atomically.
- Tests prove copied bytes are a snapshot, do not claim a live view, and stage size/file/time bounds are enforced.

## 8. Failure, cancellation, recovery, concurrency and stop conditions

Abort before any AppContainer launch on changes, link/reparse mismatch, bound overflow, secrets/deny input or incomplete copy; clean up owned staging directories. Do not delete foreign data if unexpected directory ownership/descriptor drift. Concurrent snapshots bind separately to workspace lease/job IDs. If a fully live workspace snapshot is required to support named workloads and cannot be stabilized, stop and record limitation rather than add unsafe traversal.

## 9. Verification requirements

Cross-host pure-model tests (manifest, conflict, bounds, path joins). Native Windows new test `cargo test --locked -p codegg --features windows-sandbox-test-support --test windows_projection_snapshot -- --nocapture --test-threads=1`; synthetic hardlink-to-secret, junction/symlink, ADS, protected ACL, identical casing, rename race, timestamp/size drift, large file and cancellation. Confirm no source ACL changes and fresh target ID. Run C014 tests, policy/ownership guards, fmt/quick on same SHA.

All closure records must include exact SHA, runner/OS build, Rust version, workflow job link, **selected nonzero test counts**, actual child stdout/denied operation results, skipped native/privileged coverage and known regressions. A preflight refusal, compilation-only test, model statement or parent-generated status frame never substitutes for an actual running restricted-child security assertion.

## 10. Documentation and migration

Internal manifest version and cleanup journal documented; no durable public schema/API, profile or permission migration. Update `architecture/security.md`, `architecture/testing.md` and Windows projection runbook with limitations. Do not add a user-facing setting until the security pipeline is qualified.

## 11. Closure record and handoff disposition

C015 strict positive closure enables C016 test-only launch integration; a partial/blocked/negative materializer does not. Staged bytes are untrusted input and source freshness must be rechecked at promotion.

Write `plans/closure/windows-native-sandbox/015-status.md` with strict positive or negative evidence, separate from historical `004-status.md`, `011-status.md`, `012-status.md`, `013-status.md`. Update the registry and roadmap only on verified results. A negative feasibility result is a valid *closed-negative* plan, not a reason to mark an unsupported production feature ready.
