# CodeGG Canonical Terminology and Domain Model

Status: normative companion to `plans/000-long-term-specification.md`

This document defines the language CodeGG implementation plans, protocol types, storage schemas, architecture documents, tests, UI labels, and operator documentation MUST use. When current code uses a term differently, the compatibility mapping in this document describes the migration target.

## 1. Naming rules

1. A durable concept MUST have a typed identifier rather than a path-derived string.
2. A path is a locator and MUST NOT be treated as durable identity.
3. A logical project, a physical checkout, a Git worktree, a human conversation, an agent execution, and a scheduler job are distinct objects.
4. Terms MUST NOT be used as interchangeable shorthand when they cross authorization, scheduling, persistence, or node boundaries.
5. Compatibility fields MAY remain during migration but MUST be labeled as compatibility projections.

## 2. Top-level relationships

```text
Deployment
|-- Coordinator
|-- Principals
|-- ExecutionNodes
|-- Projects
|   |-- Repositories
|   |   `-- Workspaces / Worktrees
|   |-- Sessions
|   |   `-- Turns
|   |       `-- AgentRun trees
|   |-- Jobs / Attempts / Runs / Artifacts
|   |-- ProjectChannels
|   `-- AuditEvents
|-- ProviderConnections
`-- DeploymentPolicy
```

The principal runtime relationship is:

```text
Principal
  -> ClientConnection
  -> Session
  -> Project
  -> Workspace
  -> ExecutionNode

Session
  -> Turn
  -> root AgentRun
  -> descendant AgentRuns
  -> Jobs / Runs / Artifacts
