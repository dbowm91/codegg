# Command Authorization and Execution Boundaries Roadmap

Status: closed (M001–M004 admission scope and corrective M004-C1 are closed)

Long-term references:
- `plans/000-long-term-specification.md` §§4,17,27–29.
- `plans/001-terminology-and-domain-model.md` §8.
- `plans/002-long-term-roadmap.md`.
- `plans/003-planning-process.md`.

ADRs: accepted ADR-0001, ADR-0002, ADR-0004, ADR-0016, ADR-0046, ADR-0045. Linux production enforcement remains gated by native qualification in the Linux roadmap.
Related: `plans/subsystems/linux-execution-isolation-roadmap.md`, `plans/subsystems/execution-reliability-approval-autonomy-roadmap.md` (historical closure not reopened).

## 1. Purpose and ownership boundary

Make command risk classification a non-authoritative approval signal and make authenticated execution capabilities and real process/OS restrictions the only source of security guarantees, consistently across Bash, Git, Python, native tools, command-intent routing and supported adapters. Own semantics/priorities of builtin patterns, optional user-configured pattern preferences, reviewer integration, policy receipts, route fidelity and observable diagnostics. Consume—do not rewrite—`ApprovalRouter`, `ExecutionPolicySnapshot`, `ManagedProcessService`, the scheduler, per-backend OS containment, typed Git domain, worktree scopes and audit transport.

## 2. Classification

Invariants: operator and parent/child Deny/ceiling never lifted by syntax, classifier miss or routing; approval never counts as OS isolation; required unavailable confinement fails before target; no trusted Git metadata code execution in elevated context; no unbounded secret-bearing logs.

Capabilities: ordinary in-scope developer operations (inspection, formatting, builds, tests, linters, scripts, ordinary workspace cleanup) run with minimal user prompting inside their already-authorized sandbox; new authority, explicit operator Ask/Deny, invalid requests and missing required enforcement remain subject to correct typed decisions. Legitimate shell/wrapper/script and Git requests reach the proper review/contained route rather than arbitrary prefix denials; operator-selected textual preferences remain best-effort.

Infrastructure: consolidated authorization-vs-advisory disposition, typed reason/provenance, consistent approval precedence/route context and effect-equivalence test harness.

Polish: clearer operator explanations, obsolete blocklist guard cleanup and no duplicated pattern engines where owner already exists.

## 3. Non-goals

No permissive default host execution; no automatic FullHost/network/credential escalation; no universal automatic understanding of arbitrary scripts; no magic safe list for `cargo`, `git` or `python`; no removal of explicit user/admin Deny rules; no replacement of existing scheduler/service authority; no pretending `Landlock` enforces offline networking/IPC. Existing macOS/Windows scope remains separate and may be marked unavailable with exact reason.

## 4. Current state

At `main` 2026-10-13 and Linux planning branch `c7c1818` (fresh-plan baseline):
- C002 implementation at `696e7a4d` makes built-in syntax classifications advisory, leaves the default blocked literal set empty, and avoids syntax-only Ask for ordinary wrappers/composition. Explicit configured text restrictions remain invocation-level policy.
- `src/permission/mod.rs` handles explicit `bash_allow_patterns`, `bash_deny_patterns`, `bash_ask_patterns`, default safe-prefix allow patterns and destructive escalation. `src/security/policy.rs` can return `SecurityAction::Deny` based on substring and critical classification; `src/preflight/service.rs` supports `BlockOnDefinite`.
- `src/permission/approval.rs`, `crates/codegg-core/src/approval.rs` and `src/policy_surface.rs` already separate approval mode from requested and obtained sandbox enforcement.
- Linux branch `crates/egggit/src/process.rs` adds hard subcommand family rejects via `DENIED_GIT_EXECUTABLE`; M001 closure reports 9/12 normal mutation tests fail. No namespace/network/proc isolation exists yet.
- Risk: a literal deny may be bypassed by changing representation; a command named "safe" can invoke scripts, plugins, filters or network; user-configured text restrictions are expression preferences, not OS enforcement.

## 5. Target architecture

