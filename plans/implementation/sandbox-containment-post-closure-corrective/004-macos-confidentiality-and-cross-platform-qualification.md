# Sandbox Containment Post-Closure Corrective C004 — macOS Confidentiality Boundaries and Cross-Platform Qualification

Status: blocked (hard dependencies C001 and C003)

Repository baseline: `main` `8e9d8b01e5c229715c8e4dea929e050b391e252c` (2026-10-10).

Source roadmap: `plans/subsystems/sandbox-containment-post-closure-corrective-roadmap.md#7-milestones-and-useroperator-exits`

Long-term authority: `plans/000-long-term-specification.md`, `001-terminology-and-domain-model.md`, `002-long-term-roadmap.md`, `003-planning-process.md`; ADR-0004.

Historical predecessors: runtime-safety M001/C001, `plans/closure/runtime-safety-resource-footprint/{001,009,010}-status.md`. New hard predecessors: C001 (macOS Python real containment + required launch), C003 (effective exclusions and scoped writes), with C002 completed before C003.

Primary class: invariant (accurate security claims) and capability (usable macOS containment).

## 1. Objective

Measure the real macOS Seatbelt read/IPC boundary with negative tests, harden it where compatible with common developer tooling, and establish truthful, externally verified Linux/macOS containment guarantees for the same Codegg profile. Avoid treating "backend installed" as "equal security" and avoid treating an untested or tool-breaking profile as improved containment.

## 2. Current architecture and evidence

`src/security/sandbox/seatbelt.rs::sbpl_profile` starts `(allow default)`, denies selected sensitive `file-read*`, then restricts filesystem writes. `SEATBELT_LIMITS` explicitly records broad reads, no network restrictions, no `no_new_privs`, pre-open descriptor access, and Apple Events/Launch Services channels. `src/bin/codegg-sandbox-helper.rs` uses `sandbox_init` and then `exec`, which avoids a dependency on the `sandbox-exec` shell wrapper for real enforcement but still probes using that executable's presence. `tests/sandbox_containment.rs` accepts either result for a read outside the workspace and thus does not certify read parity with Landlock.

From source review, basic Seatbelt setup appears coherent; there is no evidence that a fully equivalent broad filesystem *read* policy has been implemented or qualified. C001 and C003 correct correctness gaps before policy hardening. Do not assert that any specific macOS IPC denial successfully blocks every cross-process action until verified on native macOS.

## 3. Invariants

- Filesystem and authorization guarantees must describe actual enforcement, not nominal SBPL text or upstream behavior. A status report includes explicit, versioned backend capabilities and limitations.
- `WorkspaceWrite` and `ReadOnly` at minimum preserve C001-C003's filesystem mutation and credential-denial guarantees; a new read policy cannot loosen them.
- macOS denial of selected secrets must hold for actual reads, `open`, path aliases and child `exec`.
- Network, IPC and privilege boundaries are reported **separately**; denying a Seatbelt operation class does not justify saying "network isolated" or "all IPC blocked".
- Never switch to `FullHost`, use an uncontained fallback, or waive denied sensitive paths for compatibility.
- A test skip, mock profile rendering check, or host missing Seatbelt cannot establish real macOS containment.

## 4. Required approach / scope

**Read containment:** evaluate (i) the current allow-default/explicit-exclusion profile, (ii) narrower sensitive read classes, and (iii) a possible curated allowlist only if shell/Git/Cargo/Python/tooling compatibility can be demonstrated. Treat confidentiality as a separately reportable guarantee, not just another write rule. A strict read policy may be optional or unavailable on certain macOS configurations; enabling it must not silently change `WorkspaceWrite` semantics without approved authority/ADR where public semantics change.

**Cross-process escape surface:** inventory Seatbelt classes relevant to Apple Events, Launch Services, XPC/Mach communications, process signalling and inherited/open FDs. Test representative benign probes against private test processes or fixture services, not real user apps. Restrict only those channels that can actually be blocked without breaking the target toolchain. Explicitly classify channels that remain open.

**Network:** verify existing absence of OS network containment; do not attempt to infer isolation from Python AST gates, SSRF checks or a reported `network=Unrestricted` string. If a future network-denied profile is desired, register a separate ADR/workstream with DNS, Unix sockets, IPv4/IPv6, loopback, external egress and proxy tests; no such claim may be made here.

**Platform parity:** define a tabular **effective contract**: host, profile, read scope, persistent write scope, denied credentials, dangerous mutation rights, network, IPC, privilege transitions, FD inheritance. Distinguish strong common guarantees from backend-only guarantees and residual limits. Avoid homogenizing the backend registry's human-readable guarantees into falsely equivalent security capabilities.

