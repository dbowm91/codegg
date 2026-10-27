# ADR-0055: Session-relative working directory authority and cross-workspace transitions

Status: **accepted** — Option A (accepted by the user on 2026-10-27)
Date: 2026-10-27
Decision owners: project maintainers
Repository evidence baseline: `80171f45295bebb9ab0b521262c0b295acb93074`

References: `plans/000-long-term-specification.md` §§6, 9, 13, 16–17, 26–29; `plans/001-terminology-and-domain-model.md`; `plans/002-long-term-roadmap.md`; accepted ADR-0002, ADR-0033, ADR-0047 and ADR-0054.
Affected roadmaps: `plans/subsystems/workspace-authority-and-session-navigation-corrective-addendum.md` (W004) and `plans/subsystems/agent-worktree-placement-and-build-affinity-corrective-addendum.md`.

## Context and problem
The *main* agent's model-facing `bash` accepts `workdir` for **one invocation** (`src/tool/bash.rs`); `cd dir` changes that shell only, not the `SessionRuntime.workspace_root` or subsequent file/tool calls. `ProjectExecutionContext` in `src/tui/app/state/execution_context.rs` is client-tab state captured at dispatch. Existing `codegg launch --worktree new|<id>` creates or chooses a **managed** checkout and creates/forks a **session**; it is not a persistent in-place change to an active session's workspace. ADR-0033 forbids process-global daemon `chdir`. W001 closed process-cwd fallbacks; W002 still needs session-switch root adoption.

The user needs an understandable response to "work in `crates/foo` from now on" and "switch to that other repository/worktree," including after reconnect, without accidentally widening repository, tool, sandbox or principal authority. Current model-facing tools do not own a durable session-relative cwd selector.

## Options

A. **Recommended:** Maintain an optional, canonical, session-scoped *relative working directory* within the session's already-authorized workspace root. A host-authorized `get|set|reset` operation updates that directory after verifying containment; subsequent turn/tool construction snapshots the effective directory, while the immutable **workspace security ceiling**, project binding, `WorkspaceId`, LSP/project identity, and Git repository remain unchanged. Cross-root requests are explicit **session launch/fork/switch** actions using the existing project/worktree authorization rather than silent rebind of a running session. Subagents inherit an immutable snapshot of the effective cwd but cannot modify the parent's preference.

B. Replace the daemon process cwd via `chdir` or let a shell `cd` act as global state. **Rejected**: races unrelated TUIs, background jobs, and delegated turns; violates ADR-0033.

C. Let the main agent rewrite the session's `workspace_root`, `project_id` and worktree binding in place across arbitrary repositories. **Rejected** absent a separate, fully authorized rebind ADR: mixes identity and locator changes, may strand running operations, and circumvents session-launch lifecycle and managed worktree leases.

D. Have the model attach a different managed worktree by naming an absolute path. **Rejected** as a direct mutation: locator is not checkout ownership or authorization.

## Decision

1. Persist **two distinct locators**: immutable session-bound canonical `workspace_root` (permission and sandbox ceiling) and optional `relative_cwd` (tool default for that session). `relative_cwd` must resolve to an existing directory *within* the root, with canonical symlink resolution and revalidation on every use. No outside-root path and no implicit workspace registration.
2. The daemon and session storage, not the model's prose or a shell environment, own accepted cwd state, revision, audit event, and authorization. A user request to change cwd does not itself prove an agent-supplied path safe; ordinary path and principal checks still apply. The model's `get|set|reset` action may only narrow the default within its existing root. Every mutation requires a matching expected revision, bound session identity and a clearly surfaced effective path.
3. Workdir changes affect **later** tool/turn construction only. In-flight tools, jobs, idle continuations, delegated agents, and group members retain the immutable execution context captured at submission. Do not override explicit per-tool `workdir`, and do not change checkout lease/path policy, scratch, or session project identity.
4. Inside-root cwd defaults for file tools and command runners must be consistent; nested relative paths must not accidentally be rooted at daemon cwd. LSP/project-graph initialization, Git common-root identity and authorization continue to use `workspace_root`. Revalidate cwd after rename, removal, symlink changes and reconnect; if invalid, refuse the requested operation visibly rather than guessing another root.
5. A request crossing to another checkout or project uses `SessionLaunchService` to create/fork/activate an authorized session with its own binding. An external manually-created checkout cannot become a managed worktree merely because the model names its path; its normal project-open path remains separately authorized. No implicit parent-worktree lease reassignment or mutation of a live run.
6. Frontends expose the active session root and relative cwd separately. A project/session/tab switch changes the selected *session* (W002), never the daemon process cwd; a model-requested cross-root transition requires an explicit user-facing handoff rather than a silent switch.

## Security and concurrency
A model-authored path is untrusted. Reject `..` traversal, symlink escape, wrong principal/project, stale revision, and relative cwd pointing at an unregistered root. Use daemon-resolved canonical paths and typed errors. One session's change must never retarget another session's tool call or an already-submitted turn. Changing the in-root *default* does not create permission to access any path not already authorized by the session sandbox.

## Consequences and non-goals
This requires additive session state and protocol/tool/UI wiring, plus reconnect/restart tests. It does not add a general-purpose filesystem `chdir` primitive, arbitrary-root rebind, new sandbox exemptions, or unrestricted shared checkout processes. A shell-only per-invocation workdir remains supported without any new feature.

## Acceptance record
The user accepted **Option A** on 2026-10-27. W004 may implement the contract above after W002 strictly closes. W002 and the independent worktree/build corrective proceed under their own plans. The separate existing-worktree read-only placement contract remains subject to its own implementation review; this ADR does not authorize writable sharing.

## Verification required after decision
Same-session set/use/reset across tools and turns; A→B→A session switch; two TUIs + one daemon; action while jobs/child runs remain active; reconnect/restart; stale revision and symlink/rename rejection; different-principal denial; no cwd drift in daemon; no migration that silently rewrites existing sessions. Closure evidence must label simulated versus real provider and host coverage.
