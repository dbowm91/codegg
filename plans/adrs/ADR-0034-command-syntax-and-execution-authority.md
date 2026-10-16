# ADR-0034: Command Syntax Is Advisory; Execution Authority Is Enforced

Status: accepted

Date: 2026-10-13

Decision owners: project maintainers

Related long-term authority:
- `plans/000-long-term-specification.md` §§4, 17, 27, 28 and 29.
- `plans/001-terminology-and-domain-model.md` §8.
- `plans/003-planning-process.md`.

Affected subsystem roadmaps:
- `plans/subsystems/command-authorization-and-execution-boundaries-roadmap.md`
- `plans/subsystems/linux-execution-isolation-roadmap.md`
- `plans/subsystems/execution-reliability-approval-autonomy-roadmap.md` (cross-reference; do not reopen historical closure).

Related ADRs: ADR-0001, ADR-0002, ADR-0004, ADR-0016, accepted ADR-0035 (Linux composable isolation), and accepted ADR-0033 (directory-tied project identity). This ADR **refines** ADR-0004's separation of approval from obtained sandbox enforcement; it does not supersede its history or weaken immutable authority ceilings.

## Context

Codegg's Bash command policy recently moved many shell spellings (`bash -c`, `sudo`, shell substitution, pipe-to-shell) from hard denials to advisory `Ask` signals, recognizing that equivalent scripts or interpreter invocations bypass syntax matches. Yet `src/tool/bash/policy.rs` retains a hard-deny regex tier, literal prefix blocklist and optional command allowlist; `src/permission/mod.rs` contains configurable `bash_deny_patterns`; `src/security/policy.rs` can deny classified critical command text; and `src/preflight/service.rs` can block deterministic findings. The Linux isolation M001 Git corrective introduced `crates/egggit/src/process.rs::trusted_git_request_denied`, denying entire command families by name and substituting a fabricated nonexistent executable. The M001 closure `plans/closure/linux-execution-isolation/001-status.md` reports 9/12 mutation integration tests failing as a consequence.

A syntactic invocation restriction is easy to express around by writing a script, running a different interpreter, or dispatching the same effect through a different tool. This can make observed requests more opaque without changing real filesystem or network authority. Conversely, approving an innocuous-looking command cannot prove that an executable will have no effects. Git config, scripts, plugins, build systems and subprocesses can dynamically execute arbitrary code. Codegg must not pretend a regex or static model classifier is an operating-system guarantee.

## Decision drivers

- Preserve operator control while avoiding syntax-only denial loops, workaround pressure and false security claims.
- Maintain the daemon-owned approval/router, immutable per-project/workspace/agent/session ceilings, explicit FullHost and contained/obtained enforcement semantics.
- Keep genuinely enforceable denials: disabled tools, typed unauthorized actions, denied paths/mounts, network/credential capabilities, OS sandbox failures, worktree/lease scopes, parent/child ceilings, invalid inputs and operator-revoked permissions.
- Make advisory detection, approval, kernel enforcement and audit independent and uniformly applied to Bash, typed Git, Python, native commands, scripts and future executor routes.
- Preserve explicit user/admin preferences and the difference between a UX-level command-text filter and a security guarantee, with transparent compatibility migration.
- Avoid treating automatic approval reviewers as security boundaries or treating missing risk recognition as authorization to exceed authority.
- Preserve real security controls when `FullHost` is deliberately selected: present explicit risk/approval truth rather than manufacture a regex safety guarantee.

## Considered options

### A. Expand hard regexes and subcommand denylists

Rejected as a security architecture. Coverage cannot be complete against wrappers, scripts, process substitution, compiled code, Git filters or alternate tools; false positives block benign work and motivate less observable executions.

### B. Remove all validation and allow arbitrary execution

Rejected. Approval, authentication, immutable authority, path/network/process/credential limits, typed contracts, shell tool enable/disable switches, and explicit operator policy remain necessary. Uncontained execution must not silently result from classifier uncertainty.

### C. Treat command syntax as advisory; enforce acquired authority at the executor (selected)

Classify intended action/risks to guide approval and record decisions, then run within a kernel/OS or otherwise verified trusted capability boundary; refuse only for an actual denied capability, missing required containment, explicit scoped operator preference or invalid API contract. Keep text-based preference filters as *best-effort workflow controls*, prominently distinguished from host containment.

## Decision

