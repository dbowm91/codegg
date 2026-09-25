# Eggwork Remote Execution M002a — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/eggwork-fixed-target-remote-execution/002a-restricted-spec-live-requalification.md`

Source subsystem roadmap:

- `plans/subsystems/eggwork-fixed-target-remote-execution-roadmap.md`

Repository baseline reviewed: `e57c1499425c61b9405132363b5e7f2b2b9c2c81`

Implementation commits:

- `fba3ca11` — pin corrected Eggwork C001 revision and implement trusted Landlock fixture, required-isolation live execution, policy regressions, and operator documentation.
- `de8272a9` — complete architecture and operations documentation for the live-qualified policy.
- `e57c1499` — move M002a to closing while collecting closure evidence.

## 1. Executive finding

M002a is complete and closed. CodeGG pins the exact reviewed Eggwork C001 commit `6cc813418c3f14740a635fef79208e85219175bb` in production and the standalone live fixture. Hosted Linux CI exercised CodeGG's production mTLS client against the fixture's real Eggwork `NodeServer` with trusted Landlock setup and the same-pin `eggwork-sandbox-helper`. A required-isolation job succeeded inside its workspace, reads and writes outside the workspace were denied, and terminal evidence reported the applied `workspace_rw` profile. Capability and status feature views agreed. Restart, lease, scheduler-only dispatch, and unsupported-policy regressions passed. Disabled networking remains unsupported and is rejected before upload.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Pin the reviewed corrected Eggwork implementation consistently | Root and fixture manifests plus lockfiles use `6cc813418c3f14740a635fef79208e85219175bb`; root locked workspace check passed | pass | Exact revision matches Eggwork Security C001 closure; no feature/dependency widening |
| Use trusted helper setup from the pinned Eggwork workspace | `crates/eggwork-test-node` uses `TrustedLandlockSetup::discover_sibling()`; Linux fixture builds `eggwork-sandbox-helper` from the resolved pinned workspace | pass | No `NoExecutionSetup` qualification path |
| Require fresh authenticated capability/status agreement before upload | `live_node_connection_capabilities_and_workspace_isolation`; scripted capability-removal test | pass | Both views must carry `isolation.landlock.workspace-rw.v1`; conservative intersection rejects mismatch before upload |
| Execute required isolation through CodeGG's production mTLS path | Hosted CI `36081751855`, live test `live_node_connection_capabilities_and_workspace_isolation` | pass | Real live node and production executor, not a scripted executor seam |
| Permit workspace access and deny filesystem escape | Same live test | pass | Workspace file write/read succeeds; `/etc/passwd` read and `/tmp` write are denied |
| Report applied terminal isolation evidence | Same live test asserts `SandboxResult::Applied { profile: "workspace_rw" }` | pass | No inference from advertised capability alone |
| Refuse unsupported disabled networking before upload | `live_restricted_policies_refuse_before_workspace_upload`; scripted policy regression | pass | No `network.disabled.v1` claim or synthetic backend |
| Preserve lease and restart invariants under required isolation | Hosted live tests `live_lease_fencing_renew_reject_cancel`, `live_restart_live_reconciliation_cancels_under_persisted_handle`, and `live_restart_terminal_reconciliation_returns_result_without_resubmit` | pass | Persisted lease tuple, renew/cancel, reconciliation, and no second submit retained |
| Keep scheduler as sole dispatch/admission authority with no fallback | `live_scheduler_end_to_end_remote_only` | pass | The test registers only the Eggwork executor |
| No unresolved high- or medium-severity finding | Source/plan/closure review | pass | No open finding identified |

## 3. Production implementation evidence

- Production and fixture Eggwork dependencies are pinned to the immutable reviewed C001 commit. Lockfile changes are source-revision updates only.
- The Linux fixture creates a real Eggwork server with `TrustedLandlockSetup`, and builds the pinned workspace's sandbox helper alongside the node binary.
- The live suite validates capability/status equality, required-isolation execution, workspace-local read/write, denied outside reads/writes, and typed applied `workspace_rw` terminal evidence.
- Required isolation is also exercised through lease renewal/cancellation, restart reconciliation, and scheduler-owned remote-only execution.
- A scripted regression for forged or removed isolation capability proves pre-upload refusal. Disabled networking is refused before upload because the pinned node has no qualified network-isolation backend.
- No placement, target-selection, local fallback, or migration behavior was added.

## 4. Verification executed

### Commands run

```bash
cargo check --workspace --locked
cargo test --test eggwork_remote_execution --locked
cargo test --test eggwork_remote_execution_live --locked
scripts/verify.sh full
cargo test -p codegg --locked --features server,plugins,lsp-test-support -- --test-threads=1
```

### Results

