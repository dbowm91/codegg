# Execution Reliability, Approval, and Autonomy M005 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/execution-reliability-approval-autonomy/005-production-sandbox-policy-wiring.md`

Source subsystem roadmap:

- `plans/subsystems/execution-reliability-approval-autonomy-roadmap.md#7-milestones`

Repository baseline reviewed: `c04b98ea`

Implementation commits or pull requests:

- `34ceffe5` — execution-reliability M005: production sandbox policy wiring

## 1. Executive finding

M005 is complete. The resolved `SandboxProfile` is now a production
execution property: `ToolRegistryOptions`/`SessionToolContext`/
`TurnRunInput` thread the daemon-resolved profile into `BashTool` via
`sandbox_config_for_profile()` over the authoritative workspace root.
`WorkspaceWrite`/`ReadOnly` build an enabled Landlock config on
supported Linux; `FullHost` intentionally carries no CodeGG filesystem
containment and is explicit/auditable. Obtained enforcement is reported
separately (`SandboxEnforcement`: filesystem
`Enforced/Unavailable/FullHost` + network `Unrestricted` for shell —
Landlock never implies network isolation). `ApprovalMode` cannot mutate
the profile. Child sandboxes narrow via `resolve_child_sandbox()` and a
worktree child's writable root is its leased worktree. Outside-path
needs return a bounded `SandboxEscalationRequest`, never a silent
turn-wide `FullHost` switch. Protocol exposes `SandboxEnforcementDto`
on the snapshot (additive).

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Policy/enforcement types WP-A | `codegg-core/src/approval.rs`: `FilesystemEnforcement::{Enforced,Unavailable,FullHost}`, `NetworkEnforcement::{Enforced,Unrestricted,Unavailable}`, `SandboxEnforcement{requested,filesystem,network}` + `for_full_host/for_constrained_enforced/for_constrained_unavailable/describe/is_enforced/is_full_host`, `SandboxProfile::{requires_filesystem_containment,is_full_host,legacy_sandbox_mode_name}`, `SandboxEscalationRequest`, `resolve_child_sandbox`; unit `sandbox_enforcement_matrix_is_truthful`, `child_sandbox_resolution_enforces_ceiling`, `sandbox_escalation_is_bounded_and_explicit` | pass | Requested vs obtained never overloaded on one boolean |
| Production threading WP-B | `src/tool/mod.rs`: `ToolRegistryOptions.sandbox_profile`, `ToolRegistry.sandbox_profile` + accessor, `with_options` resolves default `WorkspaceWrite` and applies `sandbox_config_for_profile` → `with_landlock_sandbox_custom`; `src/tool/factory.rs`: `SessionToolContext.sandbox_profile` passed through; `src/agent/turn_runtime.rs`: daemon-resolved `sandbox_profile` threaded into `SessionToolContext`; `src/tool/bash.rs`: `with_sandbox_profile/has_landlock_config/sandbox_enforcement`; `src/tool/backend.rs` + `src/agent/tool_batch.rs`: `ToolExecutionContext.sandbox_profile` from batch snapshot | pass | Normal daemon `TurnRunInput`/AgentLoopFactory path carries the resolved profile; `BashTool::default()` alone is no longer the production path |
| FullHost/fallback truthfulness WP-C | `src/security/sandbox.rs`: `sandbox_config_for_profile` returns `None` for `FullHost`, `sandbox_mode_for_profile(FullHost)=None`, `resolve_sandbox_enforcement` (FullHost→full-host; constrained+available→enforced landlock; constrained+unavailable→unavailable fail-closed), `escalation_for_outside_path`; `process.rs` `SandboxRequest::Required` still fails closed via `SandboxFailed`; `daemon_ops.rs` populates `sandbox_enforcement` on `ExecutionPolicyGet` | pass | Constrained failure cannot fall through to FullHost; unsupported hosts report `Unavailable` |
| Child/worktree matrix WP-D | `codegg-core` `resolve_child_sandbox`/`child_sandbox_allowed`; `src/tool/task.rs`: parent ceiling from `ToolExecutionContext.sandbox_profile` into `SubAgentRequest.parent_sandbox_profile`; `src/agent/worker.rs`: `SubAgentRequest.parent_sandbox_profile/sandbox_profile`, effective child via `resolve_child_sandbox` (read-only agents narrow to `ReadOnly`), child registry `sandbox_profile: Some(effective_child)`, `agent_loop.set_sandbox_profile(effective_child)`; scheduler/job-dispatcher/specialized fallbacks inherit default ceiling | pass | Worktree root is the child's writable sandbox root via its own `workspace_root` + narrowed profile |
| Network truthfulness | `NetworkEnforcement::Unrestricted` for all shell paths; `SandboxEnforcement::describe` and `SandboxEnforcementDto::summary` never render filesystem success as network containment; Python `os_network_isolation=false` unchanged and documented | pass | No network backend claimed |
| Protocol/frontends | `codegg-protocol/src/core.rs`: `FilesystemEnforcementDto`, `NetworkEnforcementDto`, `SandboxEnforcementDto::for_profile_on_host`, `ExecutionPolicySnapshotDto.sandbox_enforcement` additive (`#[serde(default)]`, legacy decode test); `daemon_ops.rs` `execution_snapshot_to_dto` populates enforcement from `SandboxConfig::is_available()` | pass | Frontends render `summary`/dimensions; stronger profile requires daemon-authorized update |
| DangerFullAccess cleanup | `SandboxMode::DangerFullAccess` retained as deprecated compat-only (docs, `is_deprecated_full_access`, `parse_compat`); `sandbox_profile_for_mode` vocabulary mapping; execution mapping is not 1:1 (`sandbox_mode_for_profile(FullHost)=None`); guard rejects new production construction | pass | No silent reinterpretation of old config |
| Docs/guards | `architecture/security.md` M005 section, `architecture/tool.md` options/registry/wiring, `architecture/permission.md` M005 item; `scripts/check_sandbox_policy_wiring.py`; `tests/sandbox_policy_wiring.rs` (20) | pass | Existing sandbox-contract guard preserved |

