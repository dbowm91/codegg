# Linux Execution Isolation M001 Corrective C001 — Typed Git Policy and Execution-Class Reconciliation

Status: implemented (closure: `plans/closure/linux-execution-isolation/009-status.md`)
Primary class: invariant
Target closure: `plans/closure/linux-execution-isolation/009-status.md`
Repository baseline: Linux branch `c7c18181862d67c9e2a649399bbda9665869edd8` (M001 partial implementation), as compared with `main` on 2026-10-13. Source: `plans/subsystems/linux-execution-isolation-roadmap.md` and accepted ADR-0042; Linux mechanism ADR-0043 remains proposed. Corrects M001 `plans/implementation/linux-execution-isolation/001-trusted-git-execution-boundary.md` and historical blocked `plans/closure/linux-execution-isolation/001-status.md` without revising its evidence. Long-term: `plans/000-long-term-specification.md` §§4,17,27–29 and `plans/001-terminology-and-domain-model.md` §8. New corrective closure must be independent and preserve original historical M001 findings.

## 1. Objective

Remove fabricated Git spawn failures and **Git subcommand-name bans masquerading as security**; replace them with typed capability/authority evaluation and a distinction between strictly non-executable trusted Git and repository-executable contained Git. Restore the accepted benign non-executable Git flows without running attacker-controlled filter/hook/transport code at daemon authority. Do not claim ordinary executable Git/remote workflows restored until C002/C003 and their real kernel/broker boundaries exist.

## 2. Defects and why old verification missed them

M001 put `REPOSITORY_EXECUTABLE_OPERATIONS` behind `trusted_git_request_denied()` and launches `/__codegg_trusted_git_operation_denied__` to simulate a policy error. The native hook/fsmonitor fixtures verified "marker absent" but did not require accepted Git mutations to succeed. Nine of twelve `git_mutations_integration` cases fail for legitimate add/checkout/filter/merge/network operations. Static policy guards proved canonical use of the rejecting constructor, not adequacy of the policy. Caller receives generic `Io` spawn error, not actionable deny/approval/missing capability. Existing `GitExecutionService::execute` may fall back from a structured read to raw invocation, creating ambiguity of actual authority and failure semantics. `src/git_network_policy.rs` overlay tries to restore network variables but always-stripped variables are then removed; observe, do not opportunistically re-enable credentials.

## 3. Invariants and scope

- Daemon-owned Git has no ambient authority to execute repository config, hooks, fsmonitor, external diff/textconv, filters, merge drivers, shell-capable transport, credential helpers or arbitrary SSH commands under a privilege higher than the agent.
- A Git **operation** maps to one explicit requested execution class and capabilities; not blanket allow/deny by subcommand name. A needed executable extension must have a genuinely applied contained route, otherwise return `CapabilityUnavailable` or `RequiresApproval` with typed reason; never silently route into FullHost.
- Preserve trusted read operations that are demonstrably non-executable and networkless. A Git command considered "read-only" must still be checked for hooks, fsmonitor, pager, textconv and linked config.
- No malformed/unknown Git argv can inject trusted `-c` config, repository identity, path or environment; validate or refuse for typed contract **validity**, not because the command's word is classified dangerous.
- Don't claim Landlock filesystem only is enough to contain arbitrary Git filters with unrestricted network and IPC. Do not widen network/credential grants.
- No broken FFI or Linux-specific dependency in the reusable `egggit` crate: classify capabilities generically, enforce runtime placement in Codegg root/ManagedProcessService.
- Preserve existing process timeout/output/process-group cleanup, worktree/agent identity and redacted RunStore results.

## 4. Production changes

A. Inspect `crates/egggit/src/process.rs`, `crates/egggit/src/status.rs`, `src/git_service.rs`, `src/git_mutations*.rs`, `src/git_network_ops.rs`, `src/tool/git.rs`, `src/git_recovery.rs`, worktree helpers and raw Git fallbacks. Enumerate typed operations vs potential executable Git capabilities (filters, hooks, pager, textconv, external diff, merge driver, network, SSH agent, credential helper), configured per operation and repository/worktree. Do not try to fully parse arbitrary Git scripts to prove benignity.

