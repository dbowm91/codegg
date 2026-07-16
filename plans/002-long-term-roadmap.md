# CodeGG Long-Term Implementation Roadmap

Status: execution roadmap for `plans/000-long-term-specification.md`

Terminology: `plans/001-terminology-and-domain-model.md`

This roadmap orders the work needed to reach the long-term CodeGG architecture while preserving a useful solo-development product at every stage. Each phase MUST leave the repository in a coherent state and MUST include focused implementation plans, migrations, tests, documentation, and closure evidence before the next dependent phase is treated as available.

The roadmap is dependency-ordered, not calendar-ordered. Parallel work is appropriate only where the dependency notes allow it.

## Cross-phase execution rules

Every phase MUST:

1. preserve the singleton-daemon and scheduler-ownership invariants;
2. use explicit project/workspace/session/node context rather than process-global cwd;
3. add typed protocol and storage representations before frontend-only state;
4. maintain backward-compatible migrations or document an intentional break;
5. add bounded snapshots and events rather than unbounded payloads;
6. include restart, cancellation, duplicate-delivery, and contention tests where applicable;
7. include source provenance and diagnostics for loaded configuration or assets;
8. update architecture documentation and static ownership guards;
9. leave personal-local mode operational without team configuration;
10. record explicit exit evidence in the implementation plan or status artifact.

## Phase 0 — Canonical domain and compatibility foundation

### Objective

Introduce the domain language and typed identity relationships required by all later work without prematurely changing user behavior.

### Deliverables

- Add or formalize `ProjectId`, `RepositoryId`, `NodeId`, `PrincipalId`, `AgentRunId`, `AgentTaskId`, `WorktreeId`, `ProviderConnectionId`, `ChannelId`, and `AuditEventId` in appropriate core crates.
- Define serialization, validation, display, and database conversion rules.
- Add explicit relations from sessions and workspaces to projects.
- Mark path-derived `project_id`, `directory`, and process-global project fields as compatibility projections.
- Add migration helpers that can assign stable projects to existing registered workspaces and sessions.
- Add architecture docs matching `plans/001-terminology-and-domain-model.md`.
- Add static checks or review guards preventing new path-derived project identity in daemon-owned code.

### Dependencies

None beyond the current daemon/workspace/scheduler baseline.

### Exit criteria

- New production code distinguishes project, repository, workspace, and worktree.
- Existing sessions migrate or fail with actionable rebinding diagnostics.
- No new daemon-owned operation derives durable project identity from path text.
- Protocol DTOs can carry stable project identity while retaining compatibility fields.

### Required tests

- typed-ID round trips;
- migration from existing workspace/session rows;
- path rename without project-ID change;
- two workspaces mapped to one project;
- compatibility-client behavior;
- static guard against new process-global project identity.

## Phase 1 — Runtime asset registry, interoperability, and refresh correctness

### Objective

Eliminate stale daemon-loaded skills and custom agents while establishing clean interoperability with other harnesses used in the same repository.

This phase is intentionally early because every later multi-project, team, ACP, and distributed session must construct runtime state from a correct project-scoped asset snapshot.

### Deliverables

- Replace the mutable vector-style skill index with a source-aware project asset registry.
- Introduce immutable `ProjectAssetSnapshot`, source provenance, content digest, validation diagnostics, and asset generation.
- Refactor agent loading to use explicit project/workspace context rather than `PWD`.
- Retain compiled built-ins and CodeGG TOML overlay semantics.
- Discover project skills from:
  - `.codegg/skills/<name>/SKILL.md`;
  - `.agents/skills/<name>/SKILL.md`;
  - `.opencode/skills/<name>/SKILL.md`;
  - `.claude/skills/<name>/SKILL.md`.