## 3. Production implementation evidence

Landed ownership and behavior:

```text
Core domain (codegg-core/src/approval.rs)
  SandboxProfile::{requires_filesystem_containment,is_full_host,legacy_sandbox_mode_name}
  FilesystemEnforcement::{Enforced{backend,abi},Unavailable{reason},FullHost}
  NetworkEnforcement::{Enforced{backend},Unrestricted,Unavailable{reason}}
  SandboxEnforcement{requested,filesystem,network}
    for_full_host / for_constrained_enforced / for_constrained_unavailable
    is_enforced / is_full_host / describe (never "network enforced" for shell)
  SandboxEscalationRequest{requested_path<=1024,profile,reason<=512}
  resolve_child_sandbox(parent, requested) -> Ok(narrowed) | CeilingExceeded

Sandbox primitive (src/security/sandbox.rs)
  SandboxMode::DangerFullAccess = deprecated compat-only
    is_deprecated_full_access / parse_compat / sandbox_profile_for_mode
    sandbox_mode_for_profile(FullHost) = None (no execution mapping)
  sandbox_config_for_profile(profile, workspace_root) -> Option<SandboxConfig>
    ReadOnly/WorkspaceWrite -> enabled config over exact roots
    FullHost -> None (explicit no containment)
  resolve_sandbox_enforcement(profile) host-aware
  escalation_for_outside_path(path, profile) bounded hint

Tool registry/runtime wiring (src/tool/mod.rs / factory.rs / turn_runtime.rs / bash.rs)
  ToolRegistryOptions.sandbox_profile: Option<SandboxProfile> (None = WorkspaceWrite)
  with_options resolves profile, applies sandbox_config_for_profile(root)
    FullHost -> no config + info log (auditable)
    constrained + root -> with_landlock_sandbox_custom
    constrained + no root -> warn + disabled (Unavailable, never FullHost)
  ToolRegistry.sandbox_profile stored + accessor
  SessionToolContext.sandbox_profile -> ToolRegistryOptions
  DefaultTurnRuntime passes TurnRunInput.sandbox_profile into SessionToolContext
  BashTool::with_sandbox_profile/has_landlock_config/sandbox_enforcement(requested)
  ToolExecutionContext.sandbox_profile from batch snapshot (parent ceiling)

Child ceiling (src/tool/task.rs / src/agent/worker.rs)
  TaskTool spawn reads ctx.sandbox_profile (default WorkspaceWrite) ->
    SubAgentRequest.parent_sandbox_profile
  worker resolves effective_child = resolve_child_sandbox(parent, requested|parent)
    read-only agents narrow to ReadOnly; FullHost under constrained parent fails closed
    child registry sandbox_profile = Some(effective_child)
    child loop set_sandbox_profile(effective_child)
  worktree children: writable root = own workspace_root (leased worktree for
    isolated mutating children, inherited root for shared read-only children)

Protocol (protocol/core.rs + daemon_ops.rs)
  FilesystemEnforcementDto / NetworkEnforcementDto / SandboxEnforcementDto
    for_profile_on_host(profile, host_supports_landlock, reason) + summary
  ExecutionPolicySnapshotDto.sandbox_enforcement: Option (additive default)
  daemon ExecutionPolicyGet populates enforcement via is_available/probe_landlock
```

