# Domain Identity Milestone 004 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/domain-identity/004-closure-and-legacy-removal-criteria.md`

Source subsystem roadmap:

- `plans/subsystems/domain-identity-roadmap.md#milestone-4--closure-and-legacy-removal-criteria`

Repository baseline reviewed: `ab9d345` (`agent/project-catalog-lazy-activation-health`)

Implementation commits or pull requests:

- `f203ed9` — typed identity foundation and validation/guard seam.
- `84d92f0` — additive project/repository storage migration and canonical bindings.
- `ec42dce` — daemon/protocol adoption corrective pass; canonical context became
  authoritative for new daemon-owned session operations.

No production-code commit is claimed for Milestone 004. This milestone records
the closure evidence and removal criteria for the already-landed identity
boundary.

## 1. Executive finding

Milestone 004 is closed. The Domain Identity roadmap's completion definition is
met across the closed M001–M003 implementation boundaries: project identity is
durable and path-independent in storage, daemon execution and protocol surfaces
use canonical project/workspace context, existing sessions remain readable or
fail with bounded diagnostics, and compatibility behavior is explicit.

The remaining historical fields are not silently promoted to authority and are
not removed here. Their owners and removal prerequisites are recorded below;
Project Catalog 004 owns the remaining single-project server locator migration.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Typed, path-independent identity primitives and relations | M001 closure `plans/closure/domain-identity/001-status.md`; `codegg-core::identity`; current identity-path guard | pass | IDs validate independently of filesystem paths. |
| Additive project/repository/workspace/session storage migration | M002 closure `plans/closure/domain-identity/002-status.md`; migration and project-storage fixtures | pass | Canonical binding tables are additive; historical values remain readable. |
| Daemon and protocol authority uses canonical context | M003 corrective closure `plans/closure/domain-identity/003-corrective-status.md`; `context.rs`, session store, daemon, protocol, and server changes | pass | New executable session paths resolve and persist canonical bindings. |
| Representative migration is restart-safe and conservative | M002 closure; `storage_migrations`, `project_storage`, and context fixtures | pass | Ambiguous/unresolved cases remain inspectable rather than guessed. |
| No silent path-derived identity fallback in production writes | M003 corrective closure; `check_identity_path_usage.py`; context directory-compatibility tests | pass | Directory compatibility only finds one existing canonical binding. |
| Legacy rows and compatibility fields have owners and removal gates | §7 below and source plan §9 | pass | No field is removed until its owning subsystem proves the listed gates. |
| Identity behavior does not grant authorization | M001–M003 security reviews and context validation boundary | pass | Parsing and membership resolution are not authorization decisions. |
| Future-plan dependencies were audited | Registry and downstream plan review | pass | No plan has a hard dependency on Domain Identity 004; existing Project Catalog/TUI/projection blockers remain. |

## 3. Production implementation evidence

Milestone 004 adds no runtime code. The production boundary it closes is the
combination of:

- typed `ProjectId`, `RepositoryId`, `WorkspaceId`, and relation contracts;
- additive project/repository/workspace/session binding storage and deterministic
  migration/rebinding;
- bounded context resolution for explicit IDs and existing-directory lookup;
- atomic canonical session binding for create/import/fork/template paths;
- additive identity-bearing protocol DTOs and capability negotiation;
- daemon, REST, WebSocket, and snapshot adoption of canonical context; and
- static guards preventing new path-derived identity and cwd-based authority.

These are implemented and evidenced by `f203ed9`, `84d92f0`, and `ec42dce`.
The current checkout also contains Project Catalog M003 work, but that separate
milestone does not change the Domain Identity ownership boundary.

## 4. Verification executed

### Commands run on the reviewed checkout

```bash
rtk cargo test -p codegg-core --test context -- --test-threads=1
rtk cargo test -p codegg-core session -- --test-threads=1
rtk cargo test -p codegg-protocol -- --test-threads=1
rtk cargo test --test storage_migrations -- --test-threads=1
rtk cargo test --test workspace_isolation -- --test-threads=1
rtk cargo test --test workspace_services_isolation -- --test-threads=1
rtk cargo fmt --all -- --check
rtk bash scripts/check-core-boundary.sh
rtk python3 scripts/check_daemon_cwd_usage.py
rtk python3 scripts/check_execution_ownership.py
rtk python3 scripts/check_identity_path_usage.py
rtk python3 scripts/check_project_catalog_invariants.py
```

### Results

- Context: 6 passed; session: 43 passed; protocol: 91 passed.
- Storage migration: 4 passed; workspace isolation: 6 passed; workspace
  services isolation: 11 passed.
- Formatting passed.
- Core-boundary, daemon-CWD, execution-ownership, identity-path, and
  project-catalog guards passed; project-catalog reported 7/7 checks.
- The capped all-features workspace result is inherited from the M003 corrective
  closure: 3,814 passed with five environment-restricted socket/server failures;
  no changed identity, protocol, session, daemon, server, workspace, or TUI test
  failed. No production code changed for M004, so the full suite was not rerun.