- Discover global skills from CodeGG config, `~/.agents/skills`, OpenCode config, and Claude-compatible locations.
- Support the portable Agent Skills frontmatter and directory/resource model.
- Preserve direct native `.codegg/skills/*.md` support as a compatibility path.
- Define deterministic precedence, shadowing diagnostics, invalid-source behavior, and configurable source enablement.
- Ensure foreign harness directories are read-only unless the user explicitly selects a write target.
- Add project activation, session create, session open/attach, workspace rebind, and manual-refresh triggers.
- Add a unified manual command such as `/reload` with focused `/reload skills`, `/reload agents`, `/skills refresh`, and `/agents refresh` aliases or equivalent discoverable commands.
- Return structured refresh reports with added, removed, changed, shadowed, invalid, and retained entries.
- Build candidate snapshots transactionally and atomically swap only validated results.
- Pin active turns and agent runs to immutable runtime-asset snapshots.
- Make subsequent turns use the newest successful generation.
- Record agent-definition and activated-skill digests in run/audit metadata seams.
- Add bounded resource access and path validation for bundled skill resources.

### Dependencies

Phase 0 project/workspace identity.

### Exit criteria

- Opening any session refreshes its project assets before the next turn runtime is created.
- A daemon can remain alive while skills or custom agents are added, changed, removed, or shadowed without becoming indefinitely stale.
- Active turns do not mutate mid-turn after refresh.
- Duplicate skill names produce deterministic winners and inspectable diagnostics.
- CodeGG can use portable skills created for OpenCode, VS Code/Agent Skills, or Claude-compatible locations without copying them into `.codegg`.
- Foreign skill scripts never execute during discovery or activation.

### Required tests

- every discovery location and global/project precedence combination;
- nested directory walk bounded by Git worktree/project root;
- duplicate name shadowing and source provenance;
- invalid higher-precedence source with valid fallback;
- symlink escape, oversized skill, malformed YAML, recursive resource reference, and resource path traversal;
- session-create/open/attach refresh triggers;
- manual command refresh report;
- failed refresh preserving prior snapshot;
- in-flight turn snapshot pinning;
- concurrent refresh coalescing and atomic swap;
- remote-workspace manifest seam tests for later phases.

## Phase 2 — Eggpool and daemon-owned provider connections

### Objective

Make provider configuration a daemon resource and add Eggpool as the first explicit shared connection type.

### Deliverables

- Add durable `ProviderConnection` records and scopes: personal, project, deployment.
- Add secret references rather than embedded API keys.
- Extend `/connect` with Eggpool host, default port `11300`, TLS policy, API key, display name, and scope.
- Normalize endpoint URLs and reject ambiguous or unsafe configurations.
- Add bounded authentication/health/model discovery probes.
- Add stable connection IDs to session/provider selection.
- Expose connection health, model catalog, source, scope, and redacted diagnostics.
- Add credential rotation and deletion behavior.
- Preserve existing direct provider configuration through a migration or compatibility adapter.

### Dependencies

Phase 0 identity. Phase 1 asset work may proceed in parallel after Phase 0.

### Exit criteria

- Several sessions and TUIs can share one Eggpool connection.
- API keys never appear in protocol events, logs, chat, or persisted plain project configuration.
- Project-scoped and personal connections are distinguishable and authorization-ready.
- Provider runtime no longer assumes frontend-owned credentials.

### Required tests

- default and explicit ports;
- TLS policy and endpoint normalization;
- invalid credentials and unavailable endpoint;
- model discovery bounds;
- secret redaction;
- concurrent connection use;
- rotation during idle and active sessions;
- compatibility configuration migration.

## Phase 3 — Project catalog and lazy discovery

### Objective

Give the singleton daemon a durable catalog of local and remote project candidates independent of currently active sessions.

### Deliverables

- Add project records, repository metadata, discovery roots, explicit projects, and project health.
- Support bounded directory depth and Git/directory discovery modes.
- Add lazy activation of workspace service bundles.
- Add one-off local project registration.
- Add placeholders for SSH and linked-node locators without implementing remote execution yet.
- Add project list, get, register, archive, refresh, and health protocol operations.
- Migrate current process-global server project state to request/session project scope.
- Add project catalog cache invalidation and restart hydration.

