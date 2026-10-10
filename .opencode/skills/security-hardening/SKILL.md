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
