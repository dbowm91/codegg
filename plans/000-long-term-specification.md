# CodeGG Long-Term Architecture and Product Specification

Status: canonical long-term implementation directive

Companion documents:

- `plans/001-terminology-and-domain-model.md`
- `plans/002-long-term-roadmap.md`

This document defines the intended end state for CodeGG. It is deliberately broader than an implementation plan: it establishes product scope, domain boundaries, architectural ownership, protocol expectations, security properties, interoperability requirements, and acceptance criteria. The roadmap decomposes this specification into ordered execution phases. The terminology document is normative whenever older CodeGG code or documentation uses overlapping terms such as project, workspace, directory, session, task, run, or agent.

The keywords MUST, MUST NOT, REQUIRED, SHOULD, SHOULD NOT, and MAY are normative.

## 1. Product definition

CodeGG is a daemon-centered software-development orchestration system for individual developers and professional engineering teams. A CodeGG deployment coordinates human sessions, coding agents, nested subagents, Git worktrees, local and remote execution, shared build resources, provider access, project communication, and auditable development activity.

The TUI remains the reference frontend and receives first-class development priority. ACP is the standard editor-to-agent interoperability boundary for Zed, VS Code, and other compatible clients. CodeGG's native protocol remains the broader control-plane protocol because project administration, team authorization, scheduling, node coordination, worktree lifecycle, provider administration, project communication, and audit are outside ACP's editor-agent scope.

The same architecture MUST support three deployment forms without creating separate products:

```text
Personal local
    TUI -- local IPC --> CodeGG daemon
                         |-- coordination
                         `-- local execution

Team single-host
    multiple TUIs / ACP clients
                |
                v
    authenticated CodeGG daemon
        |-- project coordination
        |-- local execution
        |-- shared scheduler
        |-- collaboration
        `-- audit

Team distributed
    workstation leaf daemon --+
    workstation leaf daemon --+--> authoritative coordinator daemon
    build-server leaf daemon --+
```

In personal-local mode, coordinator and execution-node responsibilities collapse into one efficient process and one implicit local owner. In team deployments, those responsibilities MAY be separated across machines while preserving the same project, session, job, agent-run, worktree, and protocol models.

## 2. Primary product goals

CodeGG MUST provide:

1. A fast, low-friction solo-development experience with no mandatory login, team configuration, or network service.
2. A single user-scoped daemon capable of serving several TUI or ACP clients and several simultaneous projects.
3. A professional team mode with individual identities, project-scoped authorization, real-time presence, read-only observation, project communication, and auditability.
4. Native multi-session and multi-project TUI operation.
5. Scheduler-governed human, agent, subagent, test, build, and external-runner work across projects.
6. Worktree-native concurrency for several developers and agents operating on one repository.
7. A durable multilevel agent-run tree with bounded delegation, cancellation, recovery, attribution, and resource budgets.
8. Local and remote workspace support, including SSH-backed execution targets.
9. A distributed coordinator/leaf topology in which work stays local to execution nodes while project metadata, activity, chat, and audit flow through the coordinator.
10. ACP support without weakening or replacing the native CodeGG control-plane protocol.
11. Daemon-owned provider connections, beginning with explicit Eggpool support through `/connect`.
12. Good coexistence with other coding harnesses operating in the same repository, especially portable Agent Skills discovery and deterministic refresh behavior.

## 3. Non-goals

CodeGG is not initially:

- a general-purpose chat application;
- a federated social network;
- a Git hosting platform;
- a replacement for GitHub Actions, GitLab CI, or other CI definition systems;
- a general distributed workflow engine;
- a remote-desktop or terminal-screen-sharing system;
- a CRDT collaborative source editor;
- a multi-primary control-plane database;
- a Kubernetes replacement;
- an enterprise identity provider.

CodeGG MAY integrate with those systems where they support software-development orchestration. It MUST NOT absorb their complete product scope.

## 4. Architectural principles

### 4.1 One architecture, progressively enabled

Personal and team modes MUST share the same internal principal, project, authorization, session, agent-run, job, worktree, provider, and event abstractions. Personal-local mode resolves the operating-system owner to an implicit `LocalOwner` principal with broad local capabilities. It MUST NOT maintain a separate unauthenticated execution implementation.