### Dependencies

Phase 0. Phase 1 SHOULD be complete so activation uses project asset snapshots.

### Exit criteria

- The daemon lists several projects without starting expensive services for all of them.
- Moving or renaming a workspace does not create a new project when repository identity is unchanged.
- Project archive does not delete workspaces or session history.
- The network server no longer assumes one global `project_dir`.

### Required tests

- discovery depth and ignore rules;
- lazy activation;
- duplicate repository/path reconciliation;
- archive/restore;
- restart hydration;
- symlink and permission boundaries;
- large-root bounded scanning;
- project asset refresh on activation.

## Phase 4 — Multi-project and multi-session TUI

### Objective

Make the TUI a true daemon frontend capable of several project tabs and several sessions per project.

### Deliverables

- Add global project catalog state to the TUI.
- Implement Helix-style `Space f` project picker.
- Add project tabs, next/previous navigation, tab restoration, and close behavior.
- Add project-local session picker and several sessions per project.
- Separate daemon connection state, project state, session state, and presentation state.
- Add bounded project badges for active sessions, jobs, health, and asset generation.
- Trigger project/session asset refresh before creating new turn runtimes.
- Ensure tabs do not hold exclusive workspace-service leases while inactive unless required.

### Dependencies

Phases 1 and 3.

### Exit criteria

- One TUI can operate several projects without process restart or cwd mutation.
- Several TUIs can operate different projects through one daemon.
- Session selection and model/agent selection remain project-correct.
- Closing a tab does not destroy its durable session.

### Required tests

- tab creation, switching, closure, and restoration;
- concurrent project event routing;
- session lists with identical titles across projects;
- stale/archived workspace behavior;
- asset refresh on session open;
- bounded inactive-tab memory and leases;
- keyboard and focus-state regression tests.

## Phase 5 — Frontend-neutral session projections and durable replay

### Objective

Create one canonical session view usable by local TUI, remote TUI, observer mode, ACP, web, and future clients.

### Deliverables

- Define `SessionProjectionSnapshot`, `TurnProjection`, tool/run projections, and agent-tree placeholders.
- Add project-scoped and session-scoped subscriptions.
- Persist or durably index replay beyond the current bounded in-memory remote-TUI buffer.
- Add monotonic sequence, acknowledgement, resume, and resync behavior.
- Add visibility/redaction classification to events.
- Keep large logs and artifacts behind handles.
- Remove frontend dependence on raw render frames.
- Add protocol capability negotiation for projection versions.

### Dependencies

Phases 0, 3, and 4. Agent-tree details can be expanded later.

### Exit criteria

- Two different frontend implementations can reconstruct the same session state.
- Reconnection from a known sequence is deterministic.
- Lag or expired history triggers bounded resynchronization.
- Secret-bearing tool arguments and outputs are redacted before projection.

### Required tests

- snapshot/event equivalence;
- replay after reconnect and daemon restart;
- sequence gaps and resync;
- visibility filtering by principal capability seam;
- payload limits;
- unknown event/capability compatibility;
- artifact-handle behavior.

## Phase 6 — Team principal model and daemon authorization seam

### Objective

Introduce individual identities and project authorization without making solo mode complicated.

### Deliverables

- Add principals, local owner, human users, service accounts, authentication sessions, roles, capabilities, and project memberships.
- Resolve local IPC callers to `LocalOwner` through OS peer identity where available.
- Add personal tokens and administrator bootstrap.
- Add an OIDC/device-login seam; implementation MAY be staged.
- Replace global bearer-token authorization as the team model.
- Add daemon-side authorization middleware for native protocol operations.
- Attach principal identity to clients, sessions, prompts, jobs, provider selection, and future audit events.
- Add project privacy: unauthorized users cannot enumerate project existence or presence.
- Define disconnected/expired credential behavior.

### Dependencies

Phases 0, 3, and 5.

### Exit criteria