- Local macOS `cargo check --workspace --locked`: passed against the corrected pin.
- Local macOS `cargo test --test eggwork_remote_execution --locked`: 25 passed. The focused `required_isolation_capability_removal_fails_before_upload` test also passed separately.
- Local macOS `cargo test --test eggwork_remote_execution_live --locked`: 0 tests, because this test target is Linux-gated; this is not runtime-isolation evidence.
- Local `scripts/verify.sh full`: Quick guards, formatting, workspace check, Clippy, and the complete base workspace test suite passed. The script's additional feature-enabled test build stopped at link time with `ld: write() failed, errno=28 (No space left on device)` while linking `tool_program_runtime`.
- After `cargo clean` removed 222.3 GiB of reproducible artifacts, the direct feature-enabled suite was restarted. It reported 6,147 passed and 1 ignored across 64 suites before being stopped at 459.61 seconds while a later suite was still running. No failure was reported, but the remaining suites are not claimed as locally verified.
- Hosted Linux `CI / verify`, run [36081751855](https://github.com/dbowm91/codegg/actions/runs/36081751855), completed successfully. Guards, formatting, workspace Clippy, and the full workspace test command passed. `eggwork_remote_execution.rs` passed 25/25; `eggwork_remote_execution_live.rs` passed 12/12 in 144.28 seconds, including all seven live tests for blob/materialization, lease fencing, required Landlock, restart reconciliation, unsupported policy refusal, and scheduler end-to-end execution.
- A local cross-target compile attempt could not run because this macOS host lacks `x86_64-linux-gnu-gcc`; hosted Linux CI supplies the required runtime evidence.

## 5. Invariant review

1. **Fixed target and no fallback:** named target selection remains upstream of the executor; the live scheduler test registers only Eggwork.
2. **Fresh policy evidence:** each attempt probes authenticated capability and status; cached posture remains diagnostic only.
3. **Fail-closed restricted policy:** required isolation needs matching feature evidence in both views before upload; unsupported network-disabled policy is refused.
4. **Applied isolation:** terminal `Applied { profile: "workspace_rw" }` evidence is asserted after the live job, in addition to physical escape-denial checks.
5. **Lease and restart fencing:** accepted execution identity and persisted lease behavior remain covered under required isolation; live reconciliation does not resubmit.
6. **Scheduler ownership:** the CodeGG scheduler remains the sole admission and lifecycle authority; no direct executor fallback was introduced.
7. **Secret and credential handling:** existing mTLS configuration remains daemon/config-owned; the live fixture uses temporary key material and the shared CI secret-scan tests pass.

## 6. Failure and recovery review

- Capability mismatch fails before workspace upload, avoiding a remote side effect for an unsatisfied isolation request.
- Unsupported disabled networking fails before upload; no mode is silently downgraded.
- Lease renew/cancel and both live and terminal restart reconciliation pass on the required-isolation path. A live accepted execution is reconciled under its persisted handle rather than submitted again.
- The local verification disk exhaustion affected only the optional feature-suite link step. The workspace was cleaned, the feature suite partially rerun without failures, and hosted Linux completed the full required verification and live suite.

## 7. Migration and compatibility review

No durable schema or wire-format migration was introduced. Existing node profiles retain the serde-defaulted unrestricted network policy; filesystem isolation is opt-in through the existing explicit required policy. Existing unsupported network-disabled requests remain fail-closed. The Eggwork revision is immutable and lockfile-pinned for reproducibility.

## 8. Security review

The live Linux test proves both sides of the filesystem boundary: intended workspace read/write is allowed and outside-workspace read/write is denied. CodeGG's executor requires fresh authenticated evidence and refuses a mismatched capability projection before upload. The terminal result is checked for applied isolation. No network restriction is claimed without a real backend. The node uses the trusted helper from the pinned Eggwork workspace; no fixture-only bypass is used.

## 9. Documentation and operations

- `architecture/config.md` documents fresh authenticated feature agreement and the disabled-network fail-closed behavior.
- `architecture/jobs.md` and `architecture/scheduler.md` record the qualified required-isolation path and unsupported network-disabled behavior.
- `docs/execution-ownership.md` and `README.md` describe the operator policy and execution boundary.
- Root and fixture manifest comments identify the corrected immutable upstream pin and live qualification fixture.

## 10. Unresolved findings (severity: critical/high/medium/low)

None identified. Local macOS feature-suite completion remains unverified after an environmental disk-space failure and an interrupted retry; hosted Linux CI passed the complete workspace suite and the required live qualification. This is recorded as a verification limitation, not an open product defect.

## 11. Roadmap disposition

- M002a is closed with the required hosted Linux evidence.
- M003 remains blocked on a separately reviewed optimized Eggwork materializer contract. This blocker is independent of M002a and has not been resolved by this implementation.
- M004 remains deferred. Closing M002a removes its M002a dependency, but M003's materializer work and the stable AgentRun worker-entry contract remain unresolved; it is not ready.
- No other registered future plan names M002a live requalification as its sole blocker. No additional plan is unblocked by this closure.

## 12. Registry updates

- Mark M002a implemented/closed and link this closure record from the plan, subsystem roadmap, and registry.
- Keep the Eggwork subsystem roadmap active because M003 remains future work.
- Keep M003 blocked and M004 deferred for the independent blockers listed above.
