# Linux Execution Isolation M008 — Native Qualification, Compatibility and Rollout

Status: blocked — hard: M001–M007 for full close; approved partial tier may be reported separately
Primary class: invariant
Repository baseline: `main` @ `8535f6d7c2e68f5d134ba0c73bd146ca8bc0bd03`, 2026-10-13.
Source: `plans/subsystems/linux-execution-isolation-roadmap.md`.
Long-term requirements: `plans/000-long-term-specification.md` §§4,17,27,29; `plans/001-terminology-and-domain-model.md` §8.
Applicable accepted decisions: ADR-0001, ADR-0002, ADR-0004, ADR-0016. ADR-0022 remains proposed, not accepted.
No production implementation or verification is asserted by this plan.
Target closure record: `plans/closure/linux-execution-isolation/008-status.md`.

## 1. Objective
Provide an evidence-based Linux security qualification and explicit adoption/rollback decision. Validate *actual* filesystem, Git, process/IPC, offline/proxy network, and resource limits against hostile workloads and normal Rust/Python development, including negative hosts. Do not infer whole-agent sandbox protection when only Bash or particular native executors use the new launcher.

## 2. Why this milestone and existing evidence
`plans/closure/sandbox-containment-post-closure-corrective/002-status.md` and `003-status.md` establish Landlock ABI4, no_new_privs, secret-deny and scratch tests on a supported Linux 6.8 host, but not namespaces/network/cgroups. M001–M007 create new separate security boundaries and must each provide native closure evidence. Existing macOS Seatbelt C004 and Windows native AppContainer work are separate, not automatic parity evidence.

## 3. Non-regression and security invariants
A claimed required isolation guarantee must have both permitted positive behavior and blocked hostile negative behavior tested on the *same exact revision*. Absent feature or failed helper never runs uncontained target. No cross-session IPC/FD/process, unauthorized network, credential/Git metadata access, or quota bypass. Preserve daemon/worktree/parent-agent authorization, approval decisions and explicit FullHost. The established Landlock-only default/network meaning remains unchanged until a maintainer approves explicit migration.

## 4. Scope, required production changes and ownership
Produce a compact feature/evidence matrix by kernel, distro, architecture, namespace/userns policy, effective Landlock ABI, trusted bubblewrap helper/version, seccomp, cgroup delegation, selected mode, actual requested/obtained dimensions and exact Git trusted/untrusted execution boundary. Audit agent-loop through `ToolRegistry`, scheduler, `ManagedProcessService`, Python, Bash, Git, test runner, LSP, MCP/plugin subprocesses, Tool Programs, human shell and remote execution; declare each inside, outside or separately trusted. Do not paint a session as contained if out-of-sandbox tool services remain privileged. Add existing-code-path instrumentation and negative guards rather than a parallel execution framework.

Measure functionality/performance on real `git status/diff/safe commit`, worktrees, `cargo metadata --offline`, `cargo check/test`, rustc/rustup, Python Analyze/Verify/Transform and standard shell/compiler/LSP/test operations. Capture cold/warm launch overhead, memory and scope compatibility; avoid granting broad `/proc`, `/run`, HOME or network to fix a smoke test. Produce opt-in rollout/rollback guidance with versioned preference/API only when explicitly reviewed. Preserve historical closure records.

Explicit non-goals: altering macOS/Windows execution defaults; creating a second independent process executor; changing worktree/session authorization; agent-selected escalation to FullHost; undocumented broadening of permissions; marking a requested but unproven enforcement result as applied.

Core/domain: retain typed policy snapshot, lease and immutable parent/child ceilings. Storage/migrations: no initial DB schema change; if durable preferences or public protocol/DTO fields change, design a versioned migration and compatibility test before shipping. Runtime/concurrency: execution belongs to `ManagedProcessService`, parent and helper must bind the status to the specific child before user exec. Frontend/operator surface: show requested and obtained dimensions and a safe explicit denial/next action. Security: untrusted workspace and child inputs never select helper, override enforcement, or control trusted broker metadata. Documentation/guards: update `architecture/security.md`, relevant jobs/tool docs, tests and static guards.