- Personal-local startup remains login-free.
- Team clients have distinct identities.
- Project capabilities are enforced in the daemon.
- Viewer/Contributor/Maintainer/Owner roles function through explicit capabilities.
- Denials are structured and ready for audit.

### Required tests

- local-owner resolution;
- token creation/revocation/expiration;
- project enumeration privacy;
- role and capability matrices;
- session ownership and observation distinctions;
- provider-connection scope;
- authorization race during membership removal;
- network listener fail-closed defaults.

## Phase 7 — Presence and collaborator awareness

### Objective

Expose who is active in a project and what broad activity state they occupy.

### Deliverables

- Add project-scoped principal presence leases.
- Add session activity and agent-count summaries.
- Extend daemon snapshots with principal identity, client type, attached sessions, node, and last activity.
- Add heartbeat, idle transition, disconnect timeout, and reconnect behavior.
- Add project header and collaborator panel to the TUI.
- Add presence privacy and suppression policy.
- Ensure presence is ephemeral and not confused with audit history.

### Dependencies

Phases 5 and 6.

### Exit criteria

- Authorized members see active humans, session counts, agent counts, and bounded activity.
- Disconnects and idle transitions converge without stale permanent presence.
- Unauthorized principals receive no presence leakage.

### Required tests

- heartbeat/expiry/reconnect;
- multiple clients for one principal;
- several sessions per principal;
- node disconnection;
- privacy policy;
- high-churn bounded memory;
- daemon restart clearing/rebuilding presence.

## Phase 8 — Read-only observation mode

### Objective

Allow one developer to inspect another developer's session and agents in real time while discussing the project.

### Deliverables

- Add `session.observe` capability and observation subscriptions.
- Produce observer-safe session projection snapshots and events.
- Show prompts, agent output, explicit progress, tools, runs, diff summaries, permissions/questions, worktree, branch, and agent hierarchy according to visibility policy.
- Prevent observer permission responses, steering, cancellation, and mutations.
- Add observed-session TUI mode with a read-only main panel.
- Add project-chat side-panel placeholder and route insert-mode input there while observing.
- Add explicit control-request or suggestion seams without implementing implicit shared control.

### Dependencies

Phases 5, 6, and 7.

### Exit criteria

- A project member can select an active session and follow it in real time.
- The observed human sees no mutation caused by ordinary observer input.
- Secret-redacted events remain redacted through replay and snapshots.
- Observation survives frontend reconnect.

### Required tests

- observe allow/deny matrix;
- attempted observer steering/permission response/cancel;
- prompt and tool visibility policy;
- chat-input focus routing;
- session owner disconnect/reconnect;
- observer lag and resync;
- multiple observers and load bounds.

## Phase 9 — Durable multilevel agent-run service

### Objective

Replace first-level transient subagent semantics with a durable, scheduler-governed, multilevel agent hierarchy.

### Deliverables

- Add durable `AgentTask` and `AgentRun` stores with typed IDs.
- Record root, parent, session, turn, project, workspace, worktree, node, agent digest, provider/model, status, authority, budget, and timestamps.
- Replace per-session concurrency as the primary limit with deployment/project/principal/session/root/node limits.
- Add delegation policy, depth, fan-out, descendant-count, token, tool, and wall-clock budgets.
- Permit controlled descendant access to the task/delegation tool.
- Construct child runtimes with scheduler submission, explicit execution context, and a functional child spawner/service.
- Add idempotent delegation identities.
- Add all/any-successful/first-completed/detached join policies.
- Add downward cancellation and restart recovery.
- Project the agent tree through the native protocol and observer UI.
- Preserve narrow compatibility adapters for current `SubAgentTask`, `TaskStore`, and `SubAgentPool` call sites until migrated.

### Dependencies

Phases 0, 1, 5, and scheduler baseline. Phase 6 is required before team authority is complete; implementation MAY begin with local owner.

### Exit criteria