One immutable daemon-issued execution authority snapshot and typed tool call -> advisory risk/finding collector -> policy resolution/ApprovalRouter (explicit scoped Deny, operator-selected grants, risk-dependent Ask, reviewer choice) -> route planner (Bash/native/Python/Git) without authority changes -> `ManagedProcessService` or other authorized trusted executor with actually obtained capability receipt -> bounded provenance/observations/checkpoints and cancellation. Caller text does not create grants. For separate typed Git/remote tools, a capability-unavailable return must be first-class, never a fabricated process error.

Keep two transparent distinctions: (1) **explicit user-configured text filter** means "this invocation string is not desired", not "this effect is impossible"; (2) **hard scope denial** means authorized network/filesystem/credential/tool capability is absent and no alternate route can grant it.

## 6. Dependency graph

### M004 constrained-route successor

`plans/implementation/command-authorization-and-execution-boundaries/007-m004-constrained-command-routing-and-tool-coverage.md` is closed at `plans/closure/command-authorization-and-execution-boundaries/007-status.md` for its route-inventory and fail-closed admission scope. Corrective 008 is closed at `plans/closure/command-authorization-and-execution-boundaries/008-status.md`: accepted constrained profiles reach Test/Python through the scheduler and canonical process owner. Linux containment and Git executable/remote gates remain independently owned; no Linux production backend promotion follows from this corrective.

C004 moved the accepted command authority to ADR-0045 with exact archived provenance. Its content and permissions remain unchanged; Windows ADR identities remain separately scoped.

### Completed M001–M003 dependency graph

### M001 internal handoff ordering — avoid a permissive-but-incorrect policy

- **M001 C001** `plans/implementation/command-authorization-and-execution-boundaries/004-m001-permission-precedence-and-scope-corrective.md`: **closed** at `plans/closure/command-authorization-and-execution-boundaries/004-status.md`. Specific configured rules beat fallbacks; current Deny is evaluated before saved Allow; complex shell and wrapper/interpreter scopes cannot reuse a first-token family grant.
- **M001 C002** `plans/implementation/command-authorization-and-execution-boundaries/005-m001-advisory-policy-and-preflight-corrective.md`: **closed** at `plans/closure/command-authorization-and-execution-boundaries/005-status.md`. Built-in syntax and risk preflight are advisory; explicit operator Deny, deterministic invalidity and human-shell behavior remain intact.
- **M001 C003** `plans/implementation/command-authorization-and-execution-boundaries/006-m001-developer-workflow-friction-qualification.md`: **closed** at `plans/closure/command-authorization-and-execution-boundaries/006-status.md`. The 62-command authorized-policy corpus has zero Ask/Deny outcomes; baseline classifier signals, Landlock ABI 4 filesystem evidence, one-request routing, and unrestricted network/IPC limitations are recorded.
- **Parent M001** `plans/implementation/command-authorization-and-execution-boundaries/001-syntax-blocker-audit-and-classification.md`: **closed** at `plans/closure/command-authorization-and-execution-boundaries/001-status.md`. Full blocker inventory reconciled; the 62-command corpus, explicit Deny/typed failure preservation, single approval route, and Linux Landlock shell/Python/native filesystem fixture are recorded. Network/IPC remain unrestricted/unqualified.
- This sub-slicing **does not** promote C001–C003 into new Linux execution milestones or re-open historical Linux Git C001 closure. M002 cross-tool routing depends on parent M001 closure, not on Linux backend promotion.

### Existing workstream dependencies

