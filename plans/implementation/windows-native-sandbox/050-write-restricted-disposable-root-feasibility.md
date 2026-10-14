# Windows Native Sandbox C050 — WRITE_RESTRICTED token and disposable-root feasibility

Status: ready — strictly test-only synthetic fixture; no production admission
Class: infrastructure (native feasibility)
Baseline: main aa6cbcb1e154a2676325fb654c5b68dc068edfc3 (refresh at execution).
Source roadmap: plans/subsystems/windows-native-sandbox-roadmap.md (post-C043/C044 successor).
Authority: ADR-0004, ADR-0016, ADR-0029 bounded test-only, C042 internal guarantee contract. ADR-0030 proposed/not implementation authority.
Dependencies: C041 and C042 hard (closed); C049 interface/soft (no public profile required for a private probe); an actual Windows runner is operational.
Closure target: plans/closure/windows-native-sandbox/050-status.md.

## Objective

Determine whether a Windows WRITE_RESTRICTED token, a known restricting SID, correctly provisioned *new Codegg-owned disposable NTFS roots*, and existing ManagedProcessService suspended/Job lifecycle can confine filesystem writes while allowing realistic native subprocesses — **without any edit to the original user worktree or its security descriptors**. This is a broad-read/write-restricted candidate, NOT a replacement for strict ReadOnly/WorkspaceWrite. C043/MXC and C044/two-hop are not prerequisites.

## Work packages

A. Build a Windows-only, feature-gated test-support fixture at the existing managed-process/test-support seam. Use disposable synthetic roots created for the test, with creation-time DACLs permitting appropriate restricting-SID write access only to the stage/scratch. Inspect the actual ordinary and restricting token SIDs, LUA/UAC groups, privileges, integrity level, token type, access check behavior, and account/user access under every tested host. Never stamp ACEs or low integrity on preexisting original host paths; record before/after owner/DACL/SACL/MIC and content hashes on original canary paths.

B. Spawn with restricted token under the existing suspended→Job assign→resume ownership path; maintain explicit stdio-only handle allowlist, verified environment/cwd, deterministic process-tree termination and immutable request fingerprint. Verify no target entry on preflight/Job/attestation failure; preserve C041 release guard and prevent exposure through production registry, public config or agent shell selection. If implementing WRITE_RESTRICTED requires separate per-object grants on original existing files, stop that variant.

C. Run *positive controls*: restricted child reaches first instruction, reads ordinary Windows runtime/toolchain files, reads and writes stage/scratch, Git init/status/add/commit, cmd, PowerShell, Python with virtualenv and offline Cargo build/test where installed. Measure cold/warm timing and capture environment/toolchain/host edition/build/UBR/arch. Explicitly state filesystem host reads outside stage are not confined and might include ordinary user-readable credentials; do not probe real secrets.

D. Run *negative controls* against synthetic canaries: outside-stage create/modify/append/delete/rename, parent-directory DELETE_CHILD, existing-file write through hardlink/junction/symlink/8.3/extended paths/ADS, TOCTOU path replacement, external child/helper writes and shell→Python→Git descendants. Test pre-opened writable handles, inherited and DuplicateHandle process/token/file handles, low-integrity or other permissive named objects, named-pipe/COM-mediated write attempts where feasible; classify untested IPC bypasses as unqualified, not denied. Include explicit independent unrestricted control for each denied operation to distinguish environment failures.

E. Report exactly which writes the token actually blocks. Access to registry/services/IPC, sockets and process memory are **separate dimensions** and cannot be inferred from filesystem results. Check exact parent/child process exit and Job containment across timeout, cancellation and injected mid-launch failure. Use stable runner evidence: exact Git SHA, toolchain, OS, token identity, SID ACL snapshots, individual operation result, invocation nonce and teardown evidence, without leaking synthetic credential bytes into logs.

## Hard stop / required acceptance

A child that can write the forbidden canary, a process surviving required cleanup, a writable inherited handle escaping the root, an original-worktree descriptor mutation, or a pre-entry policy failure that still starts target code is a negative result. Do not relax restrictive ACLs, add global privileges, amend existing-root DACLs or silently switch to FullHost to make tests pass. Native success may qualify **only a test-only broad-read write-confinement hypothesis**, never strict read confinement, network Offline, production backend or ADR-0030 acceptance. Record pass/fail/unsupported/not-run distinctly and close bounded positive/negative/blocked after one defined correction cycle.

## Expected code / compatibility / verification

Changes restricted to test-support launch adapters and focused Windows fixtures; no production SandboxBackend registration, source-profile enum, IPC/storage migration or operator install. Suggested checks: cargo fmt --all -- --check; git diff --check; python3 scripts/check_adr_integrity.py; python3 scripts/check_sandbox_contract.py; python3 scripts/check_execution_ownership.py; bash scripts/verify.sh quick; focused Windows process-launcher and new test feature suite at an exact pinned SHA. Keep existing Linux/macOS constrained behavior unchanged. Closure must include evidence table, no-mutation snapshot comparison and explicit blockers for C051/C046/C048.