- Repository-wide clippy remains a known pre-existing non-clean gate recorded in
  the M003 corrective closure; this documentation-only closeout introduces no
  changed Rust code or new lint surface.

## 5. Invariant review

- **Stable identity:** canonical IDs and binding tables, not paths, carry project
  identity; the identity path guard passes.
- **Canonical execution context:** daemon-owned create/import/fork/template,
  hydration, listing, and runtime binding resolve project/workspace context as
  recorded by M003.
- **Compatibility preservation:** legacy rows and projections remain readable;
  canonical binding is authoritative and compatibility values are derived or
  retained without destructive rewriting.
- **Conservative migration:** additive schema and deterministic reconciliation
  preserve ambiguous/unresolved state for later operator action.
- **No authorization confusion:** identity parsing and lifecycle/membership
  validation do not substitute for future principal authorization.

## 6. Failure and recovery review

- Duplicate registration converges through canonical uniqueness and binding
  transactions.
- Migration is idempotent and restart-safe; failed binding does not expose an
  executable session without canonical context.
- Missing or ambiguous directory compatibility returns a bounded
  context-required outcome rather than creating identity from text.
- Legacy storage-level reads remain available, while daemon/server execution
  rejects unresolved context before action.
- No new cancellation, process-spawn, scheduler, or lease authority is added by
  this closure milestone.

## 7. Migration and compatibility disposition

| Surface | Owner | Removal prerequisites |
|---|---|---|
| Historical `project` table and path-valued provenance | Project storage/catalog migration | Canonical catalog/binding records serve every durable consumer; unresolved rows have explicit disposition; rollback/read compatibility is no longer required. |
| `session.project_id`, `session.workspace_id`, `session.directory` | Session storage and protocol compatibility | All supported clients consume `SessionBindingDto`; rows are backfilled or explicitly unbound; reads/writes no longer require projections; additive rollback evidence exists. |
| `ServerState.project_dir` and single-project route locators | Project Catalog 004 | Project-scoped REST/WS uses catalog IDs and explicit workspace/locator context; default-locator compatibility is tested; no route treats the field as identity. |
| Directory-valued `CoreRequest::SessionList.project_id` input | Daemon compatibility adapter | Stable-ID capability adoption is complete; old clients receive bounded context-required/unsupported behavior; no production write accepts it as authoritative identity. |

No compatibility field is removed by this closure. Any removal must be a later
owner plan with explicit migration, rollback, client-adoption, and negative-test
evidence.

## 8. Security review

- Identity values are bounded and validated; path-like compatibility data is not
  used to grant authority.
- No new filesystem scan, cwd lookup, shell command, network operation, or
  process-spawn path was added.
- Static core-boundary, daemon-CWD, execution-ownership, identity-path, and
  project-catalog guards pass.
- Authorization remains a separate future capability; this closure does not
  imply that a valid `ProjectId` authorizes a principal.

## 9. Documentation and operations

Added or updated:

- This implementation handoff and closure record.
- `plans/subsystems/domain-identity-roadmap.md` Milestone 4 and roadmap status.
- `plans/registry.md` active/recently-closed status and dependency review.

The M001–M003 architecture and guard documentation remains authoritative. No
operator migration command is added because no schema or compatibility field is
removed by this milestone.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| low | Historical compatibility fields remain. | They preserve old-client/data compatibility and could be misused by future code if ownership is ignored. | Keep the listed owners and gates; remove only through later subsystem plans. |
| low | `ServerState.project_dir` remains as a compatibility locator. | Full project-scoped server migration is not yet complete. | Project Catalog 004 owns the protocol/REST/WS migration and its removal criteria. |
| low | Repository-wide clippy and some test-environment restrictions remain from prior closure evidence. | Broad hygiene/environment evidence is not fully clean. | Track with the owning maintenance/environment work; no M004 production code is affected. |

No medium, high, or critical finding remains within the Domain Identity
Milestone 004 scope.

## 11. Roadmap disposition

The Domain Identity and Compatibility subsystem roadmap is closed. No future
plan became newly dependency-ready solely because Domain Identity 004 closed:

- Project Catalog 004 remains the next active catalog handoff and owns the
  project-scoped server/protocol migration.
- Multi-Project TUI 001 remains blocked on Project Catalog 004 (and consumes the
  already-closed Runtime Assets interfaces).
- Session Projections 001 remains blocked on Project Catalog 004 and the
  project-aware TUI foundation; its Domain Identity 003 dependency is already
  cleared.
- No plan directly names Domain Identity 004 as a hard dependency.

## 12. Registry updates

- Added and closed the M004 implementation plan and this closure record.
- Marked the Domain Identity roadmap closed at Milestone 4.
- Removed Domain Identity M004 from active/dependency-ready work and added it to
  recently closed work.
- Preserved the Project Catalog 004, Multi-Project TUI 001, and Session
  Projections 001 blocker rows unchanged because this closure does not satisfy
  their Project Catalog/TUI dependencies.