### 4.2 Explicit ownership

Every mutable record and execution path MUST have one canonical owner or a deterministic reconciliation rule. Coordinator-owned state, execution-node-owned state, replicated projections, and compatibility paths MUST be documented.

### 4.3 Paths are locators, not identity

A filesystem path MUST NOT be the stable identity of a project, repository, workspace, worktree, session, or execution node. Paths MAY change without changing durable identifiers.

### 4.4 Frontends render projections

TUIs, ACP clients, web clients, and future frontends MUST render frontend-neutral snapshots and events. They MUST NOT become independent owners of provider credentials, project authorization, global scheduling, or durable session state.

### 4.5 Locality by default

Filesystem access, LSP processes, Git worktree operations, PTYs, tests, builds, and other heavy tools SHOULD execute on the node that owns the workspace. The coordinator SHOULD receive structured events, bounded output, artifacts, and content-addressed data rather than proxying every local operation synchronously.

### 4.6 Progressive disclosure

Large skill bodies, tool artifacts, logs, files, and audit content SHOULD be represented by metadata and handles until explicitly requested. Catalog and presence views MUST remain bounded.

### 4.7 Correctness before transparent magic

CodeGG MUST prefer explicit worktree ownership, explicit handoff, explicit authorization, explicit conflict diagnostics, and explicit node leases over implicit synchronization or silent merging.

## 5. Canonical deployment model

A deployment contains:

```text
Deployment
|-- coordinator identity and generation
|-- principals and service identities
|-- execution nodes
|-- project catalog
|-- provider connections
|-- global policy and scheduler
|-- sessions and agent-run trees
|-- jobs, attempts, schedules, and artifacts
|-- project channels
`-- audit log
```

The coordinator is authoritative for deployment identity, project identity, human and service identities, node enrollment and revocation, project authorization, session registry, agent-run metadata, global scheduling, provider-connection metadata, project communication, and audit sequence assignment.

An execution node is authoritative for local process trees, local workspace and worktree paths, local filesystem state, local Git operations, PTYs, LSP servers, build caches, test processes, machine resource inventory, and uncommitted editor state.

The coordinator MAY also be an execution node.

High-availability multi-coordinator consensus is outside the initial scope. Backup, restore, schema migration, and warm-standby operation MUST precede any multi-primary or consensus design.

## 6. Canonical identity relationships

The target ownership chain is:

```text
Principal
  -> ClientConnection
  -> Session
  -> Project
  -> Repository
  -> Workspace
  -> Node

Session
  -> Turn
  -> root AgentRun
  -> descendant AgentRuns
  -> Jobs / Runs / Artifacts

Workspace
  -> optional Worktree
  -> canonical root on one Node