- M001 Syntax-only blocker audit and policy classification: **closed**; hard dependency ADR-0045 accepted; interface independent of Linux feature rollout.
- M002 Reviewer/preflight/permission/route convergence: **closed** at `plans/closure/command-authorization-and-execution-boundaries/002-status.md`; one accepted snapshot/receipt survives broker attempts, duplicate preflight is removed, and constrained route widening fails closed.
- M003 Cross-tool effect-equivalence/observability qualification: **closed** at `plans/closure/command-authorization-and-execution-boundaries/003-status.md`; Linux filesystem effects, Windows GNU compile, and GitHub Actions macOS cross-target compile passed. Network/IPC claims remain explicitly unavailable without an enforcing backend.
- Linux C001 typed Git denial/routing uses M001's written policy interface (**interface** dependency); implementation may proceed in parallel and must not pretend this closes executable Git compatibility.
- Linux M002 composable contract depends on existing accepted ADR-0016 and ADR-0045, *not* M001 Git closure; accepted ADR-0046 selects the concrete architecture, while native qualification gates Bubblewrap backend promotion, not non-executable interface work.
- Linux C002 contained Git workflow recovery hard-depends on real M002–M005 obtained containment for executable local filters/hooks; networked Git requires distinct M006/approved transport authority. Never treat an interface test as an actual containment proof.

## 7. Milestones

| M004 | capability/invariant | `implementation/command-authorization-and-execution-boundaries/007-m004-constrained-command-routing-and-tool-coverage.md` | **closed** | Closure `../closure/command-authorization-and-execution-boundaries/007-status.md`; accepted-receipt admission complete; constrained Test/Python fail before submission; Git gates retained |
| M004-C1 | capability/invariant | `implementation/command-authorization-and-execution-boundaries/008-m004-constrained-specialist-execution.md` | **closed** | Closure `../closure/command-authorization-and-execution-boundaries/008-status.md`; accepted profile reaches the canonical Test/Python process owner; constrained launch refuses when the obtained filesystem enforcement is absent or broader |



| ID | Class | Handoff | Status | Exit |
|---|---|---|---|---|
| M001 | invariant | `implementation/command-authorization-and-execution-boundaries/001-syntax-blocker-audit-and-classification.md` | **closed** | Permission matching and narrow grants correct, built-in syntax signals advisory, routine developer work low-friction; explicit rules and FullHost warnings truthful |
| M001 C001 | invariant | `implementation/command-authorization-and-execution-boundaries/004-m001-permission-precedence-and-scope-corrective.md` | **closed** | Specific configured rules not shadowed; current Deny remains terminal; saved family grants do not cover compound shell |
| M001 C002 | invariant | `implementation/command-authorization-and-execution-boundaries/005-m001-advisory-policy-and-preflight-corrective.md` | **closed** | No built-in syntax-only hard denial; risk classifier and command preflight advisory, explicit user policy intact |
| M001 C003 | capability | `implementation/command-authorization-and-execution-boundaries/006-m001-developer-workflow-friction-qualification.md` | **closed** | 62-command authorized-policy corpus has zero Ask/Deny; real Landlock ABI 4 filesystem evidence and network/IPC limitations recorded |
| M002 | infrastructure | `implementation/command-authorization-and-execution-boundaries/002-approval-preflight-and-routing-convergence.md` | **closed** | one approval precedence and execution ceiling across shell/typed tools; denied capability cannot be laundered |
| M003 | invariant | `implementation/command-authorization-and-execution-boundaries/003-equivalent-effects-and-execution-observability.md` | **closed** | Linux filesystem effect-equivalence, bounded audit outcomes, and Windows GNU and hosted macOS target compiles qualified; native macOS execution and network/IPC remain unavailable |

## 8. Cross-cutting requirements

Storage/migration: preserve explicit `bash_{deny,allow,ask}_patterns` and stored decisions and signal their text-only semantics, no changing explicit Deny or grant. New DTOs additive/versioned only if needed. No undocumented default privilege expansion. Network and IPC are truly unrestricted in the current Landlock-only tier.

Security: differentiate typed authorization, opaque user code, prompt classification and native enforcement. No destructive command is guaranteed blocked in FullHost by a regex; explicit FullHost+YOLO requires truthful warning. Do not trust scripts by filename or automatic reviewer classification as a confinement boundary. Preserve secret redaction.

Concurrency/restart: policy snapshots/approval receipts immutable per attempt; no fallback into privileged executor after denied required enforcement; preserve cancellation, clean parent/child leases, scratch and bounded logs.

Observability: decision source, proposed invocation digest/argv, risk hints, chosen route, required-vs-obtained backend, actual executable identity, terminal result and limitation. Launch provenance is not proof of all downstream syscalls/effects.

