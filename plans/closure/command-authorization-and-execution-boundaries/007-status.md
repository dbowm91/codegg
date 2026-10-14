# Command Authorization M004 — Closure Status

Status: **conditionally closed for route inventory and test-only admission design; production constrained specialized routes remain unqualified**

Source implementation plan:

- `plans/implementation/command-authorization-and-execution-boundaries/007-m004-constrained-command-routing-and-tool-coverage.md`

Source subsystem roadmap:

- `plans/subsystems/command-authorization-and-execution-boundaries-roadmap.md`

Repository baseline reviewed: `0281767` (after Linux C008 closure).

Implementation commit:

- `8db8dc181bceef4badadc6cad6cc6af2f4fe19c2` — route inventory, test-only admission oracle and ownership/documentation updates.

## 1. Executive finding

The bounded route inventory and test-only admission design are complete. No production route was promoted. The audit distinguishes process ownership, tool authorization, requested profile, and obtained filesystem effects. Active command-intent routes dispatched from Bash remain explicit-FullHost-only. The separate typed `test` and `python_script` tools do not yet carry the immutable accepted sandbox snapshot to route enforcement; Git filters/hooks/remotes remain gated by Linux containment and transport work. These routes are not qualified for constrained parity.

The test-only predicate in `tests/command_routing_adversarial.rs` is a design oracle, not production dispatch logic or a security guarantee. It rejects missing/stale authority, profile mismatch, disabled or status-only constrained evidence, broader routes, and Git filters while Linux C002 remains blocked.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Inventory Test, Python, Git, native, Bash, scheduler, plugin/hook, LSP/MCP, and human-shell surfaces | `research/command-authorization-m004-route-coverage.md` | pass | Records authorization, process/effect owner, cwd/environment/FD, sandbox/network, lifecycle/audit, fallback and route disposition. |
| Distinguish accepted authority from classifier labels and process ownership | Route matrix and test-only admission oracle | pass | Scheduler ownership, ManagedProcessService use, executable names and status text are not standalone authority or effect proof. |
| Reject missing/stale receipt, profile mismatch, disabled sandbox and status-only proof in the test-only model | Three `m004_test_only_route_gate_*` tests | pass | These tests do not alter or call production routing. |
| Preserve same-or-narrower rights for production constrained special routes | Source audit | unqualified | Typed `test` does not receive accepted profile in `TestJobExecutor`/`test_runner`; Python policy is mode/code-derived rather than the accepted snapshot. No route promotion was made. |
| Keep Git hooks/filters/remotes unavailable pending independent authority | Route matrix and existing Linux C002/C003 gates | pass | No typed Git or raw Git fallback added. |
| Verify real-child Landlock denial and required setup failure behavior | Existing `sandbox_containment` and `managed_process` suites | pass | Demonstrates helper/filesystem contract independently; does not establish cross-route parity. |
| Keep M001–M003 closure results unchanged | Planning/document diff review | pass | Their implementation and closure records were not edited. |

## 3. Route findings

- Raw constrained Bash is the current Landlock-backed command route. It carries the accepted context to `ManagedProcessService`, compares the accepted and registered profiles, and refuses required containment loss.
- Bash command-intent special routes currently require explicit FullHost when an accepted authorization context is present. The compatibility guard permits missing context for legacy/harness callers; that is not daemon admission evidence.
- Typed `test` is scheduled, but its executor enters `test_runner::runner::spawn_child`, which owns a domain-specific child launch and timeout/stall behavior without accepted-profile propagation.
- Typed Python uses `ManagedProcessService`, but the OS sandbox policy is independently derived from mode and script content. It is not proof of the exact accepted profile or workspace roots.
- Typed Git's operation policy and receipt do not contain hooks, filters, repository-controlled executable metadata, or remote transport. Linux C002/C003 gates remain in force.
- Tool Programs, plugins, configured hooks, LSP/MCP and human shell retain separate owners. An invoking command name does not lend them authority.

## 4. Verification executed

```bash
CARGO_TARGET_DIR=/tmp/codegg-c005-test-target cargo test --test command_routing_adversarial --locked
CARGO_TARGET_DIR=/tmp/codegg-c005-test-target cargo test -p codegg --lib permission --locked
CARGO_TARGET_DIR=/tmp/codegg-c005-test-target cargo test --test approval_router --locked -- --test-threads=1
CARGO_TARGET_DIR=/tmp/codegg-c005-test-target cargo test --test sandbox_containment --locked -- --test-threads=1
CARGO_TARGET_DIR=/tmp/codegg-c005-test-target cargo test -p codegg --lib managed_process --locked
python3 scripts/check_execution_ownership.py
python3 scripts/check_authorization_matrix.py
cargo fmt --all -- --check
scripts/verify.sh quick
```

All commands passed. Counts: command routing 142/142, permission-filtered lib suite 124/124, approval router 17/17, sandbox containment 15/15, managed-process-filtered lib suite 20/20. `verify.sh quick` passed, including workspace all-targets checking. Hosted CI was not run.

## 5. Security and compatibility disposition

- No parent approval behavior, permission ceiling, explicit Deny, FullHost default, scheduler, or production dispatcher changed.
- No route fallback, raw Git path, credential/environment inheritance, or reroute after Deny was added.
- Landlock evidence remains filesystem-only; network and Unix IPC stay unrestricted in the existing Linux tier.
- Existing real-child fixtures show outside-root denial and setup failure behavior for the helper path. `managed_process` tests show that disabled constrained launch and missing private helper status fail before a target marker. Those tests are not evidence that typed Test/Python/Git routes use the same boundary.
- This closure does not claim stale-receipt revocation, agent-child route equivalence, enhanced proc/network/IPC denial, or production route coverage for every native helper.

## 6. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| high | Typed Test does not consume the accepted sandbox profile at execution. | A constrained tool call has no demonstrated same-profile Landlock route. | Add immutable profile propagation and either ManagedProcessService containment with real-child parity or a typed pre-launch refusal. |
| high | Typed Python computes a separate sandbox policy from code mode without the accepted snapshot. | Parent ReadOnly/WorkspaceWrite ceilings and roots are not proven to match the effective child policy. | Propagate accepted context and bind policy to the exact snapshot, or refuse unsupported profile/route before submission. |
| medium | Production active Bash route guard accepts a missing context for compatibility callers. | Contextless adapters cannot establish daemon accepted authority from this guard. | Keep this path outside production claims; any caller promotion requires explicit authority provenance. |
| medium | Git executable metadata, filters/hooks and remote transports remain outside this route audit's containment. | Trusted Git operations may execute repository-controlled code or use broader transport authority. | Keep C002/C003 blocked until independent Linux filesystem/process/offline and transport proof closes. |

## 7. Roadmap and registry disposition

M004 is conditionally closed for its independently actionable route inventory and test-only admission design. Production constrained special-route promotion remains blocked on end-to-end accepted-context propagation and native positive/negative route evidence. This closure does not reopen M001–M003 or discharge Linux C002/C003.

Unblock audit: nothing newly unblocked. Linux C006 remains separately blocked on explicit ADR-0031 disposition and qualifying mount evidence; no downstream route became production-ready.