```

A session MUST bind to exactly one project. A session MUST identify the workspace used for each active turn. A project MAY have many workspaces on one or many nodes. A repository MAY have many worktrees. A human MAY have several sessions in one project. Several humans MAY have several sessions in the same project.

The detailed definitions and compatibility mappings are normative in `plans/001-terminology-and-domain-model.md`.

## 7. Current foundation and required evolution

The existing codebase already contains a singleton daemon, socket/in-process/stdio transports, versioned request and event envelopes, active-session and connected-client snapshots, workspace registration and immutable execution contexts, durable jobs and attempts, a global admission scheduler, structured turn/tool/subagent/run events, remote TUI replay, native Git abstractions, and execution-ownership guards.

The primary evolution is not another daemon rewrite. It is the introduction of explicit project identity, principal identity, worktree identity, execution-node identity, durable agent-run hierarchy, project-scoped subscriptions, and coordinator/leaf ownership around the existing daemon and scheduler.

Compatibility fields such as path-derived `project_id`, `directory`, or process-global working-directory assumptions MUST be progressively retired from daemon-owned paths.

## 8. Deployment profiles and authentication

### 8.1 Personal-local

The daemon listens only on user-owned local IPC. Operating-system peer credentials, socket ownership, and process ownership resolve the caller as `Principal::LocalOwner`.

There is no login screen, project-membership administration, bearer-token ceremony, or mandatory HTTP server. Authorization still runs internally and receives an explicit principal.

### 8.2 Personal-remote

One owner connects remotely through SSH forwarding, mutual TLS, or a strong personal access token. The identity model remains one human owner, but all network access is authenticated.

### 8.3 Team single-host

Every network connection has an authenticated human, service-account, or node principal. Project authorization is enforced in the daemon. Several developers and agents use isolated worktrees and share one scheduler and build system.

### 8.4 Team distributed

Execution-node daemons enroll with the coordinator and maintain authenticated persistent links. Each node operates locally but publishes authorized project, session, agent, job, Git, and chat events to the coordinator.

### 8.5 Human authentication

The implementation SHOULD progress from administrator-created users and personal tokens to OIDC/device login and optional SSH-key-backed terminal login. Local Unix-socket clients MAY use OS peer credentials.

### 8.6 Node authentication

Nodes MUST have identities separate from human identities. Enrollment SHOULD use a short-lived token, node-generated private key, coordinator-issued certificate, mutual TLS, rotation, and revocation. A valid node identity permits a link but does not automatically grant every project.

### 8.7 Project authorization

The initial roles SHOULD be `Viewer`, `Contributor`, `Maintainer`, and `Owner`, expanded internally into capabilities such as:

```text
project.read           project.observe
project.chat           session.create
session.read           session.observe
agent.invoke           agent.delegate
file.read              file.modify
command.execute        job.submit
job.cancel             git.read
git.write              worktree.create
worktree.remove        project.configure
member.manage          audit.read
node.target
```

Authorization MUST occur at the daemon operation boundary. The effective authority of an agent operation is the intersection of the human or service principal, project policy, session policy, parent-agent delegation, agent definition, tool policy, workspace policy, and execution-node policy.

A child agent MUST be able to narrow authority and MUST NOT widen it.

## 9. Project, repository, workspace, and worktree model

A project is the durable authorization, collaboration, audit, and scheduling boundary. A repository is version-control lineage associated with the project. A workspace is one concrete checkout on one node. A worktree is an isolated Git checkout with explicit lifecycle and ownership.

The existing stable `WorkspaceId` and immutable `ExecutionContext` SHOULD be retained, but workspace MUST no longer stand in for the logical project.

A durable worktree record MUST include project, repository, workspace, node, path, branch, base commit, owner, lifecycle, creation time, and last-seen time. Worktree owners include human sessions, agent runs, jobs, and explicitly shared project workspaces.

Parallel mutation-capable agent runs SHOULD receive independent worktrees by default. Read-only descendants MAY share a parent workspace under a read lease. Serialized mutations MAY share a worktree only under explicit scheduler ownership.

The scheduler MUST support repository and workspace contention keys, including Git writes, workspace exclusivity, worktree mutation, build-cache writes, integration-test resources, project databases, and other shared services.

## 10. Project catalog and discovery

The daemon MUST support:

- configured discovery roots such as `~/projects`;
- bounded traversal depth;
- Git-repository or directory discovery modes;
- additional top-level roots;
- explicit one-off projects;
- local and remote project locators;
- lazy project activation.

Discovery MUST populate cheap catalog metadata only. It MUST NOT eagerly initialize LSP servers, Git watchers, configuration watchers, build caches, or provider sessions for every directory.

Repository identity SHOULD use stable remote and object metadata when available and MUST NOT be based solely on path text.

## 11. Daemon-owned provider connections and Eggpool

Eggpool MUST be added as an explicit `/connect` provider option. The default port is `11300`. The user supplies host, optional port, TLS policy, API key, optional display name, and connection scope.

A provider connection MUST be daemon-owned and represented by a stable identifier, endpoint, secret reference, scope, owner, capabilities, health, and discovered model catalog. API keys MUST be stored through the secret subsystem and MUST NOT appear in project configuration, protocol events, logs, chat, or audit metadata.

Connection scopes SHOULD support personal, project, and deployment use. Several sessions and frontends MUST be able to reference one shared connection without duplicating credentials.

The Eggpool connection workflow MUST normalize the endpoint, validate authentication, probe a bounded health or model endpoint, discover models where supported, retain actionable diagnostics, and allow independent credential rotation.

Provider connections MUST remain an abstraction. Eggpool SHOULD be implemented as a preset or provider kind over the most appropriate compatible transport rather than hard-coding Eggpool assumptions throughout session runtime.

## 12. Repository asset and harness interoperability

### 12.1 Scope

CodeGG MUST coexist cleanly with other coding harnesses operating in the same repository. It MUST read compatible shared assets without rewriting, deleting, moving, or taking ownership of another harness's directories.

The first interoperability target is the portable Agent Skills format: one skill directory containing `SKILL.md` with YAML frontmatter and optional scripts, references, assets, and other resources. CodeGG SHOULD follow progressive disclosure: load metadata for discovery, load the full `SKILL.md` on activation, and load bundled resources only when requested.

Agent-definition formats are less standardized than skills. CodeGG MUST support its native agent files and MAY add explicit adapters for other harnesses. It MUST NOT silently reinterpret an incompatible foreign agent format as a fully trusted CodeGG agent.

### 12.2 Skill discovery locations

Project-local discovery MUST support, at minimum:

```text
.codegg/skills/<name>/SKILL.md
.agents/skills/<name>/SKILL.md
.opencode/skills/<name>/SKILL.md
.claude/skills/<name>/SKILL.md
```

Global discovery SHOULD support:

```text
<platform config>/codegg/skills/<name>/SKILL.md
~/.agents/skills/<name>/SKILL.md
~/.config/opencode/skills/<name>/SKILL.md
~/.claude/skills/<name>/SKILL.md
```

CodeGG MAY continue accepting direct Markdown files in its native `.codegg/skills` directory for backward compatibility, but portable directories containing `SKILL.md` SHOULD be the preferred form.

For nested working directories and Git worktrees, discovery SHOULD walk from the effective workspace directory toward the Git worktree root, collecting supported project-local skill roots deterministically. It MUST NOT traverse above the declared project or worktree boundary without explicit configuration.

### 12.3 Skill schema compatibility

CodeGG SHOULD accept the portable frontmatter fields:

```text
name             required
description      required
license          optional
compatibility    optional
metadata         optional map
allowed-tools    optional and experimental
```

CodeGG-native extensions MAY be namespaced. Unknown fields SHOULD be preserved in provenance metadata or ignored with diagnostics rather than causing unnecessary incompatibility.

Skill names and descriptions MUST be validated. Bundled scripts MUST NOT execute merely because a skill is discovered or activated. Script and tool execution remains subject to ordinary tool permission, sandbox, authorization, and audit rules.

### 12.4 Deterministic precedence and conflicts

Discovery MUST produce a source-aware asset registry. Every skill and agent definition MUST retain source path, source kind, content digest, modification fingerprint, validation diagnostics, and effective precedence.

Default precedence SHOULD be:

```text
explicit session or project configuration
project .codegg native asset
project .agents portable asset
project .opencode compatible asset
project .claude compatible asset
global CodeGG native asset
global .agents portable asset
global OpenCode-compatible asset
global Claude-compatible asset
compiled CodeGG default, when applicable
```

The exact order MAY be configurable, but it MUST be deterministic and inspectable. A higher-precedence duplicate MAY shadow a lower-precedence asset. Shadowing MUST emit a diagnostic and preserve provenance. Invalid higher-precedence content SHOULD NOT silently erase a valid lower-precedence asset unless policy explicitly requires fail-closed behavior.

The TUI and protocol SHOULD expose commands or views that answer:

```text
which assets were discovered
which source won
which sources were shadowed
which files were invalid
which digest/version is active
```

### 12.5 Refresh and snapshot semantics

A long-running daemon MUST NOT treat agents, skills, project instructions, or related runtime assets as immutable startup state.

The daemon MUST maintain a versioned `ProjectAssetSnapshot` containing effective agents, skills, project instructions, source provenance, content digests, diagnostics, and a monotonically increasing project asset generation.

A refresh MUST occur:

1. when a project is first activated;
2. whenever a new session is created for the project;
3. whenever an existing session is opened or attached after not being active in the current frontend;
4. before a session is rebound to another workspace;
5. after an explicit manual refresh command;
6. after a coordinator accepts a newer asset manifest from an execution node.

File watching MAY provide faster updates, but correctness MUST NOT depend on watchers. Session-open refresh is the correctness baseline.

Manual commands MUST include a unified operation and focused aliases, for example:

```text
/reload
/reload project
/reload skills
/reload agents
/skills refresh
/agents refresh
```

The final command names MAY differ, but there MUST be one discoverable manual command that refreshes all project runtime assets and returns a structured report of added, removed, changed, shadowed, and invalid entries.

Refresh MUST be transactional. The daemon builds and validates a candidate snapshot, then atomically swaps it into the project service bundle. A failed refresh MUST leave the previous valid snapshot active and return diagnostics.

An active turn MUST retain the runtime asset snapshot captured at turn start. Refresh MUST NOT mutate the prompt, agent definition, permissions, or skill catalog of an in-flight turn. Subsequent turns SHOULD use the newest successful snapshot. Sessions SHOULD display when their next turn will use a newer generation.

New sessions MUST record the asset generation from which they were initialized. Agent runs MUST record the effective agent-definition digest and activated skill digests for audit and reproducibility.

### 12.6 Distributed asset discovery

For a workspace on a leaf node, discovery MUST occur on that node because the leaf owns the actual filesystem. The leaf publishes a bounded manifest and approved asset content or content-addressed blobs to the coordinator. The coordinator records node, workspace, project, source paths, digests, diagnostics, and generation.

Different nodes may temporarily contain different worktree revisions and therefore different project assets. CodeGG MUST expose this divergence rather than pretending the project has one universal asset snapshot. Sessions bind to a workspace-specific asset snapshot. Team policy MAY require assets to originate from committed Git content or a designated canonical branch before they are shared deployment-wide.

## 13. Multi-project and multi-session TUI

The TUI MUST become a projection of daemon state rather than a single privileged current directory.

Normal-mode `Space f` SHOULD open a project picker following Helix conventions. Opening a project creates or focuses a project tab. Users MUST be able to switch project tabs and select among several sessions within a project.

A project tab SHOULD contain project summary, workspace/worktree state, selected session, session list, activity projection, agent tree, Git status, project jobs, collaborator presence, and project chat.

Opening a project or session MUST trigger the asset-refresh semantics defined above before constructing a new turn runtime.

## 14. Presence and real-time team awareness

Presence is project-scoped ephemeral state. It MUST be distinct from durable session history and audit records.

The coordinator SHOULD represent principal presence, session activity, and agent activity separately. Human presence includes active, idle, and disconnected. Session activity includes viewing, composing, agent running, tool running, build running, awaiting permission, awaiting question, awaiting human, paused, and disconnected. Agent activity includes queued, thinking, delegating, reading, editing, running tool, building, testing, waiting for child, waiting for human, blocked, completed, and failed.

Presence uses bounded leases renewed by heartbeats or meaningful activity. Authorized project members SHOULD see human identifiers, session counts, agent counts, high-level status, and node location where policy permits.

Presence visibility MUST be project-authorized and MAY be disabled or reduced by deployment or project policy.

## 15. Read-only session observation

CodeGG MUST support an authorized read-only observation subscription to another human's session.

Observation MUST render the same logical activity panel from frontend-neutral state. It MUST NOT mirror raw terminal screen frames. This permits terminal-size independence, replay, redaction, ACP compatibility, web clients, and selective disclosure.

An observer MAY receive human prompts, agent output, explicit progress summaries, tool names and redacted arguments, command/test progress, changed-file and diff summaries, nested agent tree, worktree/branch state, jobs, and pending permissions/questions. The observer MUST NOT answer another session's permissions, steer the agent, cancel work, or mutate its workspace without a separate authorized operation.

Provider-private hidden reasoning is not an observable product feature. CodeGG MAY display model-supplied reasoning summaries or explicit progress events, but MUST NOT claim to expose hidden chain-of-thought.

When the TUI is observing another session, the main activity panel is read-only. Insert-mode text input SHOULD target the project-chat side panel. Steering another human's agent MUST require an explicit command or control-transfer workflow.

Each event MUST have a visibility classification such as project, session participants, actor only, administrators, or secret-redacted. Secrets and environment credentials MUST never be transported in observer events.

## 16. Durable multilevel agent-run hierarchy

The current first-level subagent pool MUST evolve into a scheduler-integrated durable agent-run service.

Every agent run MUST record root run, parent run, originating principal/session/turn, project, workspace, optional worktree, execution node, agent definition and digest, model/provider connection, depth, status, delegated authority, resource budget, timestamps, correlation, and causation.

Limits MUST exist for maximum tree depth, direct children, active descendants per root, active runs per session/project/principal/node, model-call concurrency, tool concurrency, build concurrency, token budget, tool-call budget, and wall-clock budget.

The task/delegation tool MUST no longer deny all descendants unconditionally. A child may delegate only when the parent may delegate, the child definition permits delegation, depth and fan-out limits permit it, and the root budget remains available.

Agent creation MUST be idempotent across model retries and frontend retransmission. A delegation identity SHOULD incorporate session, turn, parent run, tool-call identity, and delegation ordinal.

Cancellation MUST propagate downward by default through descendant agent runs, jobs, process groups, worktrees, and scheduler permits. Selective child cancellation MAY be supported.

Parent runs MUST have explicit join semantics such as all, any successful, first completed, or detached. Detached work remains causally linked and auditable.

Parallel mutation-capable descendants MUST use independent worktrees unless an explicit serialized policy permits sharing.

## 17. Job scheduling and execution backends

The existing global scheduler remains the sole daemon admission authority for heavy and durable work. Human sessions, root agents, descendant agents, tests, builds, scripts, remote-node work, and external CI work MUST converge on scheduler-owned submissions rather than creating parallel admission systems.

A generic execution backend SHOULD support local process, SSH, linked leaf node, GitHub Actions, GitLab CI, and future self-hosted runner implementations.

External execution remains a normal CodeGG job and attempt. The scheduler decides when and where to submit, records external identifiers, observes progress, propagates cancellation, collects artifacts, and maintains attribution.

Per-project and per-principal fairness MUST complement machine-resource admission. A single agent tree or developer MUST NOT monopolize a shared deployment unless policy explicitly permits it.

## 18. Remote projects and execution targets

A workspace locator MUST distinguish local and remote locations. An execution target MUST identify local node, linked node, SSH target, or external runner.

The daemon SHOULD initially use hardened OpenSSH subprocess integration behind an execution-target abstraction rather than requiring a native SSH stack. Host-key verification, credential references, bounded connection pools, remote cancellation, process-tree cleanup, and remote path canonicalization are REQUIRED.

Frontends MUST use project-relative file and artifact operations. They MUST NOT assume that a daemon-local path is directly accessible to the frontend machine.

## 19. Coordinator-to-leaf topology

A leaf daemon represents an individual workstation, build server, GPU host, validation machine, or other execution node. It authenticates to one authoritative coordinator and maintains a persistent link.

The link SHOULD initially extend the versioned native CodeGG protocol over mutually authenticated WebSocket/TLS. QUIC or another transport MAY be evaluated after profiling.

Link establishment MUST negotiate protocol version, capabilities, node identity, coordinator generation, project assignments, resource inventory, last acknowledged sequence, durable outbox replay, and lease reconciliation.

The connection SHOULD multiplex control, presence, project metadata, session activity, agent activity, job dispatch, job output, artifact transfer, file-change metadata, chat, audit, and heartbeat streams.

Node-to-coordinator events MUST use unique event IDs, node-local source sequence numbers, durable outbox storage, acknowledgement watermarks, and idempotent at-least-once acceptance. The coordinator assigns the global deployment sequence.

Mutating job leases MUST contain an epoch or fencing token. A stale or disconnected node MUST NOT publish an expired mutation as the current result.

A leaf MAY continue already-leased work during a temporary disconnection according to policy. It MUST NOT begin new privileged project operations from indefinitely cached authorization.

## 20. Code and metadata synchronization

Distributed CodeGG MUST begin with activity synchronization and explicit handoff, not transparent collaborative filesystem replication.

Leaf nodes SHOULD stream file-opened, file-changed, bounded diff, Git status, branch movement, commit, worktree, agent edit, and job metadata. Authorized observers MAY request exact current content through bounded snapshots or content-addressed blobs.

Git remains the durable code-transfer mechanism. Completed work SHOULD produce a commit and branch ref, patch series, Git bundle/pack, or explicit review artifact. Uncommitted changes MUST NOT be silently merged into another developer's workspace.

A central bare mirror MAY be added later. CodeGG MUST NOT become a Git hosting platform as a prerequisite for distributed operation.

## 21. Project communication

The collaboration layer is project communication, not general chat. It MUST support project channels, durable messages, message IDs, incremental synchronization, mentions, replies or threads, human and agent identities, typing/composing state, read markers, retention controls, and references to sessions, agent runs, jobs, commits, diffs, and artifacts.

CodeGG SHOULD define a small public, versioned, namespaced JSON event schema over the native protocol. Bridges MAY map project channels to Matrix, XMPP, IRCv3, Slack, Teams, or other systems. An external chat protocol MUST NOT become the canonical owner of CodeGG project, authorization, agent, job, or audit state.

A chat message that launches work MUST create a separate structured task submission, authorization decision, agent/job record, and audit linkage. Free text MUST NOT silently become an unaudited privileged command.

Chat records and audit records are separate. Chat MAY be edited, redacted, bridged, or retained selectively. Audit records are append-only structural evidence.

## 22. Audit architecture

The coordinator MUST own an append-only audit event log with global sequence, source node, actor, project/workspace/worktree/session/turn/agent/job correlation, action, authorization decision, visibility, causation, bounded metadata, and optional content digest.

Auditable events include authentication, authorization, membership changes, node enrollment, session creation/attachment, prompt submission, provider/model selection, agent delegation, permission decisions, tool invocation, command execution, file mutation, Git operations, worktree lifecycle, job submission/cancellation/completion, remote execution, chat-triggered actions, configuration changes, asset refresh, and audit export.

Prompts, command text, file content, and tool output MUST have configurable retention. Structural metadata and content digests MAY outlive content bodies. Credentials MUST never enter the audit payload.

Later hardening MAY hash-chain audit segments and sign leaf batches. This improves tamper evidence but does not make a developer-owned workstation trustworthy against its operating-system owner.

## 23. ACP boundary

ACP MUST be implemented as an adapter over the native CodeGG protocol.

ACP covers editor-agent session creation/loading, prompt submission, streaming output, tool presentation, permission requests, diffs, terminals, session metadata, and closure. It does not replace native project catalog, membership, providers, node administration, global scheduling, worktrees, project chat, audit, or coordinator/leaf operations.

ACP-specific code MUST NOT bypass scheduler, authorization, workspace, agent-run, or audit boundaries.

## 24. Protocol and storage requirements

All native protocol operations MUST be versioned and capability-negotiated. New clients MUST tolerate unknown optional fields and events where safe. Security-sensitive capability absence MUST fail closed.

Project-scoped subscriptions MUST support snapshots, monotonically ordered events, resume from acknowledged sequence, and explicit resynchronization when history is unavailable.

Large output MUST remain in run stores or artifact stores and be referenced by bounded handles. Event payloads, chat payloads, presence lists, scheduler snapshots, and observer projections MUST have explicit size bounds.

SQLite remains appropriate for personal-local and initial team deployments. Storage interfaces MUST preserve a future migration path to a server database without forcing personal users away from SQLite.

Schema migrations MUST be restart-safe, tested from supported historical versions, and coordinated with daemon generation and node protocol compatibility.

## 25. TUI target behavior

The TUI state hierarchy SHOULD include deployment connection, authenticated principal, project catalog, open project tabs, project presence, global jobs, provider connections, and notifications.

A project tab SHOULD include project summary, selected workspace/worktree, selected session, session list, activity projection, agent tree, Git state, jobs, project chat, and collaborators.

Recommended interaction concepts are:

```text
Space f   project picker
Enter     open/focus project tab
gt / gT   next/previous project tab
Space s   project session picker
Space p   collaborators/presence
Space o   observe selected session
Space c   focus project chat
Space j   project jobs
Space a   agent tree
```

Exact keys remain configurable. The conceptual hierarchy and read-only observation behavior are normative.

## 26. Reliability and recovery

Daemon restart MUST recover or deterministically interrupt sessions, jobs, attempts, schedules, agent runs, worktrees, node leases, and project asset snapshots according to documented policies.

A coordinator generation change MUST fence stale node leases. A leaf reconnect MUST replay accepted-but-unacknowledged events idempotently. Duplicate prompts, delegations, job submissions, chat actions, and asset manifests MUST be detectable by idempotency keys.

A failed project-asset refresh MUST preserve the prior valid snapshot. A failed node or worktree cleanup MUST leave a visible orphan record rather than silently losing ownership evidence.

## 27. Security requirements

Team network listeners MUST default to authenticated operation. Development-friendly anonymous server behavior MUST be explicitly restricted to local or opt-in development profiles.

Secrets MUST be stored by reference and redacted at every protocol, logging, chat, observer, artifact, and audit boundary.

Project authorization MUST be enforced server-side. Presence and observation MUST not leak project existence or activity to unauthorized principals.

Foreign skills and agent assets are untrusted content. Discovery and parsing MUST be bounded. Resource paths MUST remain within the skill root. Symlink traversal, oversized files, recursive reference explosions, and automatic script execution MUST be prevented.

Remote node and SSH operations MUST verify host or node identity. Node revocation MUST stop new work and invalidate future leases.

## 28. Observability

CodeGG SHOULD export structured metrics and traces for coordinator requests, scheduler admission, node links, agent-run trees, tool execution, jobs, provider calls, worktree operations, project asset refresh, chat delivery, and audit persistence.

CodeGG domain identifiers remain canonical. OpenTelemetry trace/span identifiers MAY provide cross-process correlation but MUST NOT replace project, session, agent-run, job, attempt, worktree, or node IDs.

## 29. System invariants

The implementation MUST preserve the following invariants:

1. Every durable shared operation has an attributable principal, service identity, schedule, or agent actor.
2. Every project-scoped operation carries a stable `ProjectId`.
3. Every daemon-owned filesystem or process operation carries a stable `WorkspaceId` and execution target.
4. Paths do not define project identity.
5. Personal-local mode requires no interactive authentication.
6. Networked team mode accepts no anonymous mutating operation.
7. Authorization is enforced in the daemon, not solely in frontends.
8. Child agents cannot gain capabilities absent from their parent.
9. Nested agent work is globally scheduler-governed.
10. Parallel mutation-capable agents do not share an unreserved worktree.
11. External CI and remote-node work remains represented as CodeGG jobs and attempts.
12. Presence is ephemeral; session history and audit are durable.
13. Chat and audit use separate records and retention policies.
14. Read-only observation cannot answer permissions, steer agents, or mutate workspaces.
15. Secrets never appear in project events, chat, observer projections, or audit metadata.
16. Leaf-to-coordinator delivery is idempotent and replayable.
17. Expired execution leases cannot publish current mutations.
18. ACP and coordinator/leaf protocols remain separate boundaries.
19. Skills and agent definitions have source provenance and deterministic precedence.
20. Every newly opened session refreshes project runtime assets before use.
21. Active turns remain pinned to an immutable runtime-asset snapshot.
22. Compatibility execution paths remain classified and progressively reduced.
23. Solo mode remains a composition of the same components used for teams.

## 30. Completion criteria

This long-term directive is complete when:

- a solo developer can use one local daemon with the same or lower friction than today;
- one TUI can operate several project tabs and sessions;
- several authenticated developers can work in one project with isolated worktrees;
- developers can see project presence and observe another authorized session read-only while chatting;
- root agents and multilevel descendants are durable, bounded, scheduler-governed, attributable, and recoverable;
- Eggpool and other provider connections are daemon-owned and shareable by scope;
- CodeGG discovers portable skills from CodeGG, `.agents`, OpenCode, and Claude-compatible locations with deterministic provenance;
- opening a session or invoking a manual reload cannot leave skills or custom agents indefinitely stale;
- remote and linked-node projects execute tools locally while the coordinator maintains authoritative project state;
- ACP clients and the TUI use the same sessions, agents, jobs, worktrees, permissions, and audit boundaries;
- project communication and audit are operational and distinct;
- the system has explicit quotas, recovery, migration, backup, revocation, and protocol-compatibility tests.

## 31. Research anchors

Implementation work should consult current primary specifications rather than copying assumptions from this document indefinitely:

- Agent Skills specification: `https://agentskills.io/specification`
- OpenCode skill discovery: `https://opencode.ai/docs/skills/`
- Agent Client Protocol: `https://agentclientprotocol.com/`
- W3C Trace Context: `https://www.w3.org/TR/trace-context/`
- OpenTelemetry specification: `https://opentelemetry.io/docs/specs/otel/`

These references inform interoperability. CodeGG's domain identities, authorization, scheduling, and control-plane ownership remain defined by this specification.