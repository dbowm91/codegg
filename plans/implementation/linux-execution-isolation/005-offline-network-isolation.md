# Linux Execution Isolation M005 — Offline Network Isolation

Status: active — internal candidate path only; production qualification depends on M004 closure
Primary class: capability
Repository baseline: reconciled feature branch at `81e857326b3d53a9ab2b606a0e9db736f43bc429`, 2026-10-15.
Source: `plans/subsystems/linux-execution-isolation-roadmap.md`.
Long-term requirements: `plans/000-long-term-specification.md` §§4,17,27,29; `plans/001-terminology-and-domain-model.md` §8.
Applicable accepted decisions: ADR-0001, ADR-0002, ADR-0004, ADR-0016, ADR-0033.
Target closure record: `plans/closure/linux-execution-isolation/005-status.md`.

Current limitation: this plan is active for a private, opt-in Bubblewrap
network-namespace candidate and native test fixture. No persisted preference,
public request field, runtime default, or user-facing `OfflineRequired`
enforcement claim is enabled. M004's production-owner and host-qualification
gates remain hard dependencies for any production use.

## 1. Objective
Introduce operator-opted `OfflineRequired` for local Linux child execution, independent from `SandboxProfile` and `ApprovalMode`, with **measured** inability to communicate with external/host-local network services. Retain existing unrestricted networking as the legacy default. Network isolation must be installed before user code is executed, not inferred from static Python analysis or shell command classification.

## 2. Why this milestone and existing evidence
`codegg-core/src/approval.rs` explicitly reports `NetworkEnforcement::Unrestricted`; `src/python_script/sandbox.rs` only provides static capability inspection. Landlock filesystem ABI 3 does not isolate egress; Landlock ABI4 TCP port rules alone do not stop UDP, DNS or destination-specific traffic. M002 owns dimensional launch authority; M004 owns descriptor/Unix socket/PID and proc isolation. Reuse the real helper rather than adding an unowned Bash spawn path.

### Implementation progress — 2026-10-15

- `SandboxConfig::with_linux_offline_network(true)` selects a private internal
  candidate that composes M004 process/IPC isolation and compiles Bubblewrap's
  `--unshare-net` before target exec. It cannot be enabled by persisted
  sandbox profiles; required setup failures remain fatal.
- A deterministic compile test asserts the network namespace option is
  present alongside PID/IPC namespaces. An opt-in native test holds a host
  loopback TCP listener open and verifies the contained child cannot connect.
  Exact-head hosted workflow [38822273062](https://github.com/dbowm91/codegg/actions/runs/38822273062)
  passed the full M004 native target (8/8), including both the offline compile
  assertion and native loopback TCP denial probe, on its qualified ephemeral
  Ubuntu runner after user-namespace setup. Its protected-mount and trusted-Git
  regression targets also passed (11/11 and 18/18); the separate restricted-
  userns refusal job passed.
- This is not closure evidence for IPv6, UDP, DNS, inherited connected
  descriptors, all launch entry points, or restricted host configurations.
  Those remain required before any `NetworkEnforcement::Enforced` result or
  production preference can be exposed.

### Expanded hosted candidate probe — 2026-10-16

Exact-head hosted workflow [38829815162](https://github.com/dbowm91/codegg/actions/runs/38829815162)
on `b30ebdcd53f5ac3a411e3365fe7288ceff1a4acd` passed the expanded native test
inside the M004 suite. The contained Python child could not connect to host
IPv4/IPv6 loopback TCP listeners, deliver IPv4/IPv6 UDP datagrams to host
listeners, resolve `example.com`, connect to the reserved external address
`198.51.100.1:443`, or use a deliberately inherited host TCP descriptor.
The Python fixture was rewritten as a raw multiline string after review found
that Rust's continued-string whitespace elision had removed indentation from a
`try` block; the hosted result is for the corrected script.

This remains bounded candidate evidence. The DNS and reserved-address probes
did not include positive controls proving those same services were reachable
from the host, and do not satisfy the plan's full external-route/DNS-rebinding
qualification. No offline Cargo/toolchain workflow or production launch owner
was exercised. M004 production-owner, host-qualification and no-admission
gates remain; no persisted preference, default, or `NetworkEnforcement::Enforced`
claim is enabled.

### Offline toolchain candidate probe — 2026-10-16

The opt-in native test `offline_network_runs_cargo_metadata_and_check_without_network`
runs the actual CodeGG workspace through the required offline M005 namespace.
It performs `cargo metadata --offline --locked --no-deps` followed by
`cargo check --offline --locked --workspace`, with the runner's Rustup and
Cargo roots explicitly restored inside the child. The companion M004 test
also performs an offline workspace check and runs managed-process regressions.
This exercises workspace build scripts, cached third-party dependencies, Cargo,
rustc, and the existing process test suite while the network namespace is active.

Hosted M004/M005 run
[38869606745](https://github.com/dbowm91/codegg/actions/runs/38869606745)
on `7312acad74f4f6f10ee6b505a61fc9ce51f3f7ac` passed the complete
`M004 managed-process host PID and IPC denial probes` step with the full
workspace offline metadata/check probes. The separate restricted-userns
refusal-before-target job also passed. The same run's protected-mount and
trusted-Git regressions passed 11/11 and 18/18. Fresh full CI
[38869639397](https://github.com/dbowm91/codegg/actions/runs/38869639397)
passed 14,020 tests with 7 skipped; all ADR, sandbox, execution-ownership and
other workflow guards, formatting, Clippy, and Eggwork fixture prebuild passed.

The test target compiles locally, but the opt-in test cannot run on this
workstation because it rejects the Landlock ABI 6 signal/abstract-socket scope
request. That local host result is unsupported and is not counted as candidate
evidence; the qualified hosted runner supplies the native result. The first
full-workspace attempt, run `38867063304`, exposed an invalid `cargo metadata
--workspace` argument; its workspace `cargo check` test passed. Commit `7312acad`
removed that unsupported argument, and the corrected hosted process/IPC step
passed all 9 tests, including both offline workspace checks and the contained
managed-process regression suite. The exact-head native run also passed the
protected-mount and trusted-Git regressions.

This is internal candidate compatibility evidence only. It does not qualify
DNS rebinding, every production launch owner, supported-host admission, or a
persisted `OfflineRequired` route. M005 remains unenforced and M004's PTY/LSP
owner gates remain open.

### Resolver endpoint probe corrective — awaiting exact-head native qualification

The existing native fixture proves host DNS works and that the contained
child's `getaddrinfo` call fails, but it did not directly attempt to contact
the host's configured resolver. The fixture now records the configured
nameserver, proves it answers the same bounded DNS query from the host, then
sends that query from the private network namespace; receiving any child-side
reply is a test failure. This proves the isolated child cannot receive a
resolver response that could carry a rebinding answer. It does not simulate a
resolver that changes answers, and it does not close the remaining
production-owner or supported-host gates. The code change is pending exact-head
hosted M004/M005 verification before this paragraph is treated as qualification
evidence.

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
