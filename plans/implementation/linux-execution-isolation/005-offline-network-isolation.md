# Linux Execution Isolation M005 — Offline Network Isolation

Status: blocked — hard: M002 and M004
Primary class: capability
Repository baseline: `main` @ `8535f6d7c2e68f5d134ba0c73bd146ca8bc0bd03`, 2026-10-13.
Source: `plans/subsystems/linux-execution-isolation-roadmap.md`.
Long-term requirements: `plans/000-long-term-specification.md` §§4,17,27,29; `plans/001-terminology-and-domain-model.md` §8.
Applicable accepted decisions: ADR-0001, ADR-0002, ADR-0004, ADR-0016, ADR-0027, and ADR-0028.
No production implementation or verification is asserted by this plan.
Target closure record: `plans/closure/linux-execution-isolation/005-status.md`.

## 1. Objective
Introduce operator-opted `OfflineRequired` for local Linux child execution, independent from `SandboxProfile` and `ApprovalMode`, with **measured** inability to communicate with external/host-local network services. Retain existing unrestricted networking as the legacy default. Network isolation must be installed before user code is executed, not inferred from static Python analysis or shell command classification.

## 2. Why this milestone and existing evidence
`codegg-core/src/approval.rs` explicitly reports `NetworkEnforcement::Unrestricted`; `src/python_script/sandbox.rs` only provides static capability inspection. Landlock filesystem ABI 3 does not isolate egress; Landlock ABI4 TCP port rules alone do not stop UDP, DNS or destination-specific traffic. M002 owns dimensional launch authority; M004 owns descriptor/Unix socket/PID and proc isolation. Reuse the real helper rather than adding an unowned Bash spawn path.

## 3. Non-regression and security invariants
Offline is operator-only policy: the agent, nested child, supplied env, command text and provider connection cannot widen it. A required offline operation never falls back to legacy host networking or FullHost. Deny host TCP/UDP/IPv4/IPv6, localhost services, default DNS, raw and abstract UDS bridges, forwarded/connected network FDs and any exposed daemon/SSH/Docker socket. Own-namespace loopback can exist only if unable to reach host processes and is reported precisely. No privileged helper or global firewall mutation.

## 4. Scope, required production changes and ownership
Add immutable `NetworkIntent::{LegacyUnrestricted,OfflineRequired}` to the authenticated launch requirement; retain existing profiles separately. Build a private Linux network namespace with deliberate loopback/route and DNS behavior in the trusted adapter, sanitize inherited sockets and network-bearing env/FDs, and compose it with M004's IPC restrictions. Bind status to the *actually applied* namespace and report exact `NetworkEnforcement`; absence or failure is fatal for required mode. Add user-visible precise denial reasons and a separate explicit network exception workflow rather than a silent retry. Do not change the default network setting or add a proxy in this milestone.

Explicit non-goals: altering macOS/Windows execution defaults; creating a second independent process executor; changing worktree/session authorization; agent-selected escalation to FullHost; undocumented broadening of permissions; marking a requested but unproven enforcement result as applied.

Core/domain: retain typed policy snapshot, lease and immutable parent/child ceilings. Storage/migrations: no initial DB schema change; if durable preferences or public protocol/DTO fields change, design a versioned migration and compatibility test before shipping. Runtime/concurrency: execution belongs to `ManagedProcessService`, parent and helper must bind the status to the specific child before user exec. Frontend/operator surface: show requested and obtained dimensions and a safe explicit denial/next action. Security: untrusted workspace and child inputs never select helper, override enforcement, or control trusted broker metadata. Documentation/guards: update `architecture/security.md`, relevant jobs/tool docs, tests and static guards.

## 5. Ordered work packages and evidence
A. Add negative fixtures for no-userns/no-netns, untrusted env, injected FD and forged setup receipt; prove no target marker after required setup rejection.
B. Wire authenticated network intent through scheduler, `ManagedProcessService` and trusted helper; enforce no child widening.
C. Construct namespace/route/loopback/Unix-socket boundary; verify actual child socket behavior on a supported kernel.
D. Demonstrate `cargo metadata --offline`, warm `cargo check`, Git status/diff, rustc, Python and shell subprocesses work without a network.
E. Add focused real-child native tests and operator documentation; record supported kernel/features in closure.

## 6. Failure, cancellation, restart and contention
On namespace setup or feature loss, fail before exec and release parent/helper/spec resources. Cancellation/timeout terminates namespace descendants, no live outbound socket or orphan retained. Two simultaneous jobs cannot use each other's namespace or broker socket. Restart never treats a partial launch as enforced. Legacy default remains as-is; any persisted opt-in/network DTO must be versioned with a reviewed migration.

## 7. Compatibility and migration
Default `ReadOnly`/`WorkspaceWrite`/`FullHost` vocabulary, existing Landlock ABI>=3 baseline and legacy unrestricted shell network stay truthful and usable until explicit, reviewed adoption. No migration is implied by writing this plan. Platform unsupported or optional skipped test is not a proof of security. A new public protocol or persisted preference must be versioned; older consumers cannot silently treat unsupported required constraints as met.

## 8. Focused, integration, negative and qualification tests
Native: TCP IPv4/IPv6 to local host listener; UDP fixture; external-route attempt in isolated test network; DNS and DNS rebinding reachability; pathname/abstract AF_UNIX; inherited connected socket; `/proc/net`; no-userns rejection; allowed private IPC and offline cargo/rustc/git. Test bounded cancellation and concurrency with distinct attempts. Include an explicit legacy profile test verifying `Unrestricted`. Commands: `cargo fmt --all -- --check`; `cargo test -p codegg --lib security::sandbox`; `cargo test -p codegg --test sandbox_containment -- --test-threads=1`; `python3 scripts/check_sandbox_contract.py`; `python3 scripts/check_sandbox_policy_wiring.py`; `scripts/verify.sh quick`. Add and name a focused Linux-only offline test target and record exact invocation/results in closure.

## 9. Acceptance criteria and closure evidence
No child can access a host-local fixture or external network via TCP/UDP/IPv4/IPv6, DNS, Unix sockets or inherited FDs under `OfflineRequired`. Offline local builds work. Unsupported host denies before target; current Landlock-only mode remains unaffected. The reported network isolation is actual kernel enforcement, independent of filesystem status.

Record implementation commits, host capability/kernel version, test output, skips/blocks, requested/obtained status, requirement-to-evidence table, compatibility, rollback, unresolved findings with severity and a justified closed/conditionally closed/blocked recommendation in `plans/closure/linux-execution-isolation/005-status.md`. Do not claim verification merely because fixtures exist.

## 10. Stop conditions
Stop if a retained descriptor or socket bridges to host, network namespace failure silently retries without containment, or satisfying requirements requires an implicit default-mode change. Do not claim offline from AST analysis or host firewall alone.