Production ToolRegistry wiring trace:

```text
CoreDaemon TurnSubmit resolves persisted preference (daemon_turns.rs)
  -> TurnRunInput{approval_mode, sandbox_profile}
  -> DefaultTurnRuntime.run_turn
    -> build_session_tool_registry(..., SessionToolContext{sandbox_profile})
      -> ToolRegistry::with_options(ToolRegistryOptions{sandbox_profile, workspace_root})
        -> sandbox_config_for_profile(profile, workspace_root)
          -> BashTool.with_landlock_sandbox_custom (constrained)
          -> no config + audit log (FullHost)
    -> AgentLoopBuildInput{approval_mode, sandbox_profile}
      -> AgentLoop::{set_approval_mode,set_sandbox_profile}
        -> batch_snapshot -> ToolExecutionContext{sandbox_profile}
          -> TaskTool spawn -> SubAgentRequest.parent_sandbox_profile
            -> worker resolve_child_sandbox -> child registry/loop
```

SandboxProfile/enforcement matrix:

```text
ReadOnly + Linux/Landlock      -> config enabled ReadOnly; filesystem Enforced(landlock); network Unrestricted
WorkspaceWrite + Linux/Landlock -> config enabled WorkspaceWrite; filesystem Enforced(landlock); network Unrestricted
ReadOnly/WorkspaceWrite + no Landlock -> no enforced claim; filesystem Unavailable(reason); network Unrestricted; action fails closed
FullHost (any host)            -> no SandboxConfig; filesystem FullHost; network Unrestricted; explicit/auditable
ApprovalMode x Profile         -> orthogonal: {Interactive,Automatic,Yolo} never changes {ReadOnly,WorkspaceWrite,FullHost}
Child FullHost <= parent WorkspaceWrite -> CeilingExceeded (fail closed)
Child ReadOnly <= parent WorkspaceWrite -> Ok(ReadOnly)
```

## 4. Verification executed

### Commands run (local, this environment)

```bash
cargo test --test sandbox_policy_wiring
cargo test --test approval_router
cargo test --test permission
cargo test --test tool_execution
cargo test --test command_routing_execution_ownership
cargo test -p codegg-core --lib approval
cargo test -p codegg --lib -- m005
python3 scripts/check_sandbox_policy_wiring.py
python3 scripts/check_sandbox_contract.py
python3 scripts/check_approval_router.py
python3 scripts/check_execution_ownership.py
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
./scripts/verify.sh quick
```

Host: `Darwin nos-MacBook-Pro.local 25.6.0 ... x86_64` (no Landlock).
Supported-Linux Landlock fixture: not runnable on this host; recorded
honestly as unsupported-host (`Landlock is only available on Linux`);
constrained enforcement asserts `Unavailable` (fail-closed) rather than
`Enforced` here.

### Results

