---
name: security-hardening
description: Review CodeGG changes against explicit trust boundaries and bounded inputs
version: 1.0.0
tags: [security, sandbox, authorization]
---

# Security hardening

Treat workspace content, plugin data, provider responses, and skill metadata
as untrusted input. Keep authority in the existing permission, sandbox,
authorization, and execution owners. Validate paths at the point of use,
bound input and output sizes, and avoid including credentials or sensitive
file bodies in diagnostics.

For process or command changes, consult `architecture/security.md`,
`architecture/permission.md`, and `architecture/approval_reviewer.md`, then run
the relevant sandbox and execution-ownership guards from `AGENTS.md`. A new
authority boundary or public security contract needs the repository's planning
and ADR process; a documentation guide does not establish permission.

Prefer deterministic fixtures for traversal, symlink, malformed input,
oversized payload, stale state, cancellation, and unauthorized action cases.
Record which cases ran and what remains unverified. Do not present a static
guard or compilation as proof of end-to-end security behavior.

On Windows, Job Object process-tree ownership is not filesystem or network
containment. Keep constrained requests fail-closed until native token/ACL
behavior and any private enforcement-status handoff have observed Windows
evidence; the platform guarantee matrix lives in `architecture/security.md`.
Drive/UNC/ADS/reparse checks on policy path strings are pre-launch diagnostics;
they do not establish handle identity, detect hardlink aliases, or prevent a
path replacement after validation. Keep those cases as native enforcement
gates for M004/M006.

The C010 AppContainer ACL fixture is only for fresh runner-owned disposable
roots. It journals the original and expected DACLs before mutation, verifies
the directory volume/file identity on recovery, restores only an exact
descriptor match, and preserves drift as a conflict. Its temporary test journal
is not the production daemon-owned storage path. Do not treat its allowed/denied
traces as production backend or existing-workspace authority evidence.