- At least three agent levels execute correctly.
- Descendants can delegate only within inherited authority and budget.
- Every run is visible in one tree and attributable to a root human/service/schedule.
- Cancellation, restart, duplicate spawn, and scheduler contention are deterministic.
- Independent sessions cannot multiply per-pool limits into uncontrolled machine load.

### Required tests

- depth/fan-out/budget boundaries;
- authority narrowing and attempted escalation;
- child task tool availability;
- three-level execution;
- idempotent duplicate delegation;
- join policies;
- root and selective child cancellation;
- daemon restart/recovery;
- many users/projects contending for global permits;
- observer tree projection;
- failure and orphan cleanup.

## Phase 10 — Worktree-native concurrency

### Objective

Make isolated Git worktrees the normal unit of concurrent mutation for humans and agents.

### Deliverables

- Add durable worktree records, owners, leases, states, health, and recovery.
- Add a daemon-owned worktree service using native Git abstractions.
- Add create, reserve, inspect, checkpoint, handoff, merge/rebase, archive, and remove operations.
- Bind sessions and agent runs to worktrees.
- Default parallel mutation-capable agent runs to independent worktrees.
- Add scheduler exclusivity keys for repository, workspace, worktree, build cache, database, and integration-test resources.
- Add orphan detection after daemon or node restart.
- Add TUI worktree/branch status and ownership views.

### Dependencies

Phases 0, 3, 4, and 9.

### Exit criteria

- Several humans and agents can modify one repository without sharing an index or working tree unintentionally.
- Every mutation is attributable to a worktree owner.
- Worktree cleanup is safe and recoverable.
- Shared resources remain scheduler-controlled even with isolated worktrees.

### Required tests

- concurrent creation/removal;
- branch collision;
- Git lock contention;
- repository relocation;
- orphan recovery;
- dirty/untracked removal refusal;
- handoff and merge conflict reporting;
- nested agent worktree policy;
- build-cache exclusivity;
- Windows/macOS/Linux path behavior.

## Phase 11 — Audit foundation

### Objective

Make team activity reconstructable and structurally attributable before adding chat-triggered actions and distributed execution.

### Deliverables

- Add append-only audit events with actor, source node, project/workspace/worktree/session/turn/agent/job scope, action, decision, correlation, causation, visibility, metadata, and optional digest.
- Instrument authentication, authorization, membership, asset refresh, prompts, provider/model selection, delegation, permission decisions, tools, commands, files, Git, worktrees, jobs, and configuration.
- Add content-retention policy independent of structural metadata.
- Add query, pagination, filtering, and export APIs.
- Add redaction guarantees and secret scanners at the audit boundary.
- Add administrator/project-owner visibility policy.

### Dependencies

Phases 6, 9, and 10 for complete identity and activity linkage. Basic seams SHOULD be added earlier.

### Exit criteria

- A project owner can trace an operation to a principal/service/schedule and agent/job chain.
- Sensitive bodies can expire without destroying structural evidence.
- Asset generation and skill/agent digests make turn behavior reproducible.
- Audit writes do not block critical execution indefinitely.

### Required tests

- attribution chains;
- retention expiry;
- secret redaction;
- pagination and ordering;
- authorization for audit readers;
- storage failure and backpressure;
- duplicate event idempotency;
- export integrity;
- high-volume bounded performance.

## Phase 12 — Project communication

### Objective

Add project-scoped human/agent communication integrated with observation but separate from audit and command execution.

### Deliverables

- Add channels, messages, replies/threads, mentions, edits/redactions, typing state, read markers, retention, and references.
- Define a public namespaced JSON event schema.
- Add human and agent authorship.
- Add the TUI project-chat panel and chat tab.
- Integrate observer mode so insert-mode input targets project chat.
- Add structured chat actions for agent task launch, review request, or job reference.
- Require separate authorization for every structured action.
- Link chat actions to audit and resulting tasks/runs.
- Define bridge interfaces without adopting an external chat system as source of truth.

### Dependencies

Phases 6, 8, 9, and 11.

### Exit criteria

