# ADR-0028 — Explicit broad-read Windows compatibility profile

Status: proposed — **not authorized for implementation or user-facing selection**
Date: 2026-10-13
Decision owners: Codegg maintainers
Related: ADR-0004, ADR-0016, ADR-0027; plans/subsystems/windows-native-sandbox-roadmap.md; section 27/29 of plans/000-long-term-specification.md.

## Context and drivers

Existing ReadOnly and WorkspaceWrite profiles represent explicit authorization ceilings and obtained OS filesystem confinement. Codex/Claude-style dedicated-account execution commonly emphasizes write confinement and developer-tool compatibility while permitting broader host reads through OS permissions. A broader read scope is not equivalent to confidentiality of host credentials. Overloading WorkspaceWrite would silently weaken existing promises, especially parent/subagent inherited ceilings and automated approval.

## Options

A. Preserve existing profile semantics; provide no broad-read compatibility profile. Strongest compatibility with existing expectations, but may leave native Windows everyday tooling unavailable on hosts without strict-read backend.

B. **Propose a separately named profile** (candidate: WorkspaceWriteBroadRead) with explicitly documented broad host-read risk, workspace/scratch-scoped writes and independent network policy. Selected direction for maintainers to consider; name and external schema are not frozen.

C. Quietly reinterpret WorkspaceWrite on Windows as broad-read. Rejected as an authority downgrade and cross-platform semantic drift.

D. Default to FullHost when strict Windows containment unavailable. Rejected: explicit FullHost requires an independent authorization event under ADR-0004.

## Proposed decision pending ratification

Add a separate, operator-selectable, versioned profile only if maintainers explicitly approve its threat model, default/upgrade behavior, UI wording and permission-audit representation. Broad-read means read access beyond the workspace is **not denied by the sandbox**; it does not imply that every host path is universally readable. Host-secret protections must be independently claimed only if proved. Writes, privileged IPC, process ownership and network guarantees remain independently enforceable and reportable.

The new profile MUST NOT broaden existing ReadOnly/WorkspaceWrite, grant automatic escalation, alter an in-flight policy snapshot, or become a permissive agent-selected alternative. When a parent grants a stricter read ceiling, a subagent cannot select this profile. Missing write confinement is unavailable/fail-closed even when reads are broad. Future cross-platform semantics require a compatibility/migration matrix and owner-approved config defaults.

## Consequences and migration

If accepted, update typed public config/protocol only with explicit versioned compatibility, tests for old serialized profiles and names, human-readable risk presentation, audit/approval receipts and safe defaults. Persisted sessions must retain their originally authorized ceiling. No network=Offline claim without WFP evidence. If rejected, C042 can still deliver internal guarantee negotiation without adding a public profile.

## Verification and admission

Before acceptance: run a proposed threat-model and policy-matrix review (strict parent to broad child denied; missing backend denied; FullHost never inferred; offline independently qualified; host read allowed/denied accurately recorded). Acceptance does not by itself admit a Windows backend. A separate independently reviewed backend/release decision remains necessary.

## Supersession

None; this proposal changes nothing until ratified.