```

## 3. Deployment terms

### Deployment

A complete CodeGG administrative and consistency domain governed by one authoritative coordinator.

A personal-local daemon is a deployment. A team installation with many leaf daemons is also one deployment.

A deployment owns identities, projects, node enrollment, provider connections, global policy, project communication, audit sequencing, and the global scheduler view.

Suggested identifier: `DeploymentId`.

### Coordinator

The authoritative control-plane service for a deployment.

The coordinator owns project identity, human and service identity, project authorization, global session metadata, agent-run metadata, job scheduling, provider metadata, project communication, and audit sequence assignment.

A coordinator MAY also execute work locally.

Do not use `server` as a synonym. A server is a transport or process role; coordinator is an ownership role.

### Coordinator generation

A monotonically changing identifier representing one authoritative coordinator lifetime or leadership generation.

Jobs, attempts, node leases, and recovery logic use the generation to detect stale ownership.

Suggested type: `CoordinatorGeneration`.

### Execution node

A machine-scoped CodeGG runtime that owns local workspaces, processes, Git operations, LSP servers, build caches, PTYs, and machine resources.

The local personal daemon is an execution node. A workstation leaf daemon and a shared build host are execution nodes.

Suggested identifier: `NodeId`.

### Leaf daemon

A daemon process acting as an execution node linked to a remote coordinator.

`Leaf daemon` describes topology. `Execution node` describes the durable domain object. A leaf process authenticates as a node principal and reports node state.

Do not call a leaf a worker unless referring specifically to an internal worker task. `Worker` is too narrow and conflicts with agent pools and CI workers.

### Node enrollment

The authenticated process by which a new execution node joins a deployment and receives a durable identity or certificate.

### Node lease

A bounded authorization and liveness grant issued by the coordinator to an execution node.

A node lease MAY authorize project visibility, workspace reporting, or job execution. Expired leases cannot authorize new mutations.

### Fencing token

A monotonically ordered token attached to mutating work so stale nodes or attempts cannot publish an expired result as current.

## 4. Identity terms

### Principal

An authenticated actor recognized by CodeGG.

Principal kinds include human, service account, execution node, and implicit local owner.

Suggested identifier: `PrincipalId`.

### Human principal

A person with an individual CodeGG identity. Human identifiers MUST be used for project presence, session attribution, authorization, chat, and audit.

### Service account

A non-human identity used by automation, CI integration, scheduled work, or administrative tooling.

### Node principal

The authenticated identity of an execution node. It is separate from human users of that node.

### Local owner

The implicit principal resolved from operating-system ownership in personal-local mode.

`LocalOwner` preserves the no-login solo workflow while allowing internal authorization and audit APIs to require a principal.

### Client connection

One frontend or API connection to a daemon or coordinator.

A client connection is ephemeral and has a `ClientId`, client type, protocol capabilities, authenticated principal, connection time, and attached sessions.

A client connection is not a session. One client may attach to several sessions; several clients may observe or attach to one session according to policy.

### Role

A named bundle of project capabilities, such as Viewer, Contributor, Maintainer, or Owner.

### Capability

One daemon-enforced permission to perform an operation in a scope, for example `session.observe`, `agent.invoke`, or `worktree.create`.

### Authorization decision

The allow, deny, or conditional result of evaluating a principal, project, resource, requested capability, and policy context.

## 5. Project and source-control terms

### Project

The durable collaboration, authorization, audit, communication, and scheduling boundary.

A project is a logical entity. It remains the same when its repository is moved, cloned onto another node, represented by several worktrees, or accessed from another frontend.

Suggested identifier: `ProjectId`.

A project MAY eventually include several repositories. Initial implementations MAY enforce one primary repository.

Do not use `project` to mean the current directory.

### Project catalog

The coordinator-owned collection of known projects, including explicit projects and lazily discovered candidates.

### Project discovery root

A configured filesystem location searched for project candidates, for example `~/projects`.

A discovery root is not itself necessarily a project.

### Project membership

The relation between a principal and a project, including roles, direct capabilities, restrictions, and lifecycle.

### Repository

A version-control lineage associated with a project.

Repository identity SHOULD include stable Git metadata, remotes, and object lineage when available. It MUST NOT be represented solely by a checkout path.

Suggested identifier: `RepositoryId`.

### Workspace

One concrete checkout location on one execution node.

A workspace has a stable `WorkspaceId`, project, repository, node, canonical root, health, and lifecycle. It may be a primary checkout or a Git worktree.

A workspace is the filesystem and execution boundary supplied to tools through an immutable execution context.

Do not use `workspace` as a synonym for project.

### Workspace locator

The location needed to reach a workspace, for example a local path or SSH host/path pair.

A locator may change while `WorkspaceId` remains stable.

### Workspace root

The canonical filesystem root of a workspace on its execution node.

This is a path, not an identifier.

### Worktree

A Git worktree or equivalent isolated checkout with explicit owner and lifecycle.

A worktree is a specialized workspace. It has a `WorktreeId`, repository, workspace, node, path, branch, base commit, owner, state, and timestamps.

### Worktree owner

The session, agent run, job, or explicit shared-project reservation responsible for a worktree.

### Worktree lease

A bounded reservation authorizing read or mutation access to a worktree.

### Execution context

An immutable runtime object resolving project, workspace, root, allowed paths, cancellation, and execution policy for one turn, agent run, or job.

Execution context is runtime state, not durable identity.

### Execution target

The mechanism and destination on which a job or tool executes.

Examples include local node, linked node, SSH target, or external CI runner.

### Remote project

A project whose selected workspace is located on another node or SSH target.

The project itself is not remote; a workspace locator or execution target is remote.

## 6. Session and interaction terms

### Session

A durable human-facing interaction context associated with one project.

A session contains turns, selected agent/model/provider settings, asset-generation references, human ownership, and display metadata. A session may be opened by different frontends and may be observed by authorized principals.

Suggested identifier: `SessionId`.

A session is not a terminal, process, agent run, or project.

### Session owner

The human or service principal primarily responsible for a session.

Ownership does not necessarily imply exclusive visibility.

### Session attachment

A client connection's active association with a session.

Attachment may permit control, read-only observation, or administrative inspection depending on capability.

### Session open

The frontend operation that activates or attaches to a session for interactive use.

Session open MUST trigger project-runtime-asset refresh according to the long-term specification.

### Session observation

An authorized read-only subscription to another session's frontend-neutral projection.

Observation MUST NOT imply control or permission-response authority.

### Observation subscription

The ephemeral protocol object representing a read-only stream of one session's snapshot and subsequent events.

Suggested identifier: `ObservationSubscriptionId`.

### Turn

One submitted human-to-agent interaction cycle.

A turn begins with a prompt submission and ends in completed, failed, cancelled, interrupted, or waiting state according to protocol semantics.

Suggested identifier: `TurnId`.

### Prompt

The user-authored input initiating or steering a turn.

Prompt content may have retention and visibility policies separate from structural turn metadata.

### Steering

An explicit authorized input to an active turn. Steering is not ordinary project chat and MUST require target-session control authority.

### Permission request

A request emitted by a turn or agent run for authorization to invoke a tool or perform an operation.

An observer may see a permission request but cannot answer it without explicit control authority.

### Question request

A structured request from an agent to its controlling human or service context.

## 7. Agent terms

### Agent definition

A declarative specification of an agent's role, prompt, model policy, tool permissions, delegation policy, budget defaults, runtime kind, and presentation metadata.

Compiled built-ins, CodeGG TOML files, config overlays, and explicit compatible imports are sources of agent definitions.

An agent definition is not an executing agent.

### Resolved agent definition

The effective agent definition after deterministic source precedence, overlay merging, validation, and policy narrowing.

It MUST retain source provenance and a content digest.

### Agent run

One executing instance of a resolved agent definition.

An agent run has durable identity, root and parent relationships, originating principal/session/turn, project/workspace/worktree, execution node, status, authority envelope, budget, timestamps, and output references.

Suggested identifier: `AgentRunId`.

Do not use `subagent` as the only durable type. A subagent is an agent run with a parent.

### Root agent run

The first agent run created for a turn or structured task.

### Descendant agent run

An agent run with a parent agent run. It may itself create descendants when policy permits.

### Subagent

UI and conversational shorthand for a descendant agent run.

Protocol and storage SHOULD prefer `AgentRun` with `parent_run_id` over separate root/subagent record types.

### Agent tree

The rooted hierarchy of agent runs associated with one root run.

The tree includes status, delegation, joins, cancellation propagation, budgets, jobs, and worktrees.

### Agent task

A delegated unit of intent created by an agent run.

A task may produce a child agent run, deterministic job, or external-runner job. Task identity and agent-run identity MUST NOT be conflated.

Suggested identifier: `AgentTaskId`.

### Delegation

The operation by which one agent run creates an agent task or child agent run with a narrowed authority and budget.

### Delegated authority

The capabilities, paths, tools, project scope, and execution limits a parent grants to a descendant.

### Agent budget

The bounded token, tool-call, wall-clock, child-count, depth, and resource allowance assigned to an agent run or tree.

### Join policy

The rule governing how a parent waits for child work, such as all, any successful, first completed, or detached.

### Agent activity projection

A bounded frontend-neutral summary of what an agent run is doing. It is not hidden model chain-of-thought.

## 8. Scheduler and execution terms

### Job

A durable scheduler-owned description of intended work.

A job includes project/workspace scope, kind, source, priority, payload, resource request, timeout, retry policy, idempotency class, dependencies, and schedule linkage.

Suggested identifier: `JobId`.

### Job attempt

One execution of a job under a coordinator generation, execution node, executor, and lease.

Suggested identifier: `AttemptId`.

A job may have several attempts. Attempt status is not job identity.

### Schedule

A durable rule that creates or activates jobs in the future.

Suggested identifier: `ScheduleId`.

### Run

A structured record of one command, script, test, or tool execution, including output and artifacts.

A run MAY be linked to a job attempt. A run is not the global scheduler job itself.

Suggested identifier: `RunId`.

### Executor

A component that performs a job attempt, such as local process, subagent, SSH, linked node, or external CI backend.

### Execution backend

The transport and lifecycle adapter used to execute work on a target.

### Resource request

The scheduler-visible CPU, memory, process, accelerator, exclusivity, or domain-resource requirement for a job.

### Admission

The scheduler decision that sufficient capacity and policy permit an attempt to begin.

### Permit

The RAII or durable ownership object reserving admitted resources.

### Idempotency key

A caller-provided or deterministically derived key used to detect retransmitted submissions and return the existing durable object.

### Cancellation

A durable requested state change propagated through scheduler, executor, process tree, descendant agents, and worktree lifecycle according to policy.

## 9. Asset and interoperability terms

### Runtime asset

A project- or user-provided declarative input used to construct agent behavior.

Runtime assets include agent definitions, skills, project instructions, prompt fragments, and related metadata.

### Project asset registry

The source-aware collection of runtime assets discovered for one project/workspace.

It records candidate sources, validation, precedence, shadowing, and digests.

### Project asset snapshot

An immutable, versioned, validated set of effective runtime assets for one project/workspace at one generation.

Suggested identifier: `ProjectAssetSnapshotId` or `(ProjectId, WorkspaceId, AssetGeneration)`.

### Asset generation

A monotonically increasing project/workspace version assigned after a successful transactional asset refresh.

### Runtime asset snapshot

The immutable asset snapshot captured by a turn or agent run at start.

An active turn remains pinned to this snapshot even when a newer project asset generation is loaded.

### Skill

A portable reusable instruction package represented by a directory containing `SKILL.md` and optional resources.

A skill is discovered by metadata and activated by loading its full body. Discovery does not execute bundled scripts.

### Skill source

The path and harness namespace from which a skill was discovered, such as `.codegg`, `.agents`, `.opencode`, or `.claude`.

### Skill activation

The explicit loading of one skill's full instructions into a turn or agent-run context.

### Skill resource

A file bundled under a skill directory, such as a script, reference, or asset. Resource access remains subject to path, permission, sandbox, and audit policy.

### Shadowing

The deterministic selection of a higher-precedence asset when several sources define the same logical name.

Shadowing MUST retain diagnostics and provenance.

### Asset refresh

The transactional rescan, parse, validation, resolution, and atomic replacement of a project asset snapshot.

Asset refresh occurs on project activation, session create/open/attach, workspace rebind, explicit command, and accepted remote manifest update.

### Foreign harness asset

A compatible asset stored under another coding harness's standard directory.

CodeGG treats foreign harness directories as read-only discovery sources unless the user explicitly asks CodeGG to write there.

### Harness adapter

An explicit parser and semantic mapping for a foreign harness format.

CodeGG MUST NOT claim agent-definition interoperability without an adapter that preserves or rejects incompatible semantics explicitly.

## 10. Provider terms

### Provider

An implementation family capable of sending model requests, such as OpenAI-compatible, Anthropic, or another API protocol.

### Provider connection

A durable configured endpoint and credential reference used by sessions and agent runs.

It includes stable ID, provider kind, endpoint, secret reference, owner, scope, health, capabilities, and model catalog.

Suggested identifier: `ProviderConnectionId`.

### Provider connection scope

The principals and projects allowed to use a provider connection: personal, project, or deployment.

### Eggpool connection

A provider connection whose endpoint is an Eggpool service, defaulting to port 11300 unless configured otherwise.

### Model catalog

The bounded discovered set of models and capabilities available through one provider connection.

## 11. Presence and collaboration terms

### Presence lease

An ephemeral, renewable record indicating that a principal, session, or node is active.

Presence lease expiration does not delete durable session or audit history.

### Principal presence

A project-scoped active, idle, or disconnected projection for a human or service principal.

### Session activity

A bounded state such as viewing, composing, agent running, tool running, build running, awaiting human, paused, or disconnected.

### Project channel

A durable communication stream associated with a project.

Suggested identifier: `ChannelId`.

### Chat message

A human- or agent-authored communication record in a project channel.

A chat message is not an audit event and is not automatically a command.

Suggested identifier: `MessageId`.

### Structured chat action

A separately authorized operation created from a chat interaction, such as launching an agent task or requesting review.

### Mention

A structured reference to a principal, agent run, session, job, or other project object in a message.

### Read marker

A principal-specific indication of the last message or event read in a channel.

## 12. Audit and observability terms

### Audit event

An append-only structural record of a security-, execution-, configuration-, or project-relevant action.

Audit events record actor, scope, action, decision, correlation, causation, visibility, bounded metadata, and optional content digest.

Suggested identifier: `AuditEventId`.

### Audit content

Optional retained prompt, command, output, or file content associated with an audit event.

Audit content may expire independently from structural audit metadata.

### Correlation ID

An identifier grouping related operations across protocol requests, agent runs, jobs, nodes, and external services.

### Causation ID

The direct prior event or operation that caused another event.

### Trace ID

An observability identifier used by tracing systems. It does not replace CodeGG domain identifiers.

### Event visibility

The policy classification controlling who may receive an event or its content, such as project, session participants, actor only, administrators, or secret-redacted.

## 13. Protocol terms

### Native CodeGG protocol

The versioned control-plane and frontend protocol for project, session, agent, job, provider, node, chat, and audit operations.

### ACP adapter

The boundary translating Agent Client Protocol operations into native CodeGG protocol and domain operations.

ACP is not the coordinator-to-leaf protocol.

### Snapshot

A bounded point-in-time projection used to initialize or repair client state.

A snapshot is not necessarily the canonical storage record.

### Event

An ordered state transition or activity notification following a snapshot.

### Source sequence

A monotonically increasing event number assigned by a leaf or source process before coordinator acceptance.

### Global sequence

The deployment-wide event order assigned by the coordinator.

### Resume

A request to continue an event stream after a known acknowledged sequence.

### Resynchronization

Replacement of incomplete client state with a new snapshot when event history is unavailable or incompatible.

### Capability negotiation

The protocol exchange establishing supported optional operations and semantic versions.

## 14. Compatibility mapping from current code

### Current `project_id`

Current path-derived or compatibility `project_id` fields SHOULD migrate to a stable `ProjectId`. Until migration, they MUST be treated as compatibility projections and MUST NOT become the authorization key for team mode.

### Current `WorkspaceId`

Retain as the stable identity of one concrete checkout. Add explicit `ProjectId`, `RepositoryId`, and `NodeId` relations rather than renaming workspace into project.

### Current `directory`

Treat as a workspace-root compatibility projection. New daemon-owned code SHOULD resolve paths through `ExecutionContext`.

### Current `SubAgentTask`

Split conceptual responsibilities into `AgentTask` and `AgentRun`. A task is delegated intent; a run is execution. Preserve compatibility DTOs only during migration.

### Current `SubAgentPool`

Evolve into an `AgentRunService` and scheduler executor. Pool/semaphore implementation details MUST NOT define durable agent hierarchy or team-wide concurrency.

### Current `TaskStore`

Migrate to durable agent-task and agent-run stores with typed IDs, tree relationships, budgets, authority, and project/workspace scope.

### Current `SkillIndex`

Evolve into a source-aware project asset registry and immutable project asset snapshots. The current vector is a compatibility implementation, not the target ownership model.

### Current `AgentRegistry`

Retain deterministic overlays and provenance, but remove process-global `PWD` as project selection. Resolve project agent files from explicit workspace/project context and refresh transactionally.

### Current `ServerState.project_dir`

Replace with project catalog and explicit request/session project scope. A network server MUST NOT have one process-global current project.

### Current global bearer token

Retain only as a personal-remote or development bootstrap option. Team mode requires individual principals and project authorization.

### Current remote TUI state

Preserve structured snapshots and replay. Extend it with project-scoped subscriptions, principal identity, observation mode, agent trees, presence, and chat. Do not introduce terminal-frame mirroring as the canonical model.

## 15. Prohibited ambiguous usage

The following phrases SHOULD be removed from new design documents unless qualified:

- "the project directory" when the intended object is a workspace root;
- "remote project" when the intended object is a remote workspace or execution target;
- "the agent" when several agent runs may exist;
- "subagent task" when distinguishing delegated intent from execution matters;
- "server" when coordinator ownership is intended;
- "worker" when execution node, scheduler executor, or agent worker is intended;
- "session" when turn, client connection, terminal, or process is intended;
- "run" when job, attempt, agent run, or command run is intended;
- "reload config" when the operation specifically refreshes runtime assets;
- "shared workspace" when isolated worktrees are intended.

## 16. Review checklist

Any implementation plan or architectural change SHOULD answer:

1. Which durable IDs are involved?
2. Which object owns the mutable state?
3. Is the operation project-, workspace-, session-, agent-, job-, or node-scoped?
4. Is a path being mistaken for identity?
5. Which principal or agent actor is attributable?
6. Which authorization capability is required?
7. Which execution node owns the filesystem/process operation?
8. Which scheduler job or permit owns heavy execution?
9. Which runtime asset generation is active?
10. What happens on refresh, reconnect, duplicate delivery, cancellation, and restart?
11. What is visible to observers, chat participants, and audit readers?
12. Which compatibility field or path remains, and how is it retired?