- `cargo test --test sandbox_policy_wiring`: 20/20 pass — profile→config mapping; FullHost distinct; filesystem/network separation; legacy compat; unsupported-host truthfulness; registry threading (WorkspaceWrite/FullHost); approval orthogonality; fail-closed unavailable; bounded escalation; network unrestricted; parent-child matrix; FullHost-under-WorkspaceWrite denial; distinct worktree roots; Yolo≠sandbox-disable; symlink/outside-path denial; restart re-resolution; DTO truthfulness/additivity.
- `cargo test --test approval_router`: 16/16 pass (M003 contract unregressed, including additive snapshot decode with new optional field).
- `cargo test --test permission`: 44/44 pass (deterministic ruleset/store intact).
- `cargo test --test tool_execution`: 55/55 pass (filesystem/shell tools intact through registry change).
- `cargo test --test command_routing_execution_ownership`: 21/21 pass (managed-process/scheduler ownership intact).
- `cargo test -p codegg-core --lib approval`: 16/16 pass (10 pre-existing + 3 M005 enforcement/ceiling/escalation + outcome/gate/secret-free).
- `cargo test -p codegg --lib -- m005`: 3/3 pass (profile→config, legacy compat, host truthfulness).
- `check_sandbox_policy_wiring.py`: pass (registry/factory/turn/worker/BashTool/core coverage; no new DangerFullAccess construction).
- `check_sandbox_contract.py`: pass (child-only/helper/status boundary intact).
- `check_approval_router.py`: pass (single router owner intact).
- `check_execution_ownership.py`: pass (no new spawn/scheduler surface).
- `cargo fmt --all -- --check`: pass.
- `cargo clippy --workspace --all-targets --locked -- -D warnings`: pass.
- `scripts/verify.sh quick`: pass (fmt, builtin-agents, core-boundary, sandbox, execution-ownership, tui-authority, workspace check).

Escape/fail-closed results: symlink escape denied; `/etc/passwd` outside workspace denied (cwd validation) while OS containment is separately enforced via Landlock on supported hosts; constrained Bash without a config reports `Unavailable` (never `FullHost`); helper/setup failure still surfaces typed `SandboxFailed` through `ManagedProcessService` (existing contract, guard-pinned).

Parent-child worktree matrix: `resolve_child_sandbox` unit + snapshot `narrow_for_child` integration + two-tempdir distinct-roots test; worker resolves effective child before any child loop/registry exists and fails closed on ceiling breach.

Automatic/Yolo non-mutation proof: `approval_mode_changes_do_not_alter_sandbox_profile` (all three modes keep `WorkspaceWrite`); `yolo_does_not_disable_sandbox` (Yolo routes Escalate→Allow while snapshot + Bash config stay constrained); `Automatic` placeholder still defers (M003, unregressed).

## 5. Invariant review

| Plan invariant (§4) | Evidence |
|---|---|
| Explicit constrained request fails closed if enforcement unavailable | `resolve_sandbox_enforcement` → `Unavailable`; `BashTool::sandbox_enforcement` without config → `Unavailable`; `SandboxRequest::Required` → `SandboxFailed`; tests `constrained_failure_cannot_fall_through_to_full_host`, `unsupported_host_reports_unavailable_never_full_host` |
| ApprovalMode cannot modify SandboxProfile, including Automatic/Yolo | Separate types/fields/store columns/protocol DTOs; `set_approval_mode` never touches sandbox; tests `approval_mode_changes_do_not_alter_sandbox_profile`, `yolo_does_not_disable_sandbox`; Automatic path unchanged (defer) |
| Filesystem result does not imply network sandbox | `NetworkEnforcement::Unrestricted` for all shell profiles; `describe`/`summary` wording; tests `filesystem_and_network_are_reported_separately`, `network_is_unrestricted_for_all_shell_profiles` |
| Parent/subagent ceiling cannot be broadened by child | `resolve_child_sandbox` + `narrow_for_child` + worker pre-construction check; tests `parent_child_ceiling_matrix`, `child_cannot_select_full_host_under_workspace_write_parent` |
| All process launches through canonical managed-process/scheduler paths | No new spawner added (`check_execution_ownership` pass); Bash still routes through `ManagedProcessService::run`; scheduler/Python paths untouched except truthful reporting |
| Workspace canonicalization/symlink protections intact | `validate_path_safety` unchanged; tests `symlink_path_escape_remains_denied`, `outside_workspace_path_is_denied_not_contained`; `tool_execution` 55/55 |
| FullHost explicit/auditable, never accidental fallback | `sandbox_config_for_profile(FullHost)=None` + info log; `sandbox_mode_for_profile(FullHost)=None`; enforcement `FullHost` only for explicit request; test `full_host_registry_leaves_bash_without_containment_explicitly` |
| Unsupported hosts report actual enforcement | `resolve_sandbox_enforcement` + DTO `for_profile_on_host`; Darwin run reports `Unavailable` with probe reason; test asserts never `FullHost`/`Enforced` on unsupported host |