Documentation: `architecture/permission.md`, `architecture/security.md`, `architecture/git.md`, `architecture/jobs.md`, `docs/cli.md` or exact active operator surface as appropriate; static guards revised to protect boundary rather than forbid individual spellings.

## 9. Verification strategy

The acceptance corpus must include routine developer requests and realistic shell compositions, not only catastrophic regex triggers. Test `cargo fmt/check/test/clippy`, `uv run pytest`, `ruff`, `python -c`, `npm/pnpm/yarn`, `go test`, `make/just`, `git status/diff`, in-workspace `mkdir/rm -rf target`, `env VAR=...`, `bash -c`, basic pipelines and quoted substitutions. In a session already authorized for the scoped workspace and action, measure **zero unnecessary prompts caused solely by shell syntax**; account separately for deliberate `default=ask`, explicit configured Ask, legitimate destructive risk, missing Git contained backend, ungranted network/credentials or real capability violations. The goal is a usable coding assistant, not unconditional approval by executable-name prefix. Current Linux Landlock-only networking and Unix IPC remain unrestricted; no optional offline promise may appear until a real backend is qualified.



Use paired same-effect shell/wrapper/script/Python/native binary/typed Git fixtures with synthetic secrets and marker files. Verify real filesystem read/write, outside-workspace denial, offline TCP/UDP, Git trusted metadata hijack, intentionally destructive allowed-workspace change, and explicit operator preferences. Test on exact native kernel where guarantees claimed; unsupported/partial host reports unavailable and no target marker for required mode. Verify approval matrix including protected child agent ceilings, Automatic and Yolo and no double approval on reroute. Run focused tests, static guards and `scripts/verify.sh quick` with closure matrix; avoid routine CI bloat.

## 10. Risks

- Removing heuristic hard blocks changes observable behavior, especially intentional FullHost: require conspicuous documentation and tests and do not silently discard operator rules.
- "Ask" patterns may create unnecessary prompts if context is lost or routing re-evaluates one logical action; retain one logical decision/receipt per attempt.
- Legitimately executable Git filters may need network/credentials. Defer until actual contained and separately authorized; do not restart with daemon authority.
- Text-based policy remains useful as workflow preference, *not* anti-bypass security. Tool disabling remains an enforceable typed denial.

## 11. Completion

No normative security claim relies on command-name/text patterns. Explicit Deny and execution ceilings survive alternate tool routes; operator-selected command-text controls remain functional and labeled. Parent M001 closes only after its C001–C003 exact-revision closure evidence including the routine-command friction baseline and post-change counts. Legitimate coding workflows execute without syntax-induced obstruction under existing authority, while true extra authority remains separately granted. Native adversarial and standard regression evidence record obtained containment, not imagined network/IPC restriction.

## 12. Status

M001 is closed with C001–C003 evidence and aggregate closure at `plans/closure/command-authorization-and-execution-boundaries/001-status.md`; M002 is closed at `plans/closure/command-authorization-and-execution-boundaries/002-status.md`. M003 is closed at `plans/closure/command-authorization-and-execution-boundaries/003-status.md`: Linux filesystem effects, bounded audit outcomes, Windows GNU compilation, and the hosted macOS cross-target compile passed. Network/IPC remain unrestricted/unqualified; no native macOS runtime result is claimed. Linux Git C001 typed policy and Linux launcher M002 interface are already closed/conditionally closed on this branch. Linux full mount/process/network backend and Git C002/C003 remain separately gated; this command-policy planning pass makes no new containment claim. M004 plan 007 is closed for route inventory and fail-closed admission. Corrective 008 is closed at `plans/closure/command-authorization-and-execution-boundaries/008-status.md`: accepted constrained Test/Python profiles survive scheduling and reach the canonical process owner, with same-or-narrower filesystem effects and fail-closed backend handling. Git filters/hooks/remotes remain independently gated. No Linux production backend or downstream Git route is unblocked. Linux C004 is prerequisite metadata cleanup for unambiguous ADR references, not a new accepted security policy.