## 5. Ordered work packages and evidence
A. Audit M001–M007 closures, native tests and missing guarantees; unresolved security invariants become corrective/blockers, not documentation-only closure.
B. Run exact-revision Linux adversarial matrix: Git planted hooks/filters/fsmonitor, alias links/mount races, future secrets, host proc/ptrace/daemon sockets, TCP/UDP/IPv6/DNS/abstract UDS, proxy bypass/rebinding, cgroup process/memory/cpu, nested/sibling jobs.
C. Run no-unprivileged-userns, unsupported seccomp, no-delegation, missing trusted helper, forged/duplicate status, denied mounts, partial cancel and restart: required target must produce **no marker**.
D. Run real developer-tool compatibility and resource/performance profile, multiple concurrent sessions/worktrees; do not tolerate silent host privilege shortcuts.
E. Write requirement-to-evidence and residual risk/CI/operational matrix; obtain independent code security review; decide opt-in availability. Explicit maintainer approval before any default/migration change.
F. Update `plans/registry.md`, roadmap, closure `plans/closure/linux-execution-isolation/008-status.md` with real outputs and exact commit.

## 6. Failure, cancellation, restart and contention
Refusal and orphan cleanup on helper/cgroup/proxy/mount failures; verify no mount, proxy endpoint, scratch, cgroup or process remains after timeout/cancel/crash. Two sessions must not share host process or network rights. Rollback of a required hardened attempt must not silently downgrade to legacy; old clients unable to represent enhanced required policy deny or retain legacy explicitly. Closure may say "conditional/blocked" when a claimed dimension lacks native evidence.

## 7. Compatibility and migration
Default `ReadOnly`/`WorkspaceWrite`/`FullHost` vocabulary, existing Landlock ABI>=3 baseline and legacy unrestricted shell network stay truthful and usable until explicit, reviewed adoption. No migration is implied by writing this plan. Platform unsupported or optional skipped test is not a proof of security. A new public protocol or persisted preference must be versioned; older consumers cannot silently treat unsupported required constraints as met.

## 8. Focused, integration, negative and qualification tests
Native Linux: feature-positive kernel plus no-userns negative fixture; kernel/LSM/arch/feature report. Assert seven boundaries: trusted Git no marker; protected read/write/nested secrets; host process and IPC deny; offline TCP/UDP/IPv6/UDS deny; mediated allowed HTTPS and denied SSRF/tunneling; required cgroup budgets; no-target on setup failure. Positive normal cargo/git/rustc/Python/TUI-adjacent command flows. Existing commands: `cargo fmt --all -- --check`; `cargo test -p codegg --lib security::sandbox`; `cargo test -p codegg --lib managed_process`; `cargo test -p codegg --test sandbox_containment -- --test-threads=1`; `cargo test -p codegg --test sandbox_landlock -- --test-threads=1`; `cargo test -p codegg --test managed_process_descendants -- --test-threads=1`; `python3 scripts/check_sandbox_contract.py`; `python3 scripts/check_sandbox_policy_wiring.py`; `scripts/verify.sh quick`. Run all newly added M001–M007 tests too and record each exact invocation and result.

## 9. Acceptance criteria and closure evidence
Documented coverage and native negative **plus** positive evidence exist for every reported enforced dimension, with exact SHA and host features. High/critical and security-invariant medium failures block release. Opt-in hardened mode has correct denial/operator surface, escape-free cross-session cleanup and clear rollback. Main legacy profile is unaffected. The roadmap closes only after every milestone closure; an M005-only offline partial tier cannot stand in for M006 brokered egress or M007 required cgroups.

Record implementation commits, host capability/kernel version, test output, skips/blocks, requested/obtained status, requirement-to-evidence table, compatibility, rollback, unresolved findings with severity and a justified closed/conditionally closed/blocked recommendation in `plans/closure/linux-execution-isolation/008-status.md`. Do not claim verification merely because fixtures exist.

## 10. Stop conditions
Stop for unqualified security promise, successful privileged Git payload, reachable host daemon/IPC/network bypass, uninstalled required cgroup, or compatibility fix requiring broad privilege expansion. Propose a bounded corrective instead of weakening acceptance criteria.
