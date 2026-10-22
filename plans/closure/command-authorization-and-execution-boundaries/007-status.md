# Command Authorization M004 — Closure Status

Status: **closed for route inventory and fail-closed model-route admission; no constrained specialized route is promoted**

Source implementation plan:

- `plans/implementation/command-authorization-and-execution-boundaries/007-m004-constrained-command-routing-and-tool-coverage.md`

Source subsystem roadmap:

- `plans/subsystems/command-authorization-and-execution-boundaries-roadmap.md`

Implementation commit:

- `37c4f2fb81d9b741a24bd84ceebb92527aeaef13` — shared accepted-receipt admission and typed unavailable disposition for unsupported constrained specialized routes.

This closure supersedes the conditional route-inventory disposition recorded in implementation commit `8db8dc181bceef4badadc6cad6cc6af2f4fe19c2`.

## 1. Executive finding

M004's model-facing command-route scope is complete. Active Bash command-intent, typed Test, and typed Python routes use one production admission predicate. It requires an immutable accepted `Allow` receipt, a non-empty decision ID, and matching receipt/snapshot policy revisions. The specialized route runs only when the accepted snapshot is explicit `FullHost`.

Constrained typed Test and Python calls return `ToolError::BackendUnavailable` before scheduler submission because their downstream executors do not consume the exact accepted sandbox snapshot. Missing or invalid authority returns a permission error. No route silently widens to FullHost, retries, or reroutes after Deny. For equivalent test commands, the user can select the already-authorized managed Bash route; Python does not advertise an equivalent constrained fallback. Raw constrained Bash continues to use its existing ManagedProcessService/Landlock path.

This is full closure of M004's routing and refusal criteria, not a claim that Test/Python have constrained route parity. Git filter/hook/remote execution remains unavailable under the independent Linux C002/C003 gates. M001–M003 closure evidence is unchanged.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Inventory eligible model and adjacent execution surfaces | `research/command-authorization-m004-route-coverage.md` | pass | Covers Bash, Test, Python, Git, native helpers, scheduler programs, plugins/hooks, LSP/MCP, human shell, and operator-invoked TUI `/test`, with owner and disposition. |
| Use one admission predicate based on accepted authority | `src/tool/authorization.rs::admit_full_host_specialized_route` | pass | Used by active Bash intent and structured Test/Python entrypoints; does not inspect argv, classifier labels, or tool names. |
| Reject missing, stale, mismatched, or denied authority | `specialized_admission_requires_matching_accepted_full_host_authority` | pass | Requires Allow, decision ID, equal policy revision; tests missing context, mismatch, Deny, constrained profile, and explicit FullHost. |
| Preserve constrained Test/Python rights or refuse before launch | `constrained_test_route_refuses_before_scheduler_submission`; `constrained_python_route_refuses_before_scheduler_submission` | pass | Both use `BackendUnavailable`; refusal occurs before scheduler service access/job submission. |
| Keep raw constrained Bash and command-intent routing distinct | `src/tool/bash/process.rs`; adversarial routing suite | pass | Raw Bash retains accepted-profile ManagedProcessService enforcement. Active intent routes require valid explicit FullHost authority. |
| Keep Git filters/hooks/remotes unavailable pending independent containment/transport | Route matrix and Linux C002/C003 gates | pass | No raw Git fallback or credential inheritance was added. |
| Cover child ceiling, helper failure, outside-root denial, and execution ownership | Focused child-ceiling test; `sandbox_containment`; ownership guard | pass | These prove their existing contracts; they are not claimed as Test/Python cross-route parity. |
| Update architecture, ownership, plan, registry, and route evidence | Architecture docs, tool-execution skill, ownership manifest, plan and closure | pass | M001–M003 records were not changed. |

## 3. Route disposition