## 6. Failure and recovery review

- Sandbox helper unavailable/setup error under a constrained profile fails the command with typed `SandboxFailed`/`Permission` diagnostics; no unsandboxed retry (existing `process.rs`/`managed_process.rs` contract, guard-pinned).
- Unsupported host (this Darwin run): constrained policy resolves to `filesystem=Unavailable(reason)` + `network=Unrestricted`; the action fails closed or escalates to the user — never claimed `Enforced`, never run as `FullHost`.
- Cancellation still terminates the managed process tree (`ManagedProcessService` ownership unchanged; no new background task added).
- Restart recomputes enforcement from current host + persisted requested preference (`persisted_workspace_write_reresolves_enforcement_after_restart` closes/reopens a file DB and re-resolves; stale `Enforced` is never persisted as fact).
- Concurrent turns/worktrees use immutable roots/snapshots: batch snapshot fixed per batch; two worktree children receive distinct `allowed_paths` from distinct `workspace_root`s.
- Outside-path need returns a bounded `SandboxEscalationRequest` hint; the turn stays constrained (no whole-turn `FullHost` switch).

## 7. Migration and compatibility review

- Old config with no `SandboxProfile` resolves to the M003 default `WorkspaceWrite` (`ToolRegistryOptions.sandbox_profile=None` → `WorkspaceWrite`); no surprising automatic `FullHost`.
- `SandboxConfig` remains the implementation primitive; existing builders (`with_landlock_sandbox*`, `with_sandbox_mode`) retained.
- Legacy `danger_full_access` strings parse via `SandboxMode::parse_compat` for readability but map to `FullHost` vocabulary with `None` execution mapping — documented semantic difference, no silent reinterpretation; guard blocks new production construction.
- No database migration (M003 `runtime_preferences` v58 reused; restart test proves additive reuse).
- Protocol additive: `ExecutionPolicySnapshotDto.sandbox_enforcement` is `Option` with `#[serde(default)]`; legacy payloads decode (`enforcement_dto_is_truthful_and_additive`, `protocol_snapshot_is_additive_for_older_clients`).
- `ToolExecutionContext.sandbox_profile` is additive `Option`; broker/test literals updated, legacy `None` means pre-M005 default.
- `SubAgentRequest` gains two additive `Option` ceilings; scheduler/job-dispatcher/specialized fallbacks pass `None` (default ceiling).

## 8. Security review

- Yolo/Automatic cannot disable containment: orthogonal types, snapshot-fixed profile, Bash config built before routing; proven by mode-matrix + Yolo tests.
- Child cannot broaden parent: pre-construction `resolve_child_sandbox` fail-closed; read-only agents narrow further; worktree root is a containment root, never an authority grant (network/tools/deny ceilings unchanged).
- `DangerFullAccess` deprecation is explicit: compat parse only, no execution constructor, guard-enforced.
- No secrets in new types: enforcement carries backend names/ABI/reasons + bounded paths (≤1024) and reasons (≤512), NUL-stripped; preference store still identifier-only.
- Authorization unchanged: new protocol field is a projection; `SandboxProfileSet` keeps its existing `Global` principal-scoped gate; frontends cannot choose stronger containment without daemon update.
- Audit truthfulness: FullHost logs explicit info; constrained-without-root warns; enforcement `describe`/`summary` strings distinguish filesystem vs network.