- Project members can communicate while observing active work.
- Messages reference sessions, agent runs, jobs, commits, diffs, and artifacts.
- Free text never silently executes privileged work.
- Chat edits/redactions do not rewrite audit history.

### Required tests

- message ordering and idempotency;
- authorization and private project isolation;
- typing/read-marker expiry;
- edit/redaction semantics;
- chat-triggered structured task flow;
- agent-authored message attribution;
- retention and export;
- observer-panel input routing.

## Phase 13 — ACP adapter

### Objective

Expose CodeGG sessions through the Agent Client Protocol while preserving native control-plane ownership.

### Deliverables

- Implement ACP initialization and capability negotiation.
- Map ACP sessions to CodeGG projects, sessions, turns, tools, permissions, diffs, terminals, and events.
- Support session creation/loading, prompt submission, streaming updates, cancellation, and closure.
- Add project selection/configuration outside or through compatible ACP extension seams.
- Ensure ACP clients share durable sessions with TUI clients.
- Add Zed and at least one VS Code-compatible integration test path.
- Document unsupported CodeGG administrative operations as native-protocol concerns.

### Dependencies

Phases 4, 5, 6, and 9. Observation/chat are not required for the first ACP milestone.

### Exit criteria

- Zed and one VS Code ACP client can use CodeGG reliably.
- ACP-created work is scheduler-, authorization-, workspace-, agent-, and audit-owned.
- Protocol disconnect/reconnect and cancellation behave consistently with TUI sessions.

### Required tests

- ACP version/capability negotiation;
- create/load/resume;
- streaming and tool events;
- permission flow;
- cancellation;
- project isolation;
- simultaneous ACP and TUI attachment;
- unsupported operation errors;
- compatibility against current ACP conformance fixtures.

## Phase 14 — Remote workspace and SSH execution abstraction

### Objective

Support projects whose workspace and tools reside on another machine before introducing full leaf-daemon federation.

### Deliverables

- Add workspace locators and execution targets.
- Implement local and SSH targets behind a common backend.
- Add SSH host registration, strict host-key verification, credential references, connection pooling, process cancellation, and remote path canonicalization.
- Add project-relative remote file operations and artifact transfer.
- Bind remote LSP/Git/tool execution to the target rather than coordinator-local paths.
- Add remote health and capability reporting.

### Dependencies

Phases 0, 3, 5, 6, and 11.

### Exit criteria

- A session can bind to an SSH workspace.
- Tools execute on the intended host.
- Remote paths are never interpreted as local paths.
- Cancellation and disconnect produce deterministic attempt state.

### Required tests

- host-key mismatch;
- credential failure/rotation;
- remote path escape;
- connection loss;
- process-tree cancellation;
- artifact size limits;
- Git/LSP/tool locality;
- audit attribution;
- Windows-to-Linux and Linux-to-Linux cases where supported.

## Phase 15 — Coordinator/leaf node enrollment and link

### Objective

Turn local daemons into authenticated execution nodes connected to one authoritative coordinator.

### Deliverables

- Add node records, enrollment tokens, node-generated keys, certificates, rotation, and revocation.
- Add mutual-TLS persistent link over the native protocol.
- Add protocol/capability negotiation, coordinator generation, project assignments, node inventory, and health.
- Add durable node outbox, source sequence, acknowledgement watermark, replay, and idempotent acceptance.
- Multiplex control, presence, session, agent, job, artifact, file-change, chat, audit, and heartbeat streams.
- Add bounded reconnect and backoff policy.
- Add workspace-specific asset manifest publication and coordinator asset generations.
- Expose asset divergence across nodes/worktrees.

### Dependencies

Phases 1, 5, 6, 7, 11, and preferably 14.

### Exit criteria

- A workstation daemon can enroll, authenticate, disconnect, and reconnect without losing accepted events.
- The coordinator sees node health, local sessions, workspaces, assets, and capacity only for authorized projects.
- Revoked nodes cannot reconnect or receive new work.
- Duplicate replay does not duplicate sessions, messages, jobs, or audit records.

