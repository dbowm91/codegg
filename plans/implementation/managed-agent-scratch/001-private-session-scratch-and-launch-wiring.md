# Managed Agent Scratch M001 — Private Session Scratch and Canonical Launch Wiring

Status: ready for handoff
Repository baseline: `c0d6ff2d324ee37005e828a5bf269feebfabff79`
Source roadmap: `plans/subsystems/managed-agent-scratch-roadmap.md#7-milestones`
Long-term requirements: `plans/000-long-term-specification.md` §§4, 9, 16–18, 26, 27, 29; `plans/001-terminology-and-domain-model.md` §§5–8; `plans/002-long-term-roadmap.md` Phases 9, 10, 19.
Applicable ADRs: `plans/adrs/ADR-0001-programmatic-tool-execution-authority.md`; `plans/adrs/ADR-0002-agent-run-worktree-isolation-and-control.md`; accepted ADR-0028 command authorization.
Primary class: capability.

## 1. Objective

Deliver end-to-end private **session-scoped** scratch usable by a model without separate tool calls or extra out-of-workspace approval, with CodeGG-generated identity, explicit prompt guidance, standard temp environment, safe basic lifecycle and existing sandbox guarantees. An agent can create a temporary artifact in one shell call and consume it in a later call of the same session.

## 2. Why ready

Daemon session identifiers, ToolBroker, ManagedProcessService, durable jobs and Required-sandbox launch support exist. This does not depend on a new OS backend, background-job redesign or blackboard. Inspect actual branch head before code edits; freeze the smallest typed context boundary.

## 3. Baseline evidence

`src/managed_process.rs` currently provisions per-**launch** `codegg-run-` scratch under `SandboxRequest::Required`, grants read/write and sets TMPDIR; `src/python_script/executor.rs` provisions independent `codegg-python-` script temp and `src/tool/bash/process.rs` provisions Windows shell temp. `EnvironmentPolicy` rebuilds the child environment. `src/agent/loop.rs` holds session_id/turn_id/workspace_root; `src/tool/broker.rs` is the unique tool-call boundary. Existing `tool_write_allowances()` deliberately does not grant ambient /tmp on Unix. No session-persistent scratch identity was verified.

## 4. Nonregression invariants

- No daemon cwd or caller-supplied absolute path as scratch authority. Relative workspace paths remain relative to workspace_root.
- FullHost vs Required profile semantics and explicit Deny remain intact. Required never falls back to FullHost.
- `child_workspace_ceiling` stays separate from ordinary `workspace_root`; authorization is not widened by string/prefix match.
- Human-originated `!cmd` and plugins/IDE/formatters do not silently inherit model scratch grants. Background scheduled model work does.
- Exactly one scheduler admits machine work; spawn flows through ManagedProcessService; no global process environment mutation.
- No new /tmp interception, command regex blocker, shell rewriting, or dedicated tool.

## 5. Scope

In: managed user-owned scratch root, per-session stable private resource ID/path, lazy atomic creation, ownership record and bounded recovery; model prompt and env; file-tool read/write authorized path; Required-sandbox integration without removing existing private launch staging; session-local tests and minimal user/operator docs.

Out: child-run-specific roots and sharing (M002), remote nodes, cross-session sharing, full cleanup UI/quota, native isolation parity claims, build-cache relocation (all deferred).

## 6. Required production changes

**Core/domain:** associate stable `ScratchId` (or equivalent typed opaque ID) with canonical authenticated session owner, never with a path supplied by model. Implement one daemon-owned manager; choose per-user persistent CodeGG-managed state dir, not shared /tmp. Reuse session storage or owner-only manifest if possible; explicitly justify migrations. Avoid a second SessionStore or process launcher.

**Storage:** securely create parent and child directories (0700 Unix; private owner ACL Windows as supported), unpredictable segments, reject preexisting symlink/reparse-point and non-owned parent paths. Multiple attachments / concurrent tool invocations resolve to same session-scoped object via transactional/idempotent allocation. Fail closed on directory/provisioning error. Keep creation/version metadata separate from untrusted files inside scratch. On daemon restart, load/reconcile by authoritative session ownership; never infer authorization from scanning directory names.

**Runtime:** attach scratch metadata to the model execution context, propagate through ToolBroker into accepted scheduler payload/attempt, and set `CODEGG_SCRATCH_DIR`, `TMPDIR`, `TMP`, `TEMP` through `EnvironmentPolicy.with_var` **after** its sanitization and before managed spawn. Do not trust ambient user variables as CodeGG grants. Existing `src/managed_process.rs` per-launch TempDir and Python script temp may remain subordinate/internal; explicitly choose behavior for Required launches such that the model-visible writable path and env are consistent. Do not break sandbox status reporting, process-tree cleanup or Python script staging.

