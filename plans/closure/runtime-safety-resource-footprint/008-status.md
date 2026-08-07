# Runtime Safety, Resource Control, and Footprint Milestone 008 — Closure Status

Status: conditionally closed

Source implementation plan:

- `plans/implementation/runtime-safety-resource-footprint/008-planning-verification-and-maintenance-closure.md`

Source subsystem roadmap:

- `plans/subsystems/runtime-safety-resource-footprint-roadmap.md`

Repository baseline reviewed: `bd9678dd0bf679ada8d5e59a62e57c6efe05fe39`

Implementation commits:

- `20555d94` — reconcile runtime-safety documentation, static-guard ownership,
  and bounded local/hosted verification
- `c29caf4` — record the conditional closure, roadmap disposition, and registry
  audit
- `d8ecd30` — remove the incidental internal grep batch count from the stable
  tool contract

## 1. Executive finding

M008 production and governance work is complete. Architecture, security,
process, search, dependency, storage, binary-topology, testing, CI, release,
execution-ownership, and planning documentation now describe the accepted
repository behavior. The active registry is compact, the runtime-safety
roadmap has a complete status table, and M001–M007 plus corrective C001 each
have one discoverable closure record.

Strict closure remains conditional on two external evidence items that cannot
be produced by this Darwin host: one Landlock-capable Linux fixture run and a
hosted `verify` result for the final combined revision. These are explicitly
recorded operational evidence conditions, not hidden implementation defects
and not new corrective milestones.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| M001–M007 have accepted dispositions and compact records | Runtime-safety closure directory, roadmap status table, prior accepted revisions | pass | M001/M002/M005/M006/C001 remain conditional only for named operational evidence; M003/M004/M007 are closed. |
| Architecture and security docs match sandbox behavior | `architecture/security.md`, `architecture/python_scripting.md`, C001 closure | pass | Maintained Landlock helper, required fail-closed behavior, portable fallback, ABI reporting, profiles, and child-only scope are stated. |
| Execution docs match process/argv ownership | `architecture/jobs.md`, `docs/execution-ownership.md`, `docs/execution-ownership.toml`, M002/M003 closures | pass | Typed argv, bounded streams, timeout/cancellation, Unix process groups, helper status channel, exemptions, and guard ownership agree. |
| Search behavior is bounded and stable | `architecture/tool.md`, M004 closure, current `src/tool/grep.rs` | pass | Worker batches, permit lifetime, cancellation/result caps, deterministic ordering, and single context read are documented without incidental batch API promises. |
| Dependency/parser maintenance is truthful | `docs/dependency-maintenance.md`, `architecture/storage.md`, M005/M006 closures | pass | Feature ownership, maintained YAML compatibility boundary, manual maintenance, and storage layout v35 are current. |
| Binary topology is accurately documented | `architecture/overview.md`, `RELEASING.md`, M007 closure | pass | The measured single-binary/no-split decision is recorded; unsupported daemon/TUI packages are not advertised. |
| Test-count and verification claims are consistent | `architecture/testing.md`, `AGENTS.md`, `scripts/verify.sh`, CI workflow | pass | Fragile global totals were removed; serial test-binary behavior is distinguished from Tokio runtime flavor. |
| Routine CI remains one bounded, non-release job | `.github/workflows/ci.yml`, `architecture/testing.md` | pass | One `verify` job; no release, matrix, artifact, audit, benchmark, or size gate was added. |
| Registry is compact and downstream blockers are audited | `plans/registry.md`, roadmap status table | pass | No registered future plan remains blocked or became dependency-ready; external evidence remains explicitly named. |

## 3. Production implementation evidence

M008 required no runtime feature change. The narrowly scoped guard correction
made `check_execution_ownership.py` fail closed when production source cannot
be read, while retaining its existing manifest, finite-process, and typed-argv
checks. The guard's negative fixtures remain covered by `--self-test`.

The verification contract now runs the execution-ownership self-test and
normal scan in `scripts/verify.sh quick`; the same YAML, sandbox, and
execution-ownership checks run inside the existing hosted CI job. The explicit
workspace `cargo check` remains in place because it provides an actionable
default-feature/all-target compile boundary before Clippy and tests.

The remaining changes are documentation and planning reconciliation:

- sandbox platform/fallback and daemon-scope semantics, with the existing
  Python execution documentation cross-checked;
- managed process, storage layout, typed argv, search, dependency, and release
  behavior;
- measured M007 no-split deployment topology;
- removal of fragile global test totals and correction of resource terminology;
- compact registry and complete roadmap status links.

## 4. Verification executed

### Commands run

```text
python3 scripts/check_execution_ownership.py --self-test   pass
python3 scripts/check_execution_ownership.py                pass
python3 scripts/check_sandbox_contract.py --self-test      pass
python3 scripts/check_sandbox_contract.py                   pass
python3 scripts/check_yaml_parser_boundary.py --self-test   pass
python3 scripts/check_yaml_parser_boundary.py               pass
git diff --check                                            pass
scripts/verify.sh quick                                     pass
```

`scripts/verify.sh quick` on the accepted implementation revision `20555d94`
also passed formatting, generated-agent/schema checks, Tokio flavor checks,
the codegg-core boundary check, and locked workspace/all-target compilation.