### Required tests

- enrollment replay and expiration;
- certificate rotation/revocation;
- coordinator restart/generation change;
- leaf durable outbox crash recovery;
- sequence gaps and resync;
- unauthorized project subscription;
- payload and stream backpressure;
- asset-manifest divergence;
- long disconnection and reconnect.

## Phase 16 — Cross-node scheduling and local execution

### Objective

Let the coordinator place work on suitable linked nodes while tools and heavy commands remain local to those machines.

### Deliverables

- Extend scheduler resource inventory with node CPU, memory, disk, accelerators, toolchains, project workspaces, and current load.
- Add node selection based on capability, locality, policy, fairness, and load.
- Add execution leases, fencing tokens, renewal, expiration, and reassignment.
- Dispatch jobs and agent runs to leaf executors.
- Stream bounded progress and retain large output as remote artifacts.
- Propagate cancellation and process-tree termination.
- Add disconnected-execution policy for already leased work.
- Add per-node drain and maintenance modes.

### Dependencies

Phases 9, 10, and 15.

### Exit criteria

- Jobs execute on suitable nodes and appear as ordinary CodeGG attempts.
- Local filesystem and build tools remain on the selected node.
- Stale nodes cannot complete expired mutating leases.
- Global fairness spans projects, principals, agent trees, and nodes.

### Required tests

- capability matching;
- locality preference;
- node overload and failover;
- lease expiry/fencing;
- coordinator/leaf partition;
- cancellation during partition;
- duplicate completion;
- artifact transfer;
- worktree and asset-generation binding;
- multi-node contention stress.

## Phase 17 — Code and metadata synchronization

### Objective

Provide real-time awareness and explicit code handoff across nodes without attempting transparent collaborative filesystem replication.

### Deliverables

- Stream file-opened, file-changed, bounded diff, Git status, branch, commit, worktree, and agent-edit metadata.
- Add authorized on-demand file snapshots and content-addressed blobs.
- Add explicit checkpoints and review artifacts.
- Add Git ref, patch series, bundle/pack, or remote-based handoff.
- Add conflict and divergence notifications.
- Add optional coordinator bare-mirror seam.
- Ensure uncommitted changes are never silently merged elsewhere.

### Dependencies

Phases 10, 11, 15, and 16.

### Exit criteria

- Observers can inspect authorized current edits from another node.
- Completed work can be handed off through explicit Git or patch objects.
- Workspace divergence is visible and attributable.
- CodeGG remains usable with existing Git hosting.

### Required tests

- diff bounds and redaction;
- blob digest verification;
- interrupted transfer/resume;
- conflicting branches;
- dirty workspace handoff refusal;
- unauthorized content request;
- mirror optionality;
- multi-node asset/code revision mismatch.

## Phase 18 — External CI runner adapters

### Objective

Use external CI platforms as execution backends without recreating their workflow systems.

### Deliverables

- Define a generic external execution backend interface.
- Implement one reference adapter, such as GitHub Actions or GitLab CI.
- Map submission, status, cancellation, artifacts, logs, and completion into CodeGG jobs and attempts.
- Add external credential references and project authorization.
- Link external runs to worktree/commit provenance and audit.
- Document non-goal: CodeGG does not define or replace complete CI workflow syntax.

### Dependencies

Phases 2, 6, 11, and scheduler baseline. Cross-node work is not strictly required but shares abstractions.

### Exit criteria

- A CodeGG job can execute through one external CI platform.
- Result, logs, and artifacts return through normal CodeGG records.
- Cancellation and duplicate submission are deterministic.

### Required tests

- API authentication/rotation;
- idempotent submission;
- status polling or webhook duplication;
- cancellation races;
- artifact bounds;
- missing/expired run;
- project authorization;
- audit linkage.

## Phase 19 — Operational hardening and scale closure

### Objective

Make the completed architecture operable for professional teams and recoverable under failure.

### Deliverables

