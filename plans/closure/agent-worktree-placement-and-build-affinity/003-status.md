# A003 — Repository-Scoped Build Admission — Closure Record

Plan: `plans/implementation/agent-worktree-placement-and-build-affinity/003-repository-scoped-build-admission.md`
Roadmap: `plans/subsystems/agent-worktree-placement-and-build-affinity-corrective-addendum.md`
Baseline: `80171f45295bebb9ab0b521262c0b295acb93074`
Implementation branch head: `5a479cea9924205c8828dcbeef72a40bf3391f63` (worktree changes remain uncommitted)
Implementation commit/PR: none; source and closure edits are uncommitted in the requested branch worktree.
Reviewed: 2026-10-27
Class: infrastructure
Status: **conditionally closed**
Recommendation: **conditionally closed**; real Cargo builds through the production scheduler executor still need paired baseline/limited measurements.

## 1. Requirement → evidence matrix

| Requirement | Evidence | Status |
|---|---|---|
| Repository bucket uses daemon-resolved canonical lineage, not request metadata | Submission overwrites caller quota keys from `ProjectContextResolver::repository_for_workspace`; test registers a real sibling Git worktree and catalog binding | met |
| Repository quota is optional and configuration-owned | `scheduler.quota.max_concurrent_builds_per_repository` defaults disabled; zero is rejected; callers cannot raise the configured cap | met |
| Sibling workspaces share a bucket and unrelated repositories remain independent | Typed `RepositoryBuild` quota and admission tests cover same-repository siblings, unrelated workspaces/repositories, duplicate keys, and unsupported job classes | met |
| Waiting jobs hold no slot and resume after release/cancellation | scheduler admission, permit lifecycle, cancellation, and resource-profile family tests; **49 passed** | met |
| Real scheduler submission shows one same-repository job waiting | `opt_in_repository_quota_is_bound_from_catalog_not_request_metadata`: real `JobSubmissionService` + `JobScheduler`, distinct workspace IDs bound to one repository; first starts, second stays queued with an admission-block metric, then starts after release | met |
| Two-worktree Rust build resource and lock-wait evidence is recorded without implying all shell commands are governed | paired cold-cache direct Cargo run below; the scheduler integration test uses a gated executor, not a production process executor | **partial; open condition** |

## 2. Paired build measurement

Host: x86_64 macOS. Source head: `5a479cea9924205c8828dcbeef72a40bf3391f63`.
Two clean linked worktrees of the same repository commit ran concurrently with
`CARGO_BUILD_JOBS=2`, one fresh shared `CARGO_TARGET_DIR`, and
`cargo check --locked -p codegg-core`. The first worktree finished in **3m39s**
with **305.82s user / 40.74s system CPU** and **1,710,993,408 bytes maximum
resident set**. The sibling Cargo process reported waiting for the build
directory lock, then finished in **4m09s total** with **28.30s user / 1.35s
system CPU** and **1,992,544,256 bytes maximum resident set**. The shared target
grew to **1.4 GiB**. Cargo's lock serialized these two direct invocations; this
single cold-cache sample is not a before/after scheduler quota benchmark, a
shared-cache recommendation, or evidence that arbitrary Bash is admitted by
the repository quota. The actual scheduler admission contract is established
by the separate real-submission test above. These measurement worktrees used
the checked-in branch head and did not contain the uncommitted implementation
patch. Thus this is useful Cargo contention evidence but does not compare a
baseline and repository-quota-limited production build path.

## 3. Production changes

Added the typed `RepositoryBuild` quota namespace, optional operator-owned
limit and validation, catalog-lineage resolution, unconditional submission
binding for eligible Build/Format jobs, deterministic aggregation of duplicate
keys, and bounded/redacted admission diagnostics. Existing workspace quota
behavior remains in place. Unknown quota namespaces are refused; arbitrary
Bash and direct external Cargo are not claimed as governed.

## 4. Verification

- Repository-lineage real scheduler test: **1 passed**.
- Build quota/resource profile/permit lifecycle integration family: **49 passed**.
- Core repository-scope context test and config validation test passed.
- `check_scheduler_bypass.py` and `check_execution_ownership.py` passed.
- `scripts/verify.sh quick` passed, including all repository guards, 354 harness tests (1 skipped), and workspace all-target check.
- `cargo clippy --workspace --all-targets --locked -- -D warnings` passed.
- `git diff --check` passed.

## 5. Compatibility and operations

The new scheduler quota limit is optional and defaults disabled; existing
workspace quota behavior remains unchanged. No storage migration is required:
the quota key uses the existing persisted resource-request shape. Direct Cargo
and uninspected Bash remain outside the admission claim. Metrics expose coarse
admission counts and do not include repository identifiers or command
metadata.

## 6. Condition preventing strict closure

Run the same two-worktree Rust build fixture as real Build submissions through
the production process executor and scheduler, once with repository quota
disabled and once with it enabled. Record wall time, CPU time, peak RSS, disk
growth, Cargo lock waits, and an unrelated-repository control on a qualified
host. The current gated-executor integration proves the admission boundary but
does not measure resource cost after admission. Until this condition is met,
A004 remains blocked on strict A003 closure.

## 7. Unblock audit

A003 is conditionally closed. A004 remains blocked on the comparative
scheduler-controlled build evidence above; this closure does not authorize a
shared writable target directory or install a compiler cache. A005 remains
blocked on A002, strict A003 closure, and A004 evidence.