1. **Authority is independent of representation.** A request's permitted effects derive from an immutable daemon-authorized principal, project/workspace/worktree, agent hierarchy, approval mode, requested sandbox/network/credential/resource ceilings and obtained executor capabilities. Shell spelling, Git subcommand, script name, interpreter, argument order, or routing adapter cannot grant more authority.
2. **Syntax is advisory.** Regexes, substrings, ASTs, classifiers and reviewer models may annotate risk, explain proposed actions, or escalate to the existing ApprovalRouter. Their failure to recognize an action cannot expand the sandbox or bypass explicit operator/tool authorization; their match alone is not evidence that the effect is impossible to accomplish under another spelling.
3. **Reserve hard rejection for valid enforceable cases:** authenticated deterministic Deny decisions; disabled/unavailable tools; explicit operator/session/agent/project authority ceilings; typed-invalid or unsupported tool requests; dangerous access denied by actual filesystem/network/process/credential controls; failed required setup; integrity protection for trusted helper and execution metadata. An explicit operator-selected command-text deny remains a local UX/workflow prohibition on matching invocations, with conspicuous `best_effort_text_match` semantics, *not* a statement that equivalent program behavior is forbidden. Preserve explicit denies rather than silently deleting user rules.
4. **Remove built-in syntax-only terminal command blockers.** The Bash catastrophic `HARD_DENY_PATTERNS`, literal prefix deny table, heuristic `SecurityService` critical denials and similar duplicate builtin text bans should be reclassified as risk/approval signals after a documented compatibility pass; do not substitute an equally bypassable command parser elsewhere. Destructive behavior in a writable workspace is still possible; mitigate using scoped authority, approval, checkpoints/undo and provenance. For unsandboxed FullHost, state explicitly that approval cannot provide technical protection.
5. **Operator-directed approval remains meaningful.** Permission rules, hard negative authorization receipts, Ask/Automatic/Yolo semantics and explicit grants remain intact. A security reviewer may request review or reject according to the established bounded policy, but cannot expand the execution ceiling. High-risk hints must be evaluated *before* any auto-approval rule allows a command merely because its prefix appears benign, subject to existing higher-priority explicit operator grants/denies and the acceptance tests defined in M002.
6. **Git uses typed execution classes, not name blacklists.** Trusted daemon-owned metadata operations must not execute repository-controlled hooks, filters, fsmonitors, external commands or credentials with additional authority. Potentially executable operations must either run through a verified contained child (and narrowly authorized network/credentials), be implemented via a genuinely safe non-executable typed mechanism, or return a **typed capability-unavailable/approval-required** outcome. Whole Git subcommand family blocking and fabricated missing executable are not the desired contract. No unsafe direct-host fallback to make integration tests pass.
7. **Real enforcement must be truthfully reported.** The existing Landlock-only backend restricts filesystem rights but not network/process/Unix IPC. The optional Linux namespace/seccomp/mediated egress roadmap owns stronger obtained guarantees. User-facing policy distinguishes requested/obtained rights and the specific reason for denial/escalation. No implied Linux, macOS or Windows parity without native evidence.
8. **Observation measures actual execution boundaries.** Preserve bounded/redacted command or argv provenance, route selection, signed/scoped decision receipt, executable identity, effective policy, requested-vs-obtained capabilities, outcome and teardown. Record when a script/native tool is opaque; do not claim to observe every internal syscall/child effect merely from a command digest. Prefer checkpoints and restricted environment rather than demanding agents expand shell text to pass a parser.
9. **Routing cannot launder authority.** Bash routing to Python, typed Git, native tools, plugins, LSP helpers or a raw shell must preserve the authorization snapshot and verified executor ceiling; a failed sandbox or rejected privilege path must not retry through a more privileged owner without explicit approval.
10. **Staged migration and testing.** First map all existing syntax-only denials and priority rules. Introduce typed disposition/provenance and correct approval ordering. Convert built-in syntax hard-blocks to advisory escalation with explicit FullHost caveats, preserving user-configured text filters as documented best-effort controls. Test effect-equivalence across shell, wrapper, scripts, typed Git and native tools. Preserve historical closure records, and run Linux/macOS/Windows nonregressions.

## Consequences

Positive: fewer obstruction/bypass loops, more legitimate workflows, clearer reviewer authority, a single security vocabulary, and reproducible cross-tool denial/approval traces.

Negative: previously hard-blocked spellings may become approvable subject to the selected runtime's actual authority; in explicit FullHost, permissive approval is genuinely host-dangerous. A syntax classifier does not reliably infer latent effects, and auditing launch events cannot guarantee intent or data-flow observability inside a child.

Deferred: higher-fidelity OS process/syscall/network audit, VM isolation and automatic safe recovery for all local workspace mutations. These are not a prerequisite for honest authority semantics.

## Compatibility and migration

Maintain stored user/admin `bash_deny_patterns`, `bash_allow_patterns`, `bash_ask_patterns`, tool toggles and explicit deny receipts, with visible best-effort-text-filter labels for matching rules. Preserve their invocation-level matching where configured, but make no host-level security claim. Changes to built-in default catastrophic denials require a documented behavior migration, targeted regression fixtures and operator-facing notes for FullHost/YOLO. Avoid new persisted enums unless necessary; when added, version readers/writers and handle older clients conservatively. No automatic FullHost or extra network rights.

## Security and reliability implications

Adversarial script/alias/wrapper/compiled-program and tool-route tests must show the same **enforced capability limit** despite different syntax. Audit provenance must be bounded/redacted and should not become a script-contents logging channel. A benign-looking command can dynamically load code; no blanket "safe cargo/git/python" execution exemption from sandbox scope. Required backend failure prevents execution even after reviewer approval. Respect cancellation, process limits, scratch leases, restart recovery and authenticated child policy narrowing.

## Verification

Native effect-equivalence fixtures: workspace vs outside-root mutations, credential file reads, network egress, hidden Git hook/filter command execution, nested script/native binary and alternate tool entrypoints; paired positive authorized operations; approval matrix including explicit operator deny, Ask, Automatic, Yolo, FullHost, and child ceilings. Validate exact obtained enforcement receipt, no target marker on failed required setup, no route fallback, and no dishonest "safe" label. Closure requirements and owner are in the command-policy roadmap.

## Supersession

None. This ADR refines ADR-0004 and complements accepted Linux ADR-0035; previously accepted ADRs remain historical.
