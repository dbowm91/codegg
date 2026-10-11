---
name: testing-ci
description: Select focused CodeGG verification and report CI evidence accurately
version: 1.0.0
tags: [testing, ci, verification]
---

# Testing and CI

Use this guide when choosing verification for a CodeGG change. The repository
verification entry point is `scripts/verify.sh`: `quick` runs formatting,
boundary and contract guards plus a workspace check; `full` adds Clippy and
the workspace test suites. Read `AGENTS.md` for the current exact command
definitions because the verifier evolves with the workspace.

## Choose evidence by change

- Start with the narrowest test target covering changed behavior.
- Run `scripts/verify.sh quick` for routine repository sanity.
- Run `scripts/verify.sh full` for broad handoff qualification when the change
  affects shared contracts or when the owning plan requires it.
- Run change-triggered guards named in `AGENTS.md`; a green CI subset does not
  imply every quick guard ran.
- Separate local command results from hosted CI results. Report the command,
  exit status, and any skipped or unavailable evidence.

Do not claim a test passed because a build succeeded. Do not run
`--all-features` for workspace sweeps: real-server tests need separately
installed servers. Use the feature set documented in `AGENTS.md`.

## Source of truth

- `scripts/verify.sh` defines quick and full verification.
- `architecture/testing.md` describes test classes and execution policy.
- `AGENTS.md` lists feature and change-triggered guard requirements.