**Authorization:** extend existing validated filesystem-tool and sandbox allow-root calculation narrowly to this authenticated session resource, preserving explicit deny precedence, no same-user sibling access claims, and no automatic permission to write the scratch parent. Unsandboxed arbitrary child programs are cooperative; never claim OS-enforcement for them.

**Agent UX:** inject short bounded, trusted initial context: private path, recommendation for disposable artifacts, no approval required *within grant*, other paths follow existing permission behavior. Do not append unbounded per-turn context or a model-visible scratch tool.

**Docs:** update `architecture/process-tool-execution-ownership.md`, `architecture/permission.md`, `architecture/security.md` and user-facing `docs/tools.md` as actually affected.

## 7. Ordered work packages

A. Trace session identity through main model bash/Python, ToolBroker and scheduler; document source owner and all dispatch paths. Reconcile existing launch-private temp owners rather than duplicate.
B. Implement stable owner-private resource and storage/restart provenance with atomic allocation; demonstrate two frontends/same session converge.
C. Wire explicit scratch env and prompt on model contexts; preserve human-only and Required sandbox behavior.
D. Wire structured file tools and confined subprocess path grants; validate real integration under supported local profiles, negative tests, documentation and closure evidence.

## 8. Failure, cancellation, restart, contention

Concurrent first usage produces one resource. New allocation failure yields an actionable error and no global /tmp fallback presented as managed. Interruption leaves a bounded recoverable record; never reclaim while active session/job is using the directory. Restart must not mistake an old on-disk directory for an authorized active grant. For M001, provide safe conservative cleanup/retention rather than aggressive orphan deletion; M003 qualifies full reclamation.

## 9. Compatibility and migration

Existing session and job snapshots decode with defaults; no protocol fields unless demonstrated necessary. Migrate schema and bump `STORAGE_LAYOUT_VERSION` only if a real migration is introduced. Existing old sessions lazily obtain scratch and must not be forced into a new workspace. Environment vars override ambient temp **only for model-owned subprocesses**; retain child launch staging where needed. Document supported and unknown native platform behavior.

## 10. Required tests

- Distinct sessions distinct opaque private paths; concurrent same-session calls and reconnect share one.
- Two successive shell calls write/read same scratch artifact; launched Python `tempfile.gettempdir()` sees same appropriate authorized session root (or document a specifically justified, stable usable child route).
- Exact `CODEGG_SCRATCH_DIR`, `TMPDIR`, `TMP`, `TEMP` injection under sanitized env; no inherited user override, no human-shell regression.
- File tool can read/write its authorized scratch but not parent/neighbor; symlink escape/TOCTOU negative coverage.
- Required sandbox includes authorized root and preserves explicit deny, private launch scratch and status. Unsupported backend refuses rather than quietly disabling policy.
- Failure to allocate and restart before/after durable registry commit; no unauthorized orphan reuse; background process not killed by foreground turn ending.

## 11. Verification commands

```bash
cargo fmt --all -- --check
cargo test -p codegg-core
cargo test -p codegg --test tool_execution
cargo test -p codegg --test sandbox_containment
python3 scripts/check_execution_ownership.py
python3 scripts/check_scheduler_bypass.py
python3 scripts/check_sandbox_policy_wiring.py
python3 scripts/check_daemon_cwd_usage.py
bash scripts/check-core-boundary.sh
scripts/verify.sh quick
```

Add focused test commands for new scratch tests. Do not use all-features (real LSP tests); run workspace Clippy in supported `--all-targets` configuration after focused tests. Native Linux/macOS/Windows claims require real platform evidence and availability disclosure; no SBC startup benchmark.

## 12. Static guards/docs

Update `docs/execution-ownership.toml` only if adding classified spawn sites (prefer zero new sites). Regenerate agent builtins only if modifying generated prompt sources; never edit generated builtins by hand. Keep existing security and tool boundary checks green.

## 13. Acceptance

An actual admitted model-owned execution sees a valid private scratch path in context and inherited child env, can write/read across tool invocations without out-of-workspace approval, while another session and human execution do not acquire its grant. Required backend rules do not widen; no alternate raw spawn exists. No claim from unit-only testing.

## 14. Stop conditions

Stop and record a blocker if Required sandbox must be weakened, if session ownership cannot be unambiguously threaded into scheduler jobs, if file access remains path-only authority, or if ownership recovery permits cross-session scratch reuse. Escalate a real cross-subsystem authority decision as an ADR rather than inventing a second dispatcher.

## 15. Closure evidence

`plans/closure/managed-agent-scratch/001-status.md`: exact implementation SHA/PR, code ownership matrix, real multi-invocation trajectory, sandbox path-grant matrix, restart and contention results, migration posture, platform test outcomes, guards, negative findings, residual blockers and explicit M002 unblock decision.

## 16. Handoff notes

Review current branch, `AGENTS.md`, `.opencode/skills/planning/SKILL.md`, existing `src/managed_process.rs` scratch, Python staging and Windows shell temps. Respect user changes and current CI failures; do not claim product or deployment readiness.