- **Raw model Bash:** constrained launches retain the accepted snapshot and ManagedProcessService/Landlock enforcement. Network and Unix IPC remain unrestricted in the current Linux tier.
- **Active command-intent from Bash:** one shared gate checks the accepted receipt and permits only explicit FullHost. Missing/invalid authority is denied; constrained execution is typed unavailable. No implicit raw fallback runs.
- **Typed Test:** explicit FullHost with a valid receipt may submit to the existing scheduler/test runner. Constrained or missing-authority structured calls do not create a job. The refusal explains that an equivalent test command may use managed Bash.
- **Typed Python:** explicit FullHost with a valid receipt may submit to the existing Python scheduler path. Constrained or missing-authority structured calls do not create a job. No constrained reroute is offered because equivalent semantics and rights are not proven.
- **Git filters/hooks/remotes:** remain unavailable under their independent Linux containment and transport dependencies. Typed Git policy is not filesystem or network enforcement.
- **Operator TUI `/test`:** an explicit local operator request over authenticated `CoreRequest::JobSubmit`/`job.submit`, not a model ToolBroker call; it carries no agent `ExecutionAuthorizationContext`. It is separately inventoried and is not claimed as constrained model-route parity.
- **Tool Programs, plugins, configured hooks, LSP/MCP, and human shell:** retain their independent admission and execution owners. Invoking command text does not lend them authority.

## 4. Verification executed

All commands below passed locally against implementation commit `37c4f2f`:

```bash
cargo test --locked -p codegg --lib specialized_admission_requires_matching_accepted_full_host_authority
cargo test --locked -p codegg --lib constrained_test_route_refuses_before_scheduler_submission
cargo test --locked -p codegg --lib constrained_python_route_refuses_before_scheduler_submission
cargo test --locked --test command_routing_adversarial
cargo test -p codegg --lib permission --locked
cargo test --test approval_router --locked -- --test-threads=1
cargo test --test sandbox_containment --locked -- --test-threads=1
cargo test -p codegg --lib managed_process --locked
cargo test --locked --test reliability_qualification_m008 m008_child_effective_authority_never_exceeds_parent
python3 scripts/check_execution_ownership.py
python3 scripts/check_authorization_matrix.py
cargo fmt --all -- --check
scripts/verify.sh quick
```

Counts: admission predicate 1/1, Test refusal 1/1, Python refusal 1/1, command routing 142/142, permission-filtered library tests 124/124, approval router 17/17, sandbox containment 15/15, managed-process-filtered library tests 21/21, and child-ceiling test 1/1. Both manual guards and `verify.sh quick` passed; quick included formatting, repository boundaries/guards, and workspace all-target checking. Hosted CI was not run for this M004 change.

## 5. Security and compatibility disposition

- No permission ceiling, parent ApprovalRouter, FullHost default, scheduler owner, or process owner was relaxed.
- `BackendUnavailable` is terminal for retry and maps to a service-unavailable response; the broker does not silently retry or change routes.
- No shell/raw Git fallback, credential/environment inheritance, or accepted-Deny reroute was added.
- FullHost remains explicit host authority. Landlock evidence remains filesystem-only; this closure makes no network, IPC, proc, or seccomp isolation claim.
- Existing real-child sandbox tests establish helper setup failures, outside-root denial, and no-target behavior for their covered paths. New specialized-route tests establish pre-submission refusal; neither is misrepresented as cross-route filesystem parity.

## 6. Remaining independent work

These are explicit dependencies or separate surfaces, not unfinished M004 model-route criteria:

- Linux C002/C003 must close before Git filters, hooks, and remote execution can be admitted.
- Test/Python constrained parity requires future accepted-snapshot propagation and native route evidence; until then these calls remain unavailable under constrained profiles.
- Operator TUI `/test`, Tool Programs, plugins/hooks, LSP/MCP, and human shell retain their independently authorized owners and are not model-route parity evidence.
- Linux C006 remains a negative strict-M003 disposition. Its nested Git metadata and native compatibility findings do not promote any M004 route.

## 7. Roadmap and registry disposition

M004 is closed with a complete route inventory, a shared production admission predicate, and a fail-closed disposition for constrained specialized routes. No downstream plan was unblocked. Linux C002/C003 and all M001–M003 closure records remain unchanged.
