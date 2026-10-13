# Command Authorization and Execution Boundaries Roadmap

Status: active (planning; ADR-0024 accepted; M001 ready for handoff)

Long-term references:
- `plans/000-long-term-specification.md` §§4,17,27–29.
- `plans/001-terminology-and-domain-model.md` §8.
- `plans/002-long-term-roadmap.md`.
- `plans/003-planning-process.md`.

ADRs: accepted ADR-0001, ADR-0002, ADR-0004, ADR-0016, ADR-0024; proposed ADR-0023 for Linux stronger enforcement.
Related: `plans/subsystems/linux-execution-isolation-roadmap.md`, `plans/subsystems/execution-reliability-approval-autonomy-roadmap.md` (historical closure not reopened).

## 1. Purpose and ownership boundary

Make command risk classification a non-authoritative approval signal and make authenticated execution capabilities and real process/OS restrictions the only source of security guarantees, consistently across Bash, Git, Python, native tools, command-intent routing and supported adapters. Own semantics/priorities of builtin patterns, optional user-configured pattern preferences, reviewer integration, policy receipts, route fidelity and observable diagnostics. Consume—do not rewrite—`ApprovalRouter`, `ExecutionPolicySnapshot`, `ManagedProcessService`, the scheduler, per-backend OS containment, typed Git domain, worktree scopes and audit transport.

## 2. Classification

Invariants: operator and parent/child Deny/ceiling never lifted by syntax, classifier miss or routing; approval never counts as OS isolation; required unavailable confinement fails before target; no trusted Git metadata code execution in elevated context; no unbounded secret-bearing logs.

Capabilities: legitimate shell/wrapper/script and Git requests reach the correct review/contained route rather than arbitrary prefix denials; users can explain approval vs kernel denial; operator-selected textual pattern preferences remain supported but are explicitly best-effort.

Infrastructure: consolidated authorization-vs-advisory disposition, typed reason/provenance, consistent approval precedence/route context and effect-equivalence test harness.

Polish: clearer operator explanations, obsolete blocklist guard cleanup and no duplicated pattern engines where owner already exists.

## 3. Non-goals

No permissive default host execution; no automatic FullHost/network/credential escalation; no universal automatic understanding of arbitrary scripts; no magic safe list for `cargo`, `git` or `python`; no removal of explicit user/admin Deny rules; no replacement of existing scheduler/service authority; no pretending `Landlock` enforces offline networking/IPC. Existing macOS/Windows scope remains separate and may be marked unavailable with exact reason.

## 4. Current state

At `main` 2026-10-13 and Linux planning branch `c7c1818`:
- `src/tool/bash/policy.rs` has builtin `HARD_DENY_PATTERNS`, literal `default_blocked_commands`, `ASK_PATTERNS`, prefix-based `check_shell_security`, and optional allowlist. Recent Bash changes rightly moved many wrapper/interpreter patterns from terminal rejection to `Ask`.
- `src/permission/mod.rs` handles explicit `bash_allow_patterns`, `bash_deny_patterns`, `bash_ask_patterns`, default safe-prefix allow patterns and destructive escalation. `src/security/policy.rs` can return `SecurityAction::Deny` based on substring and critical classification; `src/preflight/service.rs` supports `BlockOnDefinite`.
- `src/permission/approval.rs`, `crates/codegg-core/src/approval.rs` and `src/policy_surface.rs` already separate approval mode from requested and obtained sandbox enforcement.
- Linux branch `crates/egggit/src/process.rs` adds hard subcommand family rejects via `DENIED_GIT_EXECUTABLE`; M001 closure reports 9/12 normal mutation tests fail. No namespace/network/proc isolation exists yet.
- Risk: a literal deny may be bypassed by changing representation; a command named "safe" can invoke scripts, plugins, filters or network; user-configured text restrictions are expression preferences, not OS enforcement.

## 5. Target architecture

One immutable daemon-issued execution authority snapshot and typed tool call -> advisory risk/finding collector -> policy resolution/ApprovalRouter (explicit scoped Deny, operator-selected grants, risk-dependent Ask, reviewer choice) -> route planner (Bash/native/Python/Git) without authority changes -> `ManagedProcessService` or other authorized trusted executor with actually obtained capability receipt -> bounded provenance/observations/checkpoints and cancellation. Caller text does not create grants. For separate typed Git/remote tools, a capability-unavailable return must be first-class, never a fabricated process error.

Keep two transparent distinctions: (1) **explicit user-configured text filter** means "this invocation string is not desired", not "this effect is impossible"; (2) **hard scope denial** means authorized network/filesystem/credential/tool capability is absent and no alternate route can grant it.

## 6. Dependency graph