The existing hosted run [`CI run 31007071234`](https://github.com/dbowm91/codegg/actions/runs/31007071234)
is retained as prior broad evidence: all verification commands and tests passed
and the run failed only during the Rust-cache post-step while writing
diagnostics. It predates `20555d94` and is therefore not claimed as final M008
hosted evidence. No hosted run was scheduled for the final direct-branch
revision, and the workflow has no manual dispatch trigger.

The supported-Linux fixture was not run because this host is Darwin. The exact
future command is:

```bash
cargo test --test sandbox_landlock -- --test-threads=1
```

The future evidence must record the Linux kernel, effective Landlock ABI, and
all fixture outcomes; an unsupported-kernel skip is not enforcement evidence.

## 5. Invariant review

- Routine CI remains one bounded non-release job with `CARGO_BUILD_JOBS=1` and
  `--test-threads=1`.
- Local quick verification remains the ordinary handoff command and now owns
  the cheap parser, sandbox, and execution-ownership guard checks.
- Managed process output, timeout, cancellation, descendant, and sandbox
  semantics remain documented as implemented by M001–M003.
- Landlock required mode remains fail-closed; portable fallback is labeled as
  non-OS isolation; the daemon is not confined by a child-only policy.
- Typed argv remains distinct from explicit raw-shell routing.
- Grep remains bounded, cancellable, deterministic, and non-indexed.
- Dependency reduction retains supported features and legacy namespace reads;
  no binary-size gate or automated dependency/release machinery was added.
- The current single executable and user-scoped singleton daemon contract remain
  the supported deployment topology.
- Historical closure evidence remains under `plans/closure/`; no history was
  deleted or rewritten.

## 6. Failure and recovery review

No durable job, process, storage, protocol, or recovery implementation changed
in M008. The guard change fails verification rather than silently accepting an
unreadable source file. CI and local verification stop at the first failed
command. A hosted cache/post-step failure is distinguished from a compile or
test failure. The missing Linux result is distinguished from a Linux test
failure and remains an evidence condition.

No cancellation, restart, contention, migration, or duplicate-delivery
semantics were changed. Existing M001–M007 records remain authoritative for
those mechanisms.

## 7. Migration and compatibility review

No database migration, protocol change, CLI change, configuration change, or
release-package migration was introduced. `architecture/jobs.md` and
`architecture/storage.md` now agree with the current `STORAGE_LAYOUT_VERSION =
35`; this is documentation correction, not a schema change. Existing YAML
frontmatter remains readable through the maintained compatibility codec, and
manual release/publication ownership is unchanged.

The registry de-registration is a planning-control-surface change only.
Roadmap, implementation-plan, and closure links remain valid.

## 8. Security review

The M008 guard change is fail-closed and has a negative self-test. The sandbox
documentation accurately distinguishes required Landlock enforcement,
portable fallback, disabled execution, ABI reporting, target output versus
private setup status, and the unsandboxed daemon boundary. No secrets,
credentials, path policy, authorization, or process authority changed.

No critical, high, or product-correctness medium security finding remains open.
The supported-Linux run is an explicit medium evidence gap, not an unreviewed
security claim.

## 9. Documentation and operations

Updated:

- `architecture/security.md`, `architecture/jobs.md`, and
  `architecture/storage.md` now match accepted sandbox/process/storage
  behavior; the existing `architecture/python_scripting.md` contract was
  cross-checked and remains accurate;
- `architecture/testing.md`, `AGENTS.md`, and `scripts/verify.sh` define one
  bounded local/hosted verification contract without fixed global totals;
- `docs/execution-ownership.md` documents guard scope and fail-closed behavior;
- `docs/dependency-maintenance.md`, `architecture/overview.md`, and
  `RELEASING.md` record feature ownership, manual maintenance, and no-split
  binary topology;
- `.github/workflows/ci.yml` keeps all checks in the existing single job;
- `plans/registry.md` and the runtime-safety roadmap now expose only current
  control-surface state while preserving historical links.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| medium evidence gap | Supported-Linux Landlock enforcement has not been executed on a Landlock-capable Linux host | Strict M001/M002/C001/M008 sandbox evidence remains incomplete | Run the existing `sandbox_landlock` fixture and record kernel, ABI, and outcomes; do not create a new milestone. |
| low operational | No hosted `verify` result exists for the final direct-branch revision | Final combined-tree hosted evidence is unavailable; prior run is older and had a cache post-step failure | Use the normal PR-triggered CI run when available and attach its URL to this closure; no new workflow lane or manual-dispatch workflow is required. |

No critical, high, or hidden production medium finding remains.

## 11. Roadmap disposition

M008 is conditionally closed. Production implementation and documentation /
governance reconciliation are complete, and the condition is limited to the
two named external evidence items above. The runtime-safety roadmap is marked
conditionally closed with a complete milestone table. It should move to strict
`closed` only after the existing Linux fixture and final hosted verify evidence
are recorded.

No corrective implementation plan is required. No future registered plan was
unblocked by this closure because the registry contained no remaining blocked
runtime-safety plan and no other registered dependency graph names M008 as a
hard or interface dependency.

## 12. Registry updates

- Marked the M008 implementation plan implemented and linked this closure.
- Marked the runtime-safety roadmap conditionally closed and added the complete
  M001–M008/C001 status table.
- Removed completed runtime-safety milestones from dependency-ready, active,
  and blocked registry sections.
- Added one recent-closure row for the workstream and retained the roadmap and
  closure-directory links for historical traceability.
- Audited all blocked work and affected roadmap dependency graphs: nothing
  became ready, and no new corrective plan was warranted.
- Retained manual release ownership, one bounded CI job, the minimal quick/full
  verification policy, and the explicit future Linux evidence condition.

## C002 final-integration addendum

C002 completed the branch reconciliation, PR metadata correction, and final
verification scheduling on `d781fdc`. M008 remains conditionally closed only
because the existing bounded workflow does not emit the supported-Linux
Landlock kernel, effective ABI, and fixture outcome details required for strict
closure. No workflow lane, artifact mechanism, or C003 was added.
