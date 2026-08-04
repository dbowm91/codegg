# Tool Programs Milestone 020 — Canonical Child-Artifact Recovery Corrective Closure

Status: implemented — closed; see `plans/closure/tool-programs/020-status.md`

Source subsystem roadmap: `plans/subsystems/tool-programs-roadmap.md`

## Objective

Record and close the narrow corrective implementation discovered during DVR
M007's shared workspace verification. Child-job completion must expose a
context-artifact handle that recovery can resolve and validate, and the M015
daemon-failpoint fixture must use the typed child-job command contract.

## Implemented scope

- place the persisted `child-job://…/summary` handle before opaque scheduler
  `run://` and `job://` metadata handles;
- record the canonical child handle and content digest in the child-artifact
  tracking record;
- use the repository's canonical bare content-hash helper for the stored
  artifact, matching executor validation;
- replace the obsolete `bash -lc sleep` fixture argv with the allowed typed
  `cargo build` argv.

No new authority, capability, scheduler policy, or artifact store was added.

## Verification contract

The M015 daemon-failpoint target, Tool Programs runtime target twice, and
authority-pipeline target pass on the accepted revision. DVR M007's quick and
hosted verification records are shared evidence for this corrective closure.