**No-goals:** new entitlements/App Sandbox distribution, network policy framework, complete XPC governance, virtual machine/container, unsigned deprecated API replacement, arbitrary host denial profiles, new frontend features, extra CI lane.

## 5. Ordered work packages

A. Freeze C001-C003 accepted tests and capability descriptors; inventory `security::sandbox` APIs, `sandbox_init` FFI and current macOS kernel behavior on at least one native Apple Silicon host. Verify `sandbox-exec` probe and real apply availability differ gracefully.

B. Implement a *differential fixture* run on macOS and Linux: read workspace, read nonsecret outside file, read nested credential under allow-ancestor, read missing-then-created secret, write inside workspace, write outside, truncate outside, mutate shared cache, and open via symlink/hardlink. Assert protections required by C001-C003; record divergent read outcomes rather than accepting them as equivalence. Capture OS/kernel details, backend version/effective ABI and actual status frame.

C. Add bounded macOS-only IPC/process negative probes: attempted child setuid/privilege transition in safe fixture (without privileged installation), Apple Events/Launch Services/XPC/Mach reachability from a contained command where safe to test, and inherited FDs. Do not attempt intrusive operations against real sessions. A lack of test privileges is a documented unqualified scenario, not a passed escape prevention test.

D. Prototype narrower SBPL read/IPC rule sets in real helper fixtures. Benchmark representative `sh`, `git`, `cargo`, Rust, Python, package manager and `gh` read-only operations. Choose only a profile with measurable protection and acceptable compatibility; any regression requiring secret disclosure or broad user HOME access is a stop condition, not grounds for relaxing denies. Preserve baseline as an explicitly limited profile if no stronger supported option qualifies.

E. Refine actual reported guarantees and limits in `seatbelt::enforcement`, `landlock::enforcement`, `resolve_sandbox_enforcement`, Python projection and user docs so operator can distinguish write restriction, read restriction, secret denial, network, IPC, privilege and inherited FD handling. Do not invent a kernel-certified `network-denied` flag.

F. Extend existing `tests/sandbox_containment.rs` (or a small clearly bounded companion macOS target) and platform-neutral guards. Update `architecture/security.md`, `architecture/python_scripting.md`, `docs/cli.md`/`docs/tui.md` as applicable. Ensure older claims like "fully contained" are qualified.

## 6. Failure, cancellation, concurrency and migration

A Seatbelt profile that fails compilation/application never executes user code under a required policy. Probe/application races report setup failure. No long-lived helper. Concurrent sessions receive individually scoped writable roots; test that one session cannot alter another's fixture. Child cancellation and restart behavior remains supervised by `ManagedProcessService`. No DB migration; if changing the public meaning of existing profiles is required, seek ADR and explicit migration instead of silently strengthening or weakening an existing operator contract.

## 7. Required tests and verification

```bash
cargo fmt --all -- --check
cargo test -p codegg --lib security::sandbox::seatbelt
cargo test -p codegg --lib python_script::sandbox
cargo test -p codegg --test sandbox_containment -- --test-threads=1
cargo test -p codegg --test sandbox_policy_wiring
python3 scripts/check_sandbox_contract.py
python3 scripts/check_sandbox_policy_wiring.py
scripts/verify.sh quick
```

On one qualified Darwin host record exact macOS build/version, architecture, Seatbelt applied identity, 3 Python modes, all differential filesystem fixtures, representative IPC/FD behavior and tool compatibility. On one supported Linux Landlock host (ABI>=3) record rights and matching differential tests; missing native evidence means *conditionally closed*, not security-equivalent. Reuse existing hosted verify when possible; no permanent CI matrix or remote test farm.

## 8. Acceptance and stop conditions

Accept if C001-C003 guarantees continue to hold, macOS confidentiality/IPC probes identify and where practical close exploitable omissions, documented residual channels remain explicit, operator language distinguishes backend limitations, and native Linux and macOS evidence supports each *claimed* common protection. Strict read parity is **not** an unconditional success criterion; truthful narrower capabilities with documented reason and an actionable later decision are acceptable.

Stop if meaningful macOS protection requires an unsupported entitlement, breaks essential tools in a way not fixable without broad global disclosure, changes `SandboxProfile` authority semantics without ADR, or no native macOS host can run the essential tests. Do not promise equivalence or conceal failed/unsupported probes.

## 9. Closure

Create `plans/closure/sandbox-containment-post-closure-corrective/004-status.md` including commit/PR, C001-C003 predecessor closure references, Darwin/Linux host specs and actual results, mechanism comparison, tool compatibility, status truthfulness, limitations and unresolved findings by severity, all guards/quick and any hosted run, and final roadmap/registry disposition. Historical runtime-safety conditional closures remain separately truthful until independently qualified.
