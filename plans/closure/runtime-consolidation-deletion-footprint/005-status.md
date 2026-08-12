# Runtime Consolidation, Deletion, and Footprint M005 — Closure Status

Status: closed

Source implementation plan: `plans/implementation/runtime-consolidation-deletion-footprint/005-verification-ratchet-retirement.md`

Source subsystem roadmap: `plans/subsystems/runtime-consolidation-deletion-footprint-roadmap.md`

Repository baseline reviewed: `362101ef` (current `main` before this change)

Implementation commits: this closure and documentation contraction commit

## 1. Executive finding

M005 is closed. The verification surface was audited against the current
post-M001–M004 runtime. No security or correctness guard met the plan's safe
deletion condition: each retained guard protects a still-possible authority,
disclosure, sandbox, workspace, scheduler, transport, or compatibility
regression that is not fully prevented by Rust visibility or tests alone.

The documentation contraction removes the `AgentLoop` field inventory and
stale scheduler/LSP test totals. Architecture documents now describe stable
ownership and evidence locations rather than rapidly changing implementation
counts. Routine CI remains one bounded job with the same high-value guards,
formatting, Clippy, and workspace tests.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result |
|---|---|---|
| Every retained routine guard has a distinct invariant and owner | Guard disposition below; `.github/workflows/ci.yml`; `scripts/verify.sh` | pass |
| No high-value security invariant loses coverage | Sandbox, broker, projection, Git, identity, scheduler, and transport guards retained; focused tests remain documented | pass |
| Routine CI remains one bounded job | `.github/workflows/ci.yml` has one `verify` job, no matrix or schedule | pass |
| No duplicate compilation step added | Clippy remains the workspace compile/lint gate; no standalone CI check was added | pass |
| Architecture docs avoid stale implementation mirrors | `architecture/agent.md`, `architecture/scheduler.md`, and `architecture/testing.md` contracted | pass |
| Quick verification and Clippy remain required | Plan commands executed below | pass |
| No new scanner was introduced | `git diff` contains no new verification script | pass |

## 3. Guard disposition

| Guard | Classification | Decision and owner |
|---|---|---|
| `check-core-boundary.sh` | retained permanent | Crate dependency boundary; core maintainers; compiler checks are necessary but do not express the forbidden dependency policy alone. |
| `check_sandbox_contract.py` | retained permanent | Child-only sandbox and helper identity; security/runtime owners. |
| `check_execution_ownership.py` | retained permanent | Process-spawn and durable execution ownership; scheduler/runtime owners. |
| `check_scheduler_bypass.py` | retained permanent | Line-level scheduler fallback and direct-run bypasses; scheduler owners. Its per-line annotations are not equivalent to the file-level execution manifest. |
| `check_tool_broker_boundary.py` | retained permanent | Agent-to-broker authority; tool runtime owners. |
| `check_git_forbidden_patterns.py` | retained permanent | Secret-safe Git argv/environment and RunStore boundaries; Git/security owners. |
| `check_identity_path_usage.py` | retained permanent | Prevents path-derived durable identity; core identity owners. |
| `check_daemon_cwd_usage.py` | retained permanent/manual | Workspace authority remains possible at legacy/bootstrap boundaries; workspace owners. |
| `check_project_agent_pwd_inference.py` | retained permanent/manual | Deprecated compatibility constructors still exist; runtime-asset owners. |
| `check_discovery_invariants.py` | retained permanent/manual | Bounded metadata-only discovery and cancellation; core/catalog owners. |
| `check_project_catalog_invariants.py` | retained permanent/manual | Remote locator and migration safety; core/catalog owners. |
| `check_tui_project_authority.py` | retained permanent/manual | Prevents reintroduction of path/current-focus authority in the frontend; TUI owners. |
| `check_projection_disclosure.sh` | retained permanent/manual | Disclosure-class and path-free handle encapsulation; projection/security owners. |
| `check_projection_publication_seam.sh` | retained permanent/manual | Central publication ownership; projection owners. |
| `check_projection_transport_isolation.py` | retained permanent/manual | Prevents raw projection broadcast and subscription-derived identity; transport owners. |
| `check_projection_transport_lifecycle.py` | retained permanent/manual | Connection lifecycle, cancellation, and replay evidence; transport owners. |
| `check_websocket_bounds.py` | retained permanent/manual | Bounded server outbound queues; server/transport owners. |
| `check_provider_connections_m4_coverage.sh` | retained manual | Provider lifecycle API/protocol smoke contract; provider-connection owners. Detailed lifecycle tests are the primary evidence. |
| `check_provider_connections_tombstone_compat.sh` | retained manual | Additive migration/tombstone compatibility smoke contract; provider-connection owners. Core store tests are the primary evidence. |

Deleted migration guards: none. The audit explicitly rejected deleting a
guard when its invariant remained possible to violate and no stronger direct
enforcement existed. This is the plan's required stop condition, not an
unresolved implementation gap.

## 4. Production implementation evidence

No production Rust behavior or CI topology changed. The implementation is a
documentation and verification-surface contraction: stale exact counts and a
field-by-field `AgentLoop` mirror were removed, while the authoritative
source modules and focused test paths remain linked.

## 5. Verification executed

Commands run locally:

```text
python3 scripts/check_daemon_cwd_usage.py
python3 scripts/check_project_agent_pwd_inference.py
python3 scripts/check_discovery_invariants.py
python3 scripts/check_project_catalog_invariants.py
python3 scripts/check_scheduler_bypass.py
python3 scripts/check_execution_ownership.py
python3 scripts/check_tool_broker_boundary.py
python3 scripts/check_tui_project_authority.py
python3 scripts/check_projection_transport_isolation.py
python3 scripts/check_projection_transport_lifecycle.py
python3 scripts/check_websocket_bounds.py
scripts/check_projection_disclosure.sh
scripts/check_projection_publication_seam.sh
scripts/check_provider_connections_m4_coverage.sh
scripts/check_provider_connections_tombstone_compat.sh
scripts/verify.sh quick
CARGO_BUILD_JOBS=1 cargo clippy --workspace --all-targets --locked -- -D warnings
git diff --check
```

All commands passed. Hosted CI evidence is not required because the routine
workflow was not changed.

## 6. Invariant, failure, recovery, and security review

The audit preserved scheduler admission and line-level compatibility
exceptions, explicit workspace context, broker-only model execution, child
sandbox setup, path-free projection disclosure, typed Git safety, bounded
WebSocket delivery, and provider tombstone compatibility. No fallback,
retry, permission, identity, or recovery behavior changed.

## 7. Migration and compatibility review

No protocol, storage, feature, or user-facing compatibility contract changed.
Manual guards remain available for targeted maintenance; routine verification
continues to run only the bounded high-value set in `scripts/verify.sh` and CI.

## 8. Roadmap and dependency disposition

M005 is closed. M006 remains blocked because the M003 corrective physical
extraction is incomplete; M005 did not falsely mark the measurement pass ready
against the transitional `AgentLoop` tree. M007 remains blocked on M006.
No other registered plan became unblocked as a result of this closure.