B. Introduce a typed result or error `GitExecutionPolicyOutcome` distinguishing `AllowedTrustedNonExecutable`, `RequiresContainedExecution{capabilities}`, `RequiresAdditionalAuthorization{capabilities}`, `DeniedExplicitAuthority`, `InvalidRequest` and `BackendUnavailable`. Variant names may adapt to existing error taxonomy but preserve semantics. Eliminate `DENIED_GIT_EXECUTABLE`, missing-executable policy and status masking. Configure proper status/provenance through `GitMutationError`, `EgggitError`, caller responses and audit without introducing a broad daemon protocol.

C. Keep existing strict `-c` pins and environment cleanup for **trusted** background operations; add explicit per-command verification that dynamic Git config cannot inject executable code through textconv/filter/fsmonitor/pager/etc. For operations requiring such extensions, provide a typed deferred/contained route, not a classification-derived string block. Strictly validate argv *structure* and repository identity independently of "command is bad" heuristics. Fix the structured-read fallback to preserve a denied/unsupported result rather than run a more permissive raw Git path.

D. Add integration/compatibility probes for local operations, classifying each current M001 test failure as: non-executable safe Git operation possible now; needs future contained filters/hooks; genuinely network/credential-gated; or currently unsupported by design. A safe operation may be restored immediately only when tests prove no privileged repository-controlled program executed under hostile metadata. Contained route remains "unavailable" until native enforcement measured.

E. Update `architecture/git.md`, `docs/validation/git-security-review.md`, `scripts/check_git_forbidden_patterns.py` and M001 corrective closure with precise categories. Preserve all historical evidence in M001 001-status without pretending that C001 closes its entire workflow compatibility blocker.

## 5. Ordered work packages

1. Capture old integration 9/12 failures, hook/fsmonitor success, fake-executable errors and automatic raw fallback; test distinction in public results.
2. Implement typed policy classification and error propagation without any required Linux sandbox backend rollout.
3. Restore truly non-executable operations with adversarial Git config, .gitattributes, linked worktree and includes fixtures; reject or defer executable cases.
4. Add static guards rejecting fabricated executable and raw bypass; keep actual Git process identity.
5. Run tests and record remaining executable local/remote Git requirements to C002/C003, then create C001 closure.

## 6. Test and verification

`cargo fmt --all -- --check`; `cargo test -p egggit`; `cargo test -p codegg --lib git`; `cargo test --test git_mutations_integration -- --test-threads=1`; `cargo test -p codegg --test sandbox_containment -- --test-threads=1`; `python3 scripts/check_git_forbidden_patterns.py`; `python3 scripts/check_execution_ownership.py`; `scripts/verify.sh quick`. In the known 9 failures, accept a deliberately *typed* contained-backend-unavailable result for executable operations **as an interim C001 state**, not as complete Git compatibility and not as a pass of the old integration expectation. Add separate negative/positive tests: attacker hook/merge/filter/fsmonitor markers absent on trusted path, specific missing capability typed, valid status and index-free read completes, no raw fallback, malformed argv refused, no after-approval privilege upgrade.

## 7. Failure, recovery and migration

A typed denied/missing capability result must be terminal for that attempt, with no Git child spawn and no fabricated ENOENT; retries require explicit policy change or a newly available backend, never silent fallback. Existing settings and typed Git operations remain parse-compatible; new result fields may be additive and versioned. Concurrent Git operations still obey worktree leases; timeout/cancellation and output bounding unchanged. No implicit network credential grant.

## 8. Acceptance

No execution-restricting Git subcommand-name blacklist, no manufactured nonexistent executable, and no unreviewed raw fallback. Authorized genuinely safe local Git operations work under hostile repo metadata and without elevated code execution. Operations needing executable local Git functionality return a typed, comprehensible `contained backend unavailable` pending C002; network/remote operations return separately typed network/credential-unavailable pending C003. The historical full Git mutation compatibility suite may remain failing and M001 remains blocked until C002/C003 native/transport requirements close. Evidence includes exact tests/host, fixture outputs and a requirement mapping.

## 9. Stop

Stop if any trusted path runs hostile executable metadata, if typed denial can be converted into raw Bash/FullHost, if the revised error API loses caller provenance, or if fixing interoperability requires broad unsandboxed Git access. That work belongs to C002/C003 or an explicit ADR.