- M001 Syntax-only blocker audit and policy classification: **ready**; hard dependency ADR-0024 accepted; interface independent of Linux feature rollout.
- M002 Reviewer/preflight/permission/route convergence: **blocked hard M001**; `ApprovalRouter` and `ExecutionPolicySnapshot` existing interfaces are stable.
- M003 Cross-tool effect-equivalence/observability qualification: **blocked hard M002**; OS-level native Linux test availability is an **operational** evidence dependency, not a license to weaken launch.
- Linux C001 typed Git denial/routing uses M001's written policy interface (**interface** dependency); implementation may proceed in parallel and must not pretend this closes executable Git compatibility.
- Linux M002 composable contract depends on existing accepted ADR-0016 and ADR-0024, *not* M001 Git closure; proposed ADR-0023 gates concrete Bubblewrap backend promotion, not non-executable interface work.
- Linux C002 contained Git workflow recovery hard-depends on real M002–M005 obtained containment for executable local filters/hooks; networked Git requires distinct M006/approved transport authority. Never treat an interface test as an actual containment proof.

## 7. Milestones

| ID | Class | Handoff | Status | Exit |
|---|---|---|---|---|
| M001 | invariant | `implementation/command-authorization-and-execution-boundaries/001-syntax-blocker-audit-and-classification.md` | ready | Syntax-only builtins classified/advisory; explicit admin text filters preserved; FullHost warnings and safeguards truthful |
| M002 | infrastructure | `implementation/command-authorization-and-execution-boundaries/002-approval-preflight-and-routing-convergence.md` | blocked: M001 | one approval precedence and execution ceiling across shell/typed tools; denied capability cannot be laundered |
| M003 | invariant | `implementation/command-authorization-and-execution-boundaries/003-equivalent-effects-and-execution-observability.md` | blocked: M002 | equivalent forbidden effects cannot bypass actual enforcement by changing invocation surface; transparent unknown/unsupported |

## 8. Cross-cutting requirements

Storage/migration: preserve explicit `bash_{deny,allow,ask}_patterns` and stored decisions and signal their text-only semantics, no changing explicit Deny or grant. New DTOs additive/versioned only if needed. No undocumented default privilege expansion. Network and IPC are truly unrestricted in the current Landlock-only tier.

Security: differentiate typed authorization, opaque user code, prompt classification and native enforcement. No destructive command is guaranteed blocked in FullHost by a regex; explicit FullHost+YOLO requires truthful warning. Do not trust scripts by filename or automatic reviewer classification as a confinement boundary. Preserve secret redaction.

Concurrency/restart: policy snapshots/approval receipts immutable per attempt; no fallback into privileged executor after denied required enforcement; preserve cancellation, clean parent/child leases, scratch and bounded logs.

Observability: decision source, proposed invocation digest/argv, risk hints, chosen route, required-vs-obtained backend, actual executable identity, terminal result and limitation. Launch provenance is not proof of all downstream syscalls/effects.

Documentation: `architecture/permission.md`, `architecture/security.md`, `architecture/git.md`, `architecture/jobs.md`, `docs/cli.md` or exact active operator surface as appropriate; static guards revised to protect boundary rather than forbid individual spellings.

## 9. Verification strategy

Use paired same-effect shell/wrapper/script/Python/native binary/typed Git fixtures with synthetic secrets and marker files. Verify real filesystem read/write, outside-workspace denial, offline TCP/UDP, Git trusted metadata hijack, intentionally destructive allowed-workspace change, and explicit operator preferences. Test on exact native kernel where guarantees claimed; unsupported/partial host reports unavailable and no target marker for required mode. Verify approval matrix including protected child agent ceilings, Automatic and Yolo and no double approval on reroute. Run focused tests, static guards and `scripts/verify.sh quick` with closure matrix; avoid routine CI bloat.

## 10. Risks

- Removing heuristic hard blocks changes observable behavior, especially intentional FullHost: require conspicuous documentation and tests and do not silently discard operator rules.
- "Ask" patterns may create unnecessary prompts if context is lost or routing re-evaluates one logical action; retain one logical decision/receipt per attempt.
- Legitimately executable Git filters may need network/credentials. Defer until actual contained and separately authorized; do not restart with daemon authority.
- Text-based policy remains useful as workflow preference, *not* anti-bypass security. Tool disabling remains an enforceable typed denial.

## 11. Completion

No normative security claim relies on command-name/text patterns. Explicit real Deny and execution ceilings survive alternate tool routes; operator selected command-text controls remain functional and clearly labeled. Legitimate workflows are not unnecessarily made opaque; outcomes and obtained containment are truthful. Native adversarial and standard regression evidence recorded in all three closure records.

## 12. Status

M001 ready for handoff; M002 blocked on M001; M003 blocked on M002. Linux C001 typed Git corrective may proceed once M001 classification interface is stable. Linux enhanced backend is not yet implemented.