## 9. Documentation and operations

Updated:

- `architecture/security.md` — M005 production policy (threading, FullHost, enforcement separation, mode orthogonality, child narrowing, guard).
- `architecture/tool.md` — `ToolRegistryOptions.sandbox_profile`, registry wiring, child narrowing, guard + integration test pointers; `ToolRegistry` struct listing corrected.
- `architecture/permission.md` — M005 sandbox-wiring item.
- Guard: `scripts/check_sandbox_policy_wiring.py` (focused ownership lint; not a new CI lane).
- Protocol DTO doc comments in `codegg-protocol/src/core.rs`.

Operator notes: watch `sandbox profile FullHost: bash runs without CodeGG filesystem containment` (info, explicit) vs `sandbox profile requested without a workspace root; ... (reported as unavailable, never FullHost)` (warn) vs `sandbox helper failed: ...` (typed fail-closed) vs enforcement summaries (`filesystem enforced (landlock); network unrestricted (no OS isolation)`). Network `Unrestricted` on every shell profile is expected until a measured backend exists — do not report shell as network-sandboxed.

No new CI lane: the guard is a focused ownership lint run locally (plan §6 allowance); `verify.sh quick` lanes unchanged.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| Low | Supported-Linux Landlock escape/enforcement fixture not observed on this Darwin host | Policy wiring is landed with explicit operational qualification (unavailable-host path proven; enforced-host escape matrix awaits a Linux runner, same gap as Runtime Safety C002) | M008 qualification (or a Linux CI/runner pass) should run `sandbox_policy_wiring` WorkspaceWrite escape cases on supported Linux and record ABI/host evidence; no code change required for M005 |
| Low | `TerminalTool` remains cwd-bounded without Landlock wiring | Terminal surface is truthfully non-OS-contained; Bash is the canonical sandboxed shell | M007 surfaces should label terminal accordingly or wire the same profile→config mapping if the tool becomes a primary shell; no M005 scope change |
| Low | Scheduler Python/managed executors inherit workspace cwd but not per-turn Landlock roots | Durable job paths report truthfully via their existing backend outcomes; normal interactive shell (the M005 target) is wired | M008 persistence/restart matrix should assert scheduler-job enforcement reporting stays truthful; no new sandbox framework in M005 |
| — | No other open items | — | — |

No stop condition triggered (no new network/container backend invented, no managed-process ownership replaced, no approval semantics changed).

## 11. Roadmap disposition

Milestone closed with one downstream unblock:

- M005 (production sandbox policy wiring): hard dependency was the M003 execution-policy/ApprovalRouter contract. Contract consumed as designed. **Close.**
- M006 (automatic approval reviewer): hard dependencies were M003+M005. Both now closed. **Unblock to `ready`.**
- M007 remains **blocked** on M006 (M003+M004+M005 closed; was M005-M006).
- M008 remains **blocked** on M006-M007 (M001-M005 closed; was M005-M007).
- No corrective pass required; no deferred product work registered.

## 12. Registry updates

- `plans/registry.md`: M005 `active` → `closed` with closure link and implementation `34ceffe5`; subsystem row `M001+M002+M003+M004 closed; M005 ready` → `M001+M002+M003+M004+M005 closed; M006 ready`; dependency-ready table M005 row → `closed`, M006 row `blocked` → `ready` with M003+M005-contract note; execution-order item 2 rewritten (M005 closed, M006 ready, M007-M008 still blocked); M005 appended to recently-closed work; blocked-work M006 row removed, M007/M008 rows narrowed to remaining gates.
- `plans/subsystems/execution-reliability-approval-autonomy-roadmap.md`: M005 section `ready` → `closed` with closure link; M006 `blocked` → `ready`; M007 blocker `M005-M006` → `M006`; M008 blocker `M005-M007` → `M006-M007`; status table updated.
- `plans/implementation/execution-reliability-approval-autonomy/005-production-sandbox-policy-wiring.md`: `Status: active` → `Status: implemented`.
- `plans/implementation/execution-reliability-approval-autonomy/006-automatic-approval-reviewer.md`: `Status: blocked` → `Status: ready for handoff`.