- Per-principal, per-project, per-provider, per-agent-tree, and per-node quotas.
- Node draining, maintenance, and revocation workflows.
- Credential and certificate rotation.
- Backup/restore and warm-standby procedures.
- Schema migration testing from all supported releases.
- Event-log compaction and retention enforcement.
- Audit integrity and optional hash chaining.
- Denial-of-service controls for subscriptions, chat, artifacts, skills, agents, prompts, and node streams.
- Protocol compatibility matrix and upgrade order for coordinator, leaves, TUIs, and ACP clients.
- Observability export, alerts, health checks, and administrative recovery commands.
- Large-team and multi-project load testing.
- Optional server-database backend evaluation while retaining SQLite personal mode.

### Dependencies

All foundational phases. Hardening work MUST also be performed incrementally rather than deferred entirely to this phase.

### Exit criteria

- Documented backup, restore, upgrade, downgrade/rollback, node replacement, credential compromise, and coordinator-loss procedures.
- Sustained multi-user/multi-project/multi-node load remains bounded and fair.
- Security defaults distinguish local development from networked team operation safely.
- Protocol version skew fails predictably.
- The project can make a release-candidate claim against the long-term specification.

### Required tests

- chaos and restart testing;
- storage corruption and partial migration;
- coordinator/leaf version skew;
- quota abuse;
- subscription and artifact floods;
- credential compromise/revocation;
- backup restoration consistency;
- multi-day soak tests;
- high-contention scheduler and worktree tests;
- audit and retention verification.

## Phase 20 — Optional ecosystem bridges

### Objective

Add integrations that are useful after the canonical CodeGG models are stable.

### Candidate deliverables

- Matrix, XMPP, IRCv3, Slack, or Teams project-channel bridges;
- optional NATS transport for large installations;
- optional central Git mirror;
- web administration frontend;
- enterprise identity-provider adapters;
- specialized execution-node autoscaling.

### Dependencies

Canonical project, authorization, chat, audit, node, and protocol models.

### Exit criteria

Each bridge remains an adapter. It MUST NOT become the canonical owner of CodeGG project identity, authorization, agent runs, jobs, worktrees, chat semantics, or audit state.

## Recommended immediate execution sequence

The first implementation tranche SHOULD be:

```text
Phase 0  canonical IDs and domain separation
Phase 1  runtime asset interoperability and refresh correctness
Phase 2  Eggpool and daemon-owned provider connections
Phase 3  project catalog and lazy discovery
Phase 4  multi-project/multi-session TUI
Phase 5  frontend-neutral session projection
```

This sequence gives immediate solo-user value, prevents stale skills/agents in a long-running daemon, enables Eggpool for small teams, and establishes project/session boundaries before team authorization or distributed execution depend on them.

The next collaboration tranche SHOULD be:

```text
Phase 6  principals and project authorization
Phase 7  presence
Phase 8  read-only observation
Phase 9  durable multilevel agent trees
Phase 10 worktree-native concurrency
Phase 11 audit
Phase 12 project communication
```

The interoperability and distribution tranche SHOULD then be:

```text
Phase 13 ACP
Phase 14 SSH execution targets
Phase 15 coordinator/leaf links
Phase 16 cross-node scheduling
Phase 17 code/metadata synchronization
Phase 18 external CI runners
Phase 19 operational hardening
```

## Roadmap governance

Implementation plans derived from this roadmap SHOULD cite the exact phase and specification sections they satisfy. A phase is not complete because code exists; it is complete only when its ownership model, migrations, protocol, tests, documentation, failure semantics, and closure evidence are present.

When implementation reveals that a term or ownership boundary is wrong, update the terminology document and long-term specification first, then adjust the roadmap. Avoid accumulating incompatible local meanings in phase plans.

New scope SHOULD be evaluated against the non-goals in the specification. Features that do not strengthen solo development, professional team coordination, multi-agent correctness, project interoperability, or distributed execution SHOULD not displace the roadmap's core work.