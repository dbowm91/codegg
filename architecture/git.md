# Git Subsystem — Typed Operations, Structured Execution, and Recovery

The Git subsystem spans three crates and a dozen root adapters, providing a
typed workflow vocabulary (codegg-git), generic read/process mechanics
(egggit), and CodeGG durable lifecycle integration in codegg-core/root. It
serves the command-intent classifier, command planner, BashTool dispatch, the
native Git tool, provenance tracking, TUI sidebar, and RunStore persistence.

## Purpose

Provide a single source of truth for Git argv parsing, risk classification,
structured read execution, typed mutation execution with snapshot/delta
capture, network/destructive operation support, and operation-aware
conflict recovery — all without exposing credentials through display,
logging, or persistence surfaces.

## Where It Lives

| Layer | Crate / Module | Role |
|-------|----------------|------|
| Generic Git mechanics | `crates/egggit/` | `process` (canonical environment policy and shell-free process construction), structured status/diff/log/blame/refs/worktree/conflict/operation-state reads |
| Typed workflow model | `crates/codegg-git/` | `GitOperation`, risk/parser/render/path/ref/sensitive types, portable `RepoSnapshot`/`StateDelta`/`MutationResult` workflow boundary |
| Execution adapter | `src/git_service.rs` | `GitExecutionService` — maps typed operations to structured egggit reads and the generic process boundary |
| Mutations | `src/git_mutations.rs` | `GitMutationExecutor` and repository snapshot/delta orchestration over `codegg_git::workflow` |
| Index publication | `src/git_publication/` | Private-index staging, compare-and-swap publication under Git's own `index.lock`, crash reconciler; durable journal in `codegg-core::git_publication` |
| Typed ops | `src/git_mutations_ops.rs` | Stage, commit, branch, restore, stash, merge, rebase, cherry-pick, revert, tag-delete |
| Network | `src/git_network_ops.rs` | Fetch, pull, push, remote add/remove/set-url/rename, config get/set/unset, reset variants, clean |
| Network policy | `src/git_network_policy.rs` | `NetworkEnvPolicy`, `NETWORK_ALLOWED_ENV_VARS`, `NetworkFailureKind`, URL credential redaction, `sanitize_argv_for_run_store` |
| Recovery | `src/git_recovery.rs` | `continue_in_progress`, `abort_in_progress_typed`, `skip_in_progress`, `assert_action_matches` |
| Projector | `src/git_mutation_projector.rs` | `project_mutation`, `project_network_mutation`, `project_destructive_mutation`, `project_recovery` |
| RunStore | `src/git_run_store.rs` | `persist_mutation`, `persist_recovery` |
| Tool surface | `src/tool/git.rs` | `GitTool` with `subcommand`, `mutation` (42 actions), `recover`, `operation_state` |
| Canonical env/process policy | `crates/egggit/src/process.rs` | `ALLOWED_ENV_VARS` (21), `ALWAYS_STRIPPED_ENV_VARS` (28), `GitEnvPolicy` — single source of truth; `codegg-git` keeps compatibility re-exports |
| Managed worktree lifecycle | `crates/codegg-core/src/worktree_service.rs` | Durable worktree identity/lease state over hardened add/remove helpers |

## Operation ownership matrix

| Operation category | Canonical owner | Adapters/callers |
|---|---|---|
| Repository discovery and identity inspection | `egggit::worktree`, `egggit::refs`; lineage normalization in `codegg-core::repository_lineage` | workspace/project catalog, security review |
| Status, diff, log, blame, ref/object reads | `egggit` read modules | `GitExecutionService`, TUI sidebar, review/security tools |
| Safe argv parsing/rendering and path/ref validation | `codegg-git` typed model | command intent/planner, GitTool, mutation/network adapters |
| Local Git process construction and environment hardening | `egggit::process` | all CodeGG-owned local Git subprocess callers |
| Credential-safe remote process overlay and failure classification | root `git_network_policy` / `git_network_ops` | typed network mutation adapters; raw credentials remain ephemeral |
| Commit, stage, restore, branch, stash, merge/rebase/cherry-pick/revert | root `git_mutations` + `git_mutations_ops` | GitTool and command routing |
| Private index publication (stage → compare-and-swap → publish → recover) | root `git_publication` publisher; durable journal in `codegg-core::git_publication` | the `Add` arm of `GitMutationExecutor`, GitTool `stage_paths`, the commit tool, daemon bootstrap reconciler |
| Worktree create/remove/list/validate and leases | `codegg-core::worktree_service` (durable lifecycle); generic facts in `egggit::worktree` | agent-run scheduler/worktree services |
| Base/result commit relationships and integration | `codegg-core` agent-run/worktree services and root `agent::run_integration` | parent-side integration only |
| Mutation provenance and rerun metadata | root `git_run_store` over `codegg_git::workflow` results | RunStore; future rerun vertical M005 |
| Integration/recovery/promote operations | root typed mutation/recovery adapters | agent-run integration and GitTool |
| Projection-safe mutation summaries | root `git_mutation_projector` | TUI/model projection and RunStore artifacts |

The crate boundary is intentional: `egggit` cannot own CodeGG durable
identity without violating the dependency direction, and `codegg-git` cannot
depend on `codegg-core` without creating a cycle. The portable workflow result
types therefore live in `codegg-git`, while durable lifecycle and persistence
remain in their existing CodeGG owners. There is one generic subprocess
constructor and one typed workflow result vocabulary.

## How It Works

### Read path

`GitExecutionService::execute` accepts a typed `GitOperation` and repository
root. Read-only operations (`Status`, `Diff`, `DiffStaged`, `Log`, `Blame`,
`ChangedFiles`, `BranchList`, `TagList`, `RemoteList`, `RemoteGetUrl`,
`WorktreeList`, `StashList`, `Show`) delegate to egggit functions for
structured parsing, falling back to raw subprocess if egggit fails.
All other operations fall through to `execute_raw` (subprocess via
`render_argv`).

Every raw execution call flows through `egggit::process::GitEnvPolicy::apply()`
(env-clear, allowlist restore, hard-deny strip, editor/pager pinning,
kill_on_drop). stdout/stderr pass through
`redact_url_credentials_in_text` before reaching any downstream consumer.

The trusted process policy also pins repository-controlled execution settings
with Git `-c` options: hooks, fsmonitor, external diff, credential helpers,
SSH command overrides, signing programs and pagers are disabled; diff/show/log
also receive `--no-ext-diff --no-textconv`. The environment additionally sets
`GIT_NO_REPLACE_OBJECTS=1`, so a repository-declared `refs/replace/*` cannot
substitute one object for another on a read. `classify_git_request` returns a
typed `GitExecutionPolicyOutcome` before command construction. It distinguishes
trusted non-executable requests, requests requiring contained execution or
additional authorization, explicit authority changes, and malformed requests.
The command builder returns this outcome instead of spawning a fabricated
missing executable. Operations requiring filters, merge drivers, or remote
transport remain unavailable until their containment/mediation plan lands;
their subcommand names alone are not treated as the security boundary.
`stash list`, `worktree list`, and the verified non-executable local flows
remain available. This policy does not protect writable `.git` metadata or
claim network isolation; those are separate Linux execution-isolation gates.

**Pinned keys are not enough on their own.** A `-c` override beats the keys
it *names*, in every configuration file the child reads — so an included file
cannot smuggle a named key past it. It cannot beat a key it does not name,
and two of those keys are named by the *repository*: `filter.<name>.clean`
and `diff.<name>.command`. `git status` and `git diff` are on the trusted
read route and both compare the working tree against the index *through the
clean filter*, so a repository that declares a filter and ships an attribute
that selects it gets that command executed by an ordinary read. This was
reproduced on this host before it was fixed.

`crates/egggit/src/config_surface.rs` therefore inspects the repository's
configuration surface before the trusted route spawns. It parses the
configuration itself (it never shells out to `git config`, which would honour
includes and could run helpers), follows `include.path` and
`includeIf.<condition>.path`, and is bounded on include depth, file count,
per-file size and total bytes. It reports the dangerous **surfaces** it finds
and never a configuration **value**, so a credential helper whose value embeds
a password cannot leak through the report. The trusted route refuses when the
surface declares something it cannot neutralise:

| Surface | Disposition |
|---|---|
| `filter.<name>.process` / `.clean` / `.smudge` | refused — reachable from an ordinary `git status` or `git diff` |
| `diff.<name>.command` | refused — a repository-chosen diff driver |
| an inspection that was truncated | refused — "we could not check" is not "we checked and found nothing" |
| `core.attributesFile`, `core.alternateRefsCommand`, `core.gitProxy`, `core.pager`, `core.editor`, `sequence.editor` | neutralised by the pinned override table |
| `url.*.insteadOf`, `credential.<url>.helper`, `core.sshCommand` | reachable only from a remote operation, which the trusted route refuses |
| `merge.<name>.driver` | reachable only from a merge operation, which the trusted route refuses |

The refusal reuses the existing typed vocabulary
(`RequiresContainedExecution` with `CleanSmudgeFilter` or
`ExternalDiffOrTextconv`, which the route's gate turns into
`BackendUnavailable`). No new outcome shape was invented and **no Git
authorization was widened**: the trusted read route can now do less, not
more.

Two limits of the inspection are recorded rather than papered over. It is a
pre-flight, so it cannot close the window between the check and the spawn.
And it follows `includeIf` conditions **unconditionally**, because
`onbranch:`, `gitdir:` and `hasconfig:` cannot be evaluated faithfully at
inspection time — so it over-reports relative to what the child will
actually read, which is the safe direction.

Returns `GitExecutionResult` with `GitPayload` (status, diff, log, branches,
tags, remotes, worktrees, stashes, show, etc.), raw stdout/stderr,
exit code, `ProjectionHints`, and the **consistency class** the read was
obtained under (see "Read consistency" below).

### Read consistency

A Git read is only as trustworthy as the object generation it observed, and a
consumer that needs one stable generation has to be able to *say* so at the
type level rather than in prose. `crates/egggit/src/consistency.rs` owns that
vocabulary:

- **`GitReadConsistency`** is the closed class a read carries:
  `ImmutableGeneration` (one materialized object generation), `CapturedNotCurrent`
  (a capture that was immutable when taken and may now be stale),
  `LiveMutable` (bytes read from a concurrently mutable repository; the
  observation may be torn), and `NoObjectGeneration` (the read consulted no
  object store).
- **`GitReadFact<T>`** is a read result *together with* its class. The class
  travels with the value, so a consumer cannot receive the bytes and forget
  what it was allowed to assume about them. Eighteen typed `egggit` read
  functions return it.
- **`GenerationConsistent<T>`** is the only shape a generation-bound consumer
  may accept, and `admit_generation_consistent` is the only way to obtain
  one. A live-class read is **refused**, not warned about.
- **`LiveReadCapability`** is the authority a consumer must **hold** to use a
  live-class read — on the same footing as `DestructiveRepositoryChange` gates
  a destructive mutation. Its only constructor is compiled out unless
  `egggit`'s `git-live-read-test-support` feature is enabled, and the shipped
  binary does not enable it. **A production build therefore cannot obtain the
  capability at all.**
- `GitReadConsistency::weaker` is the composite rule: a read assembled from
  several Git invocations carries the **weakest** class among them.

Two boundaries of the class are worth stating plainly:

- **The class describes the object store only.** The private object view
  covers the object database; refs, the index and the worktree are always read
  live. A read that consults no object store reports `NoObjectGeneration`
  rather than borrowing a class it did not earn.
- **`CapturedNotCurrent` is not a weaker kind of `ImmutableGeneration`.** The
  preparation cache is a best-effort mechanism with eviction, not a durability
  boundary, so a cache-served read reports `CapturedNotCurrent`. It is still
  admissible to a generation-bound consumer, because such a consumer needs
  the *internal consistency* of the bytes; currency is a separate question
  that the publisher's compare-and-swap answers.

The root crate's fence is `src/git_consistency.rs`. `admit_generation_bound`
maps a refusal onto the typed `GitServiceError::Consistency` variant, so a
caller that ignores the fence still cannot turn a refusal into a payload.
`live_read_capability()` returns `None` in every build: that is the single
place a live route would have to change, and changing it is a reviewed
decision rather than a build convenience.

The **publication** boundary records the class too. `PreparedPrivateIndex`
carries the class of the index generation the publication is based on, and
`GitIndexPublicationReceipt` carries the class it was committed from. A
live-class staging is refused before the lock is taken and before the journal
is written: a live read may be torn by an outside writer, and a torn
generation has no fingerprint to compare and swap against.

Object-reading trusted commands use a private object view without
`objects/info`, so a repository alternate created after admission cannot
redirect the command. The view contains copies, never hard links, and is held
for the complete child lifetime. C011 caches up to four immutable views and 2
GiB within the daemon process. Linux inotify invalidates a generation on
loose or packed object changes, watch loss, or queue overflow; cache misses
install watches before fingerprinting and copying, then reject a view if a
change arrives during construction. Where inotify is unavailable, cache hits
validate a SHA-256 inventory fingerprint. Active Git children retain a lease
across eviction and daemon shutdown. Cache pressure uses a command-scoped
snapshot and never falls back to mutable source objects. Static alternates
remain typed unavailable. This read-only boundary does not admit object-
writing Git mutations or remote operations.

The cache lowers repeated Git-read preparation on ext4, but fragmented cold
snapshots remain above the accepted M003 latency budget. C010 remains the
governing no-production-admission decision until the complete performance and
compatibility gates pass.

### Contained managed-worktree creation

`WorktreeService::create_reserved` reaches `git worktree add`, which
`classify_git_request` classifies as requiring `CleanSmudgeFilter`
containment: a linked worktree writes the branch's files through the
repository's smudge filter, and a smudge filter is arbitrary code. The
capability is not satisfiable by neutralizing configuration, because a filter
*command* lives under a name the caller cannot enumerate, so a `-c` override
cannot disable an unknown filter. Containment, not policy, is what keeps
repository-controlled code from the host.

`src/git_contained_worktree.rs` is the contained route, authorized by
ADR-0048 (`plans/adrs/ADR-0048-contained-git-execution-for-managed-worktree-creation.md`).
It grants **exactly one operation**, carried out in two steps:

```text
git --git-dir=<gitdir> worktree add --detach <checkout> <commit>
git --git-dir=<administration-dir> symbolic-ref HEAD refs/heads/<branch>
```

The attached form of `worktree add` is deliberately **not** used. Measured on
Git 2.43.0 and 2.55.0: `git worktree add <path> <existing-branch>` locks and
rewrites `refs/heads/<branch>` even though the value is unchanged, so it fails
closed against this grant with `cannot lock ref 'HEAD'`. The detached form
writes **no ref at all**, and attaching the branch afterwards is a write to the
administration directory the grant already names. Both steps run inside the
same grant and the same containment.

Three properties carry the contract:

- **The branch is the daemon's.** The daemon creates it first through the
  existing typed, policy-allowed `git branch` path, so the child attaches to a
  branch that already exists and writes **no ref**. The grant is what
  enforces this: `refs` is read-only to the child, so a child that tried to
  create a branch would fail closed rather than write one.
- **The attach step runs last, with the daemon's target.** The administration
  directory is inside the child's grant, so repository-controlled code could
  write that directory's `HEAD` and point the worktree at a branch of its
  choosing. The attach is the child's final step and its target is fixed by
  the daemon's argv, so such a write is overwritten rather than trusted. The
  registration check then reads the end state back through Git itself.
- **The grant is the administration directory and nothing else.** The child may
  write the new checkout, the daemon scratch, and
  `<common-gitdir>/worktrees/` — the linked worktree's own administration
  directory, which Git always places inside the common Git directory
  (`git worktree add` has no separate-git-dir option). Every other path under
  the Git directory — `refs`, `objects`, `index`, `config`, `hooks`,
  `packed-refs` — is read-only to the child. The child runs with the Git
  directory as its working directory and never sees the source working tree.
- **Containment is asserted, not requested.** The launch's *obtained*
  enforcement is read after the launch; a request is a hope. The grant itself
  is asserted against the built launch description, not against the
  configuration that was requested.

Two mechanisms make that grant expressible:

- `SandboxConfig::with_read_only_paths` grants a path for reading without
  granting it for writing, so a directory that is immutable except for one
  writable subdirectory can be described.
- `SandboxConfig::with_expected_created_git_pointers` exempts exactly one
  path from the protected mount view's "hide a `.git` that does not exist yet"
  rule. The child has to write the checkout's `.git` **pointer file**, because
  Git refuses to create a worktree in a non-empty directory. The exemption
  names only the top-level `.git` of a granted write root; a nested path is
  refused when the launch description is built. Afterwards the daemon
  verifies that the result is a pointer file naming the expected
  administration directory — a nested repository is a failure, not a success.

The child is still **untrusted code**. The repository's own configuration may
define filters, hooks, merge drivers, fsmonitor hooks and credential helpers;
the sandbox, not the absence of that configuration, is what keeps them from
the host. The checkout's *contents* are untrusted content, and no claim is
made about them.

No other Git operation is affected. `git add`, `git merge`, `git rebase`,
`git stash`, `git am`, `git bisect` and remote transport keep their existing
classification, and `git worktree add` keeps its classification too — the
contained route is a separate, explicitly named path, not a change to what
the ordinary process boundary allows. ADR-0044 remains proposed and owns the
mutable object-read and metadata-promotion classes; neither is granted here.

### Mutation path

`GitMutationExecutor::execute` runs the full 8-step pipeline:

1. Resolve and policy-check repository root (`resolve_repo_root`).
2. Capture pre-operation `RepoSnapshot` via `git status --porcelain=v2 -z
   --branch`.
3. Render argv via `codegg_git::render_argv` (no shell).
4. Execute with timeout and `GitEnvPolicy` hardening. Operations requiring
   repository-controlled filters or merge drivers fail before Git spawn.
5. Capture post-operation snapshot (even on nonzero exit where safe).
6. Classify outcome (`Completed`, `NoOp`, `FastForward`, `Conflict`,
   `Rejected`) from before/after snapshots and exit code.
7. Compute `StateDelta` (commits created, refs created/deleted, paths
   staged/unstaged, conflicts).
8. Sanitize and truncate stdout/stderr (64 KiB max, URL credentials
   redacted) into `MutationResult`.

Typed helpers in `git_mutations_ops.rs` wrap the executor with
operation-specific validation (path validation as `RepoPath`, branch/tag
names as `BranchName`/`RefName`). Merge strategies are gated by
`ALLOWED_MERGE_STRATEGIES`. Rebase refuses interactive mode.

`run_raw_mutation` handles variants the typed parser cannot model (e.g.
`git add -A`, `git reset HEAD`) — same snapshot/policy pipeline.

### Private index publication

`GitOperation::Add` does not reach the in-place `git add` route. It is
published through `src/git_publication/`, which stages into a **private** index
and publishes it into the real repository index under Git's own `index.lock`.
A plain `git add` has no compare-and-swap and no durable record of intent, so
a concurrent writer can be silently overwritten and a crash leaves no evidence
of what was intended.

The path has four parts:

1. **Private staging.** The daemon-pinned Git child runs under the M004
   process/IPC tier and the M005 offline-network candidate. It writes into a
   private gitdir in daemon-owned scratch: a private index seeded byte-for-byte
   from the real index (so an already-staged change is not lost by staging)
   and a private object database whose `alternates` points at the real one, so
   existing objects resolve read-only while new blobs stay private.
   `GIT_INDEX_FILE` is deliberately **not** used — `EnvironmentPolicy` denies
   that variable, and the private gitdir already provides the property it
   would have been used for.
2. **Durable intent.** The intent (the gitdir identity, the index fingerprint
   the staging observed, the staged fingerprint, the staged paths) is written
   to the `git_index_publication` journal (`codegg-core::git_publication`,
   migration v84) before anything is published. The journal's states are
   `prepared`/`publishing` (non-terminal) and `committed`/`aborted`/`ambiguous`
   (terminal); a transition out of a terminal state is refused, not merely
   unused.
3. **Compare-and-swap publication.** The real `index.lock` is taken by a
   same-filesystem `hard_link` from a temp file written *in* the gitdir, so
   the lock's content is correct from the instant it exists and a leftover
   lock is attributable by content hash. Under the lock, the real index's
   fingerprint is compared against the one the staging observed; a mismatch is
   a typed conflict (`StaleIndex`, `GitDirIdentityChanged`) and writes nothing
   at all. The daemon then imports the staged objects (content-addressed,
   idempotent, byte-verified against their source) and replaces the real index
   by an atomic rename inside the gitdir, followed by a directory fsync.
4. **Crash recovery.** A daemon-start reconciler (bootstrap phase 5) resolves
   any publication a prior generation left in flight, from the recorded
   evidence alone, into exactly one of committed / aborted / ambiguous. It
   removes a leftover `index.lock` only when the lock's content equals the
   published fingerprint — an empty or foreign lock is never removed — and
   discards the scratch only for a committed or aborted publication. An
   ambiguous publication is escalated, never guessed at and never replayed.

The published scope is the **index only** (`PublishedResourceScope::IndexOnly`):
no ref is written and the worktree is not modified. Merge, rebase, checkout,
reset, clean, fetch, push, and pull are out of scope.

The receipt states the **obtained** network boundary explicitly
(`ObtainedNetworkBoundary`), rather than leaving the caller to infer an omission.
The boundary is copied verbatim from the staging step's own launch, so it states
what the publication actually ran under:

- `Enforced { backend }` — the staging child was observed inside a private
  network namespace. On a qualified host the contained staging runs under
  M004/M005, so the receipt states `enforced (bubblewrap-netns)`.
- `Unrestricted` — the staging child ran and obtained no network namespace.
- `NotReported` — the staging step does not run a contained child, so there is
  no launch whose obtained boundary could be read. This is the truthful answer
  for a test-supplied staging, and it is a *statement*, not an omission.

The boundary is read off the launch's obtained fact
(`ObtainedNetworkBoundary::from_launch`), never off the request, so a receipt
cannot claim a boundary the staging did not obtain. See
`architecture/security.md` for how the launch derives it.

`Add` is **refused** when no publisher is configured
(`GitMutationError::PublicationRefused`) rather than downgraded to the
in-place route, which cannot be made compare-and-swap safe. Every caller that
can reach an authorized `Add` is therefore given the backend: the `git` tool,
the `commit` tool, and the **bash route** — a bash-routed `git add` is an
authorized `Add` too, and without the backend it would be refused rather than
published. `stage_all` and `stage_tracked` still use the legacy in-place
route — they carry whole-tree `-A` / `-u` semantics that the path-scoped
publisher does not model.

### Network path

Network operations use `NetworkEnvPolicy::apply_to_command`, which extends
the base `GitEnvPolicy` by restoring `NETWORK_ALLOWED_ENV_VARS` (credential
helpers, SSH agent, proxy, git config, author/committer, trace vars) on top
of `ALLOWED_ENV_VARS`. The `ALWAYS_STRIPPED_ENV_VARS` set is then re-applied
as defense-in-depth — any variable in both the network allowlist and the
hard-deny set (e.g. `GIT_ASKPASS`) is always stripped.

`PushForce` enum: `Normal`, `ForceWithLease { expected_sha }`, `Force`.
`PushRequest.is_destructive()` is true for `Force`, `ForceWithLease`, and
`delete`. `PushForce::Force` is rejected at the tool-side policy level.
`push_permission_hint` produces human-readable scope descriptions.

`PullStrategy` enum: `Merge`, `Rebase`, `FastForwardOnly`.

### Managed worktrees

The M003 `WorktreeService` owns only CodeGG-managed local worktree
lifecycles. It resolves the canonical common repository root and base HEAD with structured
read APIs, acquires the shared repository/worktree mutation lock, and invokes
the hardened `codegg-core::worktree` add/remove helpers through
`spawn_blocking`. For linked/nested worktrees, the identity root and the
checkout used to read the effective base are separate inputs. This preserves
the logical repository identity while allowing a child to continue from its
owner's current HEAD. It does not clone repositories, perform network Git
operations, or replace `GitMutationExecutor` for ordinary Git mutations.

Its deterministic branch/path names are validated by `BranchName` and
`ObjectId`; cleanup refreshes structured status and operation state before
removing and never force-removes dirty, conflicted, unknown, or unmanaged
worktrees. Scheduler jobs that perform the lifecycle reserve
`exclusive:worktree-mutation`, matching the existing Git mutation resource
profile.
Fetch with `--prune` uses `ManagedGitArgv` fallback (not modeled by typed
parser).

Network failures are classified by `classify_network_failure` into
`Dns`, `Connect`, `Authentication`, `Authorization`, `RefRejected`,
`Timeout`, `Transport`.

Config operations validate keys against `CONFIG_KEY_ALLOWLIST` (safe
local-scope prefixes: `branch.`, `pull.rebase`, `rebase.autosquash`,
`core.autocrlf`, etc.) and `CONFIG_DENIED_KEY_PATTERNS` (`credential.*`,
`http.*`, `url.*`, `core.gitProxy`, `core.sshCommand`, `core.sshVariant`).
Global-only keys (`user.*`, `gpg.format`) are rejected when
`allow_local_only=true`.

### Recovery

`src/git_recovery.rs` provides `continue_in_progress`,
`abort_in_progress_typed`, and `skip_in_progress`. Each:

1. Calls `detect_operation_state_for_root(repo_root)` to identify the
   active operation via `.git/` plumbing file inspection.
2. Builds the matching typed `GitOperation` variant.
3. Re-checks `assert_action_matches` (TOCTOU defense — re-reads state
   from disk, refuses if state changed or action is illegal).
4. Runs `GitMutationExecutor::execute`.
5. Classifies outcome with recovery-aware labels.

Family-specific guards:
- `Bisect`, `ApplyMailbox`, `Unknown` refuse automatic recovery.
- `Merge` only supports `Continue`/`Abort` (no `--skip` for merge).
- `Sequencer`-driven operations (cherry-pick/revert ≥ Git 2.25) funnel
  through typed codegg-git variants.

`risk_classes_for_recovery`: `Abort` is tagged `HistoryIntegration +
DestructiveHistory`; `Continue` and `Skip` are `HistoryIntegration` only.

The legacy `git_mutations_ops::abort_in_progress` remains as a deprecated
shim for backward compat.

### Projection

`project_mutation(&MutationResult) -> String` formats: operation label,
before/after HEAD/branch, commits/refs created, paths affected, conflicts,
recovery hints, exit code, duration.

`project_network_mutation` appends network output (git fetch/push
summary lines, with byte-count fallback for large output).

`project_destructive_mutation` appends recovery hint (`git reflog` +
`git reset --hard <sha>`).

`project_recovery` formats action, family, outcome, and next-step hint
tailored to the outcome (abort-completed → "repository back to clean
state"; still-conflicted continue → "resolve conflict markers, git add,
then re-run recover: continue").

## Key Types & APIs

### codegg-git (`crates/codegg-git/`)

| Type | File | Purpose |
|------|------|---------|
| `GitOperation` (54 variants) | `operation.rs` | Typed vocabulary for all Git commands |
| `GitRiskClass` (11 variants) | `risk.rs` | Risk classification per operation |
| `RiskSet` | `risk.rs` | `is_destructive()`, `requires_network()` |
| `parse_git_argv` | `parser.rs` | Pre-tokenized argv → `GitOperation` |
| `render_argv` | `render.rs` | `GitOperation` → `Vec<String>` (canonical) |
| `RepoRoot`, `RepoPath`, `Pathspec` | `path.rs` | Path safety types |
| `BranchName`, `RefName`, `RemoteName`, `ObjectId`, `RevisionExpr` | `ref_name.rs` | Ref safety types |
| `RedactedUrl` | `sensitive.rs` | Credential-hiding URL wrapper; `expose_secret()` only at `render_argv` boundary |
| `AuditSafeArgv` | `sensitive.rs` | `RerunDescriptor.argv` type — always sanitized |
| `ALLOWED_ENV_VARS` (21 entries) | `process_policy.rs` (re-export from `egggit::process`) | Canonical allowlist for local git subprocesses |
| `ALWAYS_STRIPPED_ENV_VARS` (28 entries) | `process_policy.rs` (re-export from `egggit::process`) | Hard-deny set — always removed before launch |

### egggit (`crates/egggit/`)

| Type | File | Purpose |
|------|------|---------|
| `RichRepoStatus` | `status_v2.rs` | Structured status with branch, dirty state, conflict entries |
| `ConflictEntry`, `ConflictKind` (8 variants), `ConflictShape` | `conflict.rs` | Typed conflict model — does NOT auto-resolve |
| `ConflictReport` | `conflict.rs` | Aggregated summary from entries |
| `RepositoryOperationState` (9 variants) | `operation_state.rs` | In-progress operation detection from `.git/` plumbing |
| `OperationFamily` (9 variants) | `operation_state.rs` | Routing/UI labels |
| `RecoveryAction` (`Continue`, `Abort`, `Skip`) | `operation_state.rs` | Legal recovery actions |
| `detect_operation_state_for_root` | `operation_state.rs` | Canonical state discovery |
| `CommitInfo`, `BlameResult`, `BranchInfo`, `TagInfo`, `RemoteInfo` | various | Typed read results |

### Root crate

| Type | File | Purpose |
|------|------|---------|
| `GitExecutionService` | `git_service.rs:232` | Unified read+raw executor |
| `GitPayload` (13 variants) | `git_service.rs:42` | Structured read payloads |
| `GitMutationExecutor` | `git_mutations.rs:477` | Mutation executor with snapshot/delta; M002 emits one structural `git_operation` per executed transition via the injected `execution_audit` + `audit_emitter` pair (silent without it). Typed, network, and recovery paths funnel through `execute`; unparsed variants emit in `run_raw_mutation`. Labels/digests via `git_audit_op_label`/`git_audit_ref_digest` (ref/remote-name only, never URLs/messages/paths); see `architecture/audit.md` "Live execution hooks (M002)" |
| `GitEnvPolicy` | `git_mutations.rs:49` (re-export of `egggit::process::GitEnvPolicy`) | `apply()` (async) / `apply_sync()` (sync) |
| `RepoSnapshot` | `codegg-git/src/workflow.rs:15` | Pre/post state capture |
| `StateDelta` | `codegg-git/src/workflow.rs:30` | Diff between snapshots |
| `MutationOutcome` (5 variants) | `codegg-git/src/workflow.rs:64` | Completed/NoOp/FastForward/Conflict/Rejected |
| `MutationResult` | `codegg-git/src/workflow.rs:86` | Full mutation record |
| `GitMutationError` (7 variants) | `git_mutations.rs:219` | Detailed error with `ExecutionContext` |
| `NetworkEnvPolicy` | `git_network_ops.rs:46` | Extends base env with network vars |
| `PushForce` (3 variants) | `git_network_ops.rs:187` | Normal/ForceWithLease/Force |
| `PushRequest` | `git_network_ops.rs:228` | Push parameters |
| `PullStrategy` (3 variants) | `git_network_ops.rs:135` | Merge/Rebase/FastForwardOnly |
| `CleanRequest`, `CleanPreview` | `git_network_ops.rs:713,694` | Clean operation types |
| `NetworkFailureKind` (7 variants) | `git_network_policy.rs:72` | Failure classification |
| `RecoveryOutcome` (5 variants) | `git_recovery.rs:28` | Recovery result classification |
| `GitTool` | `tool/git.rs:23` | Model-facing tool with 42 mutation actions |

## Configuration Surface

| Config field | Default | Effect |
|-------------|---------|--------|
| `command_intent.route_git_local_mutation` | `off` | Gates Bash→Git routing of local mutations |
| `command_intent.route_git_network` | `off` | Gates command-intent routing of network ops |
| `command_intent.route_git_destructive` | `off` | Gates command-intent routing of reset/clean |

Tool-level typed actions (the model-facing `git` tool `mutation` parameter)
are unaffected by these flags — they route through the dedicated mutation
action regardless of routing mode.

### Env policy layers

1. `GitEnvPolicy::apply()` / `apply_sync()`: clears env, restores
   `ALLOWED_ENV_VARS` (21 vars), strips `ALWAYS_STRIPPED_ENV_VARS` (28
   vars), pins `GIT_TERMINAL_PROMPT=0`, `GIT_EDITOR=true`,
   `GIT_SEQUENCE_EDITOR=true`, strips `EDITOR`/`VISUAL`, sets
   `GIT_PAGER=cat`, `PAGER=cat`, `GPG_TTY=""`; adds fixed Git config
   overrides for executable repository settings; pins `--work-tree` to the
   canonical root supplied by the daemon, caps upward repository discovery at
   its parent, and disables system/global Git configuration; returns typed outcomes for
   caller-supplied global config/repository redirects and capability needs
   before spawn. An unknown Git subcommand is classified as a contained
   repository-alias capability rather than sent to Git.
2. `NetworkEnvPolicy::apply_to_command()`: same as base, plus restores
   `NETWORK_ALLOWED_ENV_VARS` (21 vars: `GIT_ASKPASS`, `GIT_SSH_COMMAND`,
   `GIT_SSH_VARIANT`, `GIT_CONFIG_COUNT/GLOBAL/SYSTEM`, `GIT_AUTHOR_*`,
   `GIT_COMMITTER_*`, `HTTP(S)_PROXY`, `http(s)_proxy`, `NO_PROXY`,
   `no_proxy`, `GIT_TRACE`, `GIT_TRACE_PACKET`, `GIT_CURL_VERBOSE`).
   Re-strips `ALWAYS_STRIPPED_ENV_VARS` as defense-in-depth.
3. Canonical source of truth: `crates/egggit/src/process.rs`.
   `codegg-git/src/process_policy.rs` is a re-export shim for
   compatibility. Both root crate and `codegg-core::worktree` consume
   the same lists.

For commit-producing operations, the process boundary preserves normal Git
identity precedence without loading arbitrary global configuration. It reads
only missing `user.name` and `user.email` values from the selected repository's
local config and the user's global config, then passes those values as bounded
command-scoped data. Global hooks, aliases, helpers, and other settings remain
disabled. Identity values are data; they do not enable executable metadata.

## Invariants & Gotchas

1. **egggit owns generic Git mechanics and reads, never CodeGG lifecycle.**
   Its process boundary is the only production owner of literal `git`
   construction. Mutations and durable workflow orchestration use the root
   and codegg-core adapters (`git_mutations`, `git_mutations_ops`,
   `git_network_ops`, `git_recovery`, and `worktree_service`).

   Every structured read is bounded by `egggit::process::DEFAULT_READ_TIMEOUT`
   (30s, matching `GitExecutionService`'s default). The deadline is enforced
   *inside* the `spawn_blocking` closure against the running child, not
   wrapped around the join: a blocking task is not cancellable, so an outer
   `tokio::time::timeout` would return to the caller while the Git process kept
   running and kept holding a blocking-pool thread. On expiry the process
   group is killed — not just the direct child — so a pager, credential
   helper, or filter driver cannot outlive it and keep holding the repository
   lock. On Unix, the child is made a new process-group leader before spawn;
   this keeps group cleanup scoped to the child tree and away from the caller's
   process group. stdout/stderr are drained concurrently so a large-output child cannot
   deadlock the wait on a full pipe. `GitExecutionService` additionally bounds
   each structured phase by its own `timeout`, so `with_timeout` is
   authoritative for multi-command reads.

2. **Single parsing/rendering truth.** `parse_git_argv` and `render_argv`
   in `codegg-git` are the only Git argv parsers. No duplicate parser
   logic in downstream crates.

3. **Path/ref safety at parse time.** `RepoPath` rejects NUL bytes,
   absolute paths, parent traversal (`..`), paths resolving outside the
   repository root. `BranchName`/`RefName` reject empty, leading `-`,
   `..`, `.lock` suffix, special chars.

4. **Credential leakage prevention.** `RedactedUrl` wraps raw URLs so
   `Debug`/`Serialize`/`Display` never expose credentials. Raw value
   reaches Git child only via `expose_secret()` at the `render_argv`
   boundary. `sanitize_truncate_for_result` redacts stdout/stderr.
   `sanitize_argv_for_run_store` redacts persisted argv.
   `redact_url_credentials_in_text` redacts URL-shaped tokens in
   arbitrary text.

5. **Force-push rejection.** `PushForce::Force` is classified
   `DestructiveHistory` by the parser, so command-intent routing carries
   `DestructiveFileMutation` capability (default: `Deny`). The tool-side
   policy also rejects it.

6. **Broad clean rejection.** `CleanRequest::is_broad()` (ignored=true,
   no paths) is rejected at the tool dispatch layer.

7. **Config key gating.** `credential.*`, `http.*`, `url.*`,
   `core.gitProxy`, `core.sshCommand`, `core.sshVariant` are always
   denied. Global-only keys (`user.*`, `gpg.format`) rejected when
   `allow_local_only=true`.

8. **Merge strategy allowlist.** Only `recursive`, `resolve`, `octopus`,
   `ours`, `subtree`, `ort` are permitted. Arbitrary strategy strings
   are rejected.

9. **Recovery TOCTOU defense.** `assert_action_matches` re-reads
   operation state from disk immediately before executing the recovery
   action. State mismatch → `GitMutationError::StateMismatch`.

10. **Shell boundary.** Commands with pipes, redirects, command
    substitution, semicolons, env assignments, or quoted glob patterns
    are NOT rewritten as `ActualBackend::Git`. They remain
    `ActualBackend::RawShell`. The classification boundary is
    `shell_shape::parse_shell_words` + `has_shell_operators()`.

11. **Canonical subprocess policy.** `ALLOWED_ENV_VARS`,
    `ALWAYS_STRIPPED_ENV_VARS`, and `GitEnvPolicy` live in
    `egggit::process`. `codegg-git` and `codegg-core` expose compatibility
    paths but do not maintain copies. Drift is caught by the Git forbidden
    pattern guard and the existing policy tests.

12. **Programmatic availability (M003).** Tool Programs cannot call the
    multiplexed `git` tool (`DirectOnly`). The hidden `ProgrammaticOnly`
    `git_read` adapter (`src/tool/git_read.rs`) exposes only
    `status`/`diff`/`log`/`branches` through `GitExecutionService`
    with the program workspace root as repository root, a 64 KiB
    display cap, and disabled program-call cache. See
    `architecture/tool_programs.md` (Expansion M003).

The same bounded implementation is also available to ordinary model turns as
the deferred direct-only `git_query` facade. Its strict schema cannot express
mutation, recovery, network, or raw-subcommand operations; those remain on
the canonical `git` surface.

13. **RunStore audit-safe rerun argv.** `RerunDescriptor.argv` is
    `Option<AuditSafeArgv>` — always sanitized via URL sanitizer. Raw URL
    reaches Git only ephemerally during execution.

14. **No editor/spawn injection.** `GIT_EDITOR=true`,
    `GIT_SEQUENCE_EDITOR=true`, `EDITOR`/`VISUAL` stripped,
    `GIT_ASKPASS`, `GIT_SSH_COMMAND`, `GIT_PROXY_COMMAND`, all
    `GIT_CONFIG_*` injection vectors stripped.

15. **Recovery not auto-resolved.** Conflicts are presented as typed
    data. The agent must edit files, `git add`, then `recover: continue`.

16. **Staging is private; publication is compare-and-swap.**
    `GitOperation::Add` no longer mutates the repository index in place. It
    stages into a private index in daemon-owned scratch and publishes under
    Git's own `index.lock` with a compare-and-swap against the index
    generation the staging observed. Three rules follow:
    - **No silent fallback.** With no publisher configured, `Add` is
      *refused* (`GitMutationError::PublicationRefused`). Falling back to
      the in-place `git add` would reintroduce exactly the silent overwrite
      and unrecorded intent the vertical exists to remove.
    - **`GIT_INDEX_FILE` stays denied.** `codegg_git::ALWAYS_STRIPPED_ENV_VARS`
      refuses it by design; the private gitdir already provides the
      isolation it would have been used for.
    - **The receipt states one resource.** `PublishedResourceScope::IndexOnly`
      — the index is published, no ref is written, and the worktree is not
      modified. Merge/rebase/checkout and the whole-tree `stage_all` /
      `stage_tracked` routes stay outside this vertical.

17. **Ambiguity is not success.** A publication whose outcome cannot be
    determined from the recorded evidence is reported `Ambiguous` and
    escalated. It is never reported as published and never replayed: a
    committed publication is not re-applied, and an abandoned one is not
    re-staged. A leftover `index.lock` is removed only when its content
    equals the published fingerprint — an empty or foreign lock is left
    alone.

18. **A receipt states its boundary, including "not reported".** The
    publication receipt carries `ObtainedNetworkBoundary`, with a producer for
    every variant: `Enforced` from a launch that observed its own sandbox
    setup completing with the network namespace requested, `Unrestricted` from
    a launch that obtained no network namespace, and `NotReported` from a
    staging step that runs no contained child. A receipt that names no
    boundary and a receipt that names a boundary it did not obtain are both
    wrong; the first is silent, the second is a claim.

## Testing

### Narrowest commands

```bash
# codegg-git crate (parser, operation, risk, path, ref_name, render)
cargo test -p codegg-git

# egggit structured reads + operation state + conflicts
cargo test -p egggit

# Root git modules (mutations, network, recovery)
cargo test -p codegg git_mutations
cargo test -p codegg git_network
cargo test -p codegg git_recovery
cargo test -p codegg git_service

# Integration tests (skip when git unavailable)
cargo test --test git_mutations_integration
cargo test --test git_network_integration
cargo test --test git_recovery_integration
cargo test --test git_execution_origin_matrix

# Private index publication (C007)
cargo test -p codegg --locked --test git_index_publication
```

### Isolated delegated-run commits and integration

Mutation-capable delegated runs are assigned a managed worktree before their
agent loop is constructed. The child tool registry receives that worktree as
its workspace root and applies `ChildGitPolicy::LocalCommitOnly` when the
resolved parent authority includes `GitWrite`. This permits stage/unstage and
local commits in the owned worktree while continuing to reject push, remote and
credential configuration, history rewrites, broad clean/reset, and other
network/destructive operations through their independent policy gates.

Mutation-capable is decided by `requires_isolation`, which asks whether the
agent leaves any mutating tool available: a read-only agent has to deny *all*
of them or it is classified as mutating. The list includes `git`, which can
`commit`/`push`/`reset_hard` — so a read-only agent must deny it, not just
`bash`, unless it declares `git_read_only` (below). A delegated run whose
workspace has **no repository identity** executes
in place instead of failing: there is no worktree to isolate in, and reporting
that as `isolation_unavailable` blocked all mutating subagents outside a repo.
Sandbox containment and the permission layer are orthogonal to isolation and
still apply. A repository that *does* have identity but no available worktree
still fails — that is isolation being unavailable, a different condition.

### Read-only Git (`git_read_only`)

`[agent.permissions]` is keyed by *tool*, so it cannot express "git, but only
to look": `git = "allow"` also authorises `git push`, and `git = "deny"` costs a
read-only agent the repository facts it needs to do its job. `explore`, `plan`,
and `research` therefore declare `git_read_only = true`, which `GitTool`
enforces per invocation (`read_only_only`): an argv `codegg-git` does not
classify read-only is refused with a `ToolError::Permission` naming the agent.

The classifier is `GitOperation::is_read_only()`, which is the typed risk model
(`RiskSet::is_read_only()` — every class is `ReadOnly`, not merely *contains*
it). It answers per **invocation**, which a subcommand-name check cannot:
`branch` lists and `branch -d` deletes, `remote` lists and `remote add` writes
repository config, `tag`/`stash` behave the same way. The previous name-only
list in `GitTool::is_read_only` classified all of those names as read-only,
which is why `git remote add` and `git stash` passed a read-only gate.

An argv the typed parser cannot classify is treated as **not** read-only. A
gate that degrades open on input it does not understand is not a gate.

`requires_isolation` and `has_explicit_git_write` both treat `git_read_only` as
*not* write authority — otherwise the declaration would immediately
reintroduce the isolation regression that `git = "deny"` was added to fix.

Where a decision has only the subcommand name and no argv, the name-level
allow-list `codegg_git::READ_ONLY_SUBCOMMANDS` is deliberately narrower than
what the typed model can decide: it contains only names that are read-only in
*every* modelled form. `read_only_subcommands_stay_read_only` fails if any of
them ever gains a mutating variant, so the list cannot drift into authorising a
mutation. `permission/mod.rs`'s `default_ruleset` builds its auto-Allow rules
from that constant rather than from a hand-maintained name list.

The child result is persisted as a bounded, machine-readable
`codegg_core::run_result::AgentRunResult`. Its base/result commits, changed
paths, repository state, and findings are collected from Git facts rather than
treated as claims in final model prose. A completed child does not mutate the
parent repository. `AgentRunIntegrationService` performs an explicit,
lineage-checked merge, cherry-pick, or rebase through the typed Git mutation
executor and returns a structured success or conflict outcome; failed or dirty
worktrees remain available for inspection under the worktree service's cleanup
policy.

The session projection carries only bounded worktree/run facts needed for
inspection: typed ownership IDs, branch, base/result commit, health,
dirty/conflicted state, validation summary, and retention attention. It does
not expose credentials, full diffs, or authority to integrate; integration
remains an explicit parent-side typed Git operation.

### Test counts (verified 2026-10-19; the C003 rows re-verified 2026-10-23)

Measured from the compiled test binaries' own `--list` output, not from a
source-text grep: `cargo test -p <crate> --lib -- --list` and
`cargo test -p codegg --test <file> -- --list`, counting the `: test` lines. A
grep undercounts, because a test attribute can share a line with others.

| Module / file | #[test] + #[tokio::test] |
|---------------|--------------------------|
| `codegg-git` (parser, operation, risk, path, ref_name, render, process_policy) | 362 |
| `egggit/operation_state.rs` | 11 |
| `egggit` (all `src/*.rs`) | 165 |
| `egggit/conflict.rs` | 7 |
| `tests/command_git/git_mutations_integration.inc` | 12 (tokio::test) |
| `tests/command_git/git_network_integration.inc` | 29 |
| `tests/command_git/git_recovery_integration.inc` | 19 |
| `tests/command_git/git_index_publication.inc` | 21 (C007 private-index publication; 17 C007 + 4 C003 regressions) |
| `tests/command_git/git_read_consistency.inc` | 16 (C003 read-consistency contract and fence) |
| `tests/command_git/git_hostile_metadata.inc` | 6 (C003 hostile-configuration surface) |

The `egggit` rows rose from 109 and 8 on the C003 head: the typed
consistency contract, the configuration-surface detector, and the
trusted-route pre-flight tests are in that crate. The
`git_index_publication` row rose from 17: C003 added the regression tests for
the three C007 invariants that were claimed but untested, plus the
live-class staging refusal.

These rows count the **member** of the consolidated `command_git` family, so
they are smaller than the pre-consolidation per-file counts for the members
that share the family's `common` helper: the 5 shared secret-scan tests live
once in `tests/command_git/secret_scan_self_tests.inc` and are counted
there, not in each member.

### Key test categories

- **codegg-git**: Property-based testing (proptest) for parser. Risk
  classification tests per variant. Path/ref rejection tests.
- **egggit**: Operation state detection for all 9 families. Conflict
  classification, marker detection, binary detection, report aggregation.
- **git_mutations_integration**: Stage/unstage, commit (normal + amend +
  empty), branch create/switch/delete (refuse-current), stash push/apply,
  merge (fast-forward + conflict), rebase, cherry-pick, revert, restore,
  env-policy, projector.
- **git_network_integration**: URL redaction, failure classification,
  remote management, config allowlist, network round-trips, destructive
  reset/clean.
- **git_recovery_integration**: End-to-end on tempdir fixtures — continue
  with conflicts, continue after resolution, abort without state, abort in
  progress, skip during rebase.
- **git_execution_origin_matrix**: Track U routing — verifies
  `route_git_local_mutation` gate behavior.
- **git_index_publication** (C007): the journal's typed state machine and its
  refusal of a terminal transition; the three reconciled verdicts and their
  idempotence across restarts; a concurrent **external** writer producing a
  typed `StaleIndex` conflict without overwriting the writer's index; a held
  `index.lock` producing a typed `IndexLockHeld` conflict; a failed staging
  leaving the repository byte-identical; and the atomic replacement importing
  the staged object. The **contained** staging step additionally needs a
  qualified host — it is exercised by the hosted
  `production_private_index_publisher_stages_and_publishes_under_containment`
  test in `tests/linux_git_test_support/linux_m004_process_ipc.inc`.
- **policy_drift_tests** (root): Ensures canonical lists match Phase F
  entries. `root_and_core_share_canonical_lists` verifies root and
  codegg-core read from same source.

## Related Docs

- `plans/implementation/linux-trusted-git-cost-corrective/007-production-private-index-publication-vertical.md`
  — the C007 plan for the private-index publication vertical, including §9's
  three recorded departures from the plan text.
- `plans/closure/linux-trusted-git-cost-corrective/007-status.md` — the C007
  closure: requirement-to-evidence matrix, the three deviations, and the open
  items (the contained staging step is still unproven on a qualified host).
- `architecture/git_phase_f_handoff.md` — Phase F review context
  (do not modify).
- `architecture/git_polish_verification_handoff.md` — Verified state,
  execution-origin matrix, remaining limitations.
- `docs/validation/git-security-review.md` — Phase F security closure.
- `docs/validation/git-rerun-secret-lifecycle.md` — Rerun argv credential
  lifecycle.
- `scripts/check_git_forbidden_patterns.py` — Enforces `expose_secret()`
  boundary, no hand-maintained env-policy tables, `AuditSafeArgv` usage.
- `architecture/command_intent.md` — Command intent classification and
  routing.
- `architecture/command_routing.md` — Active routing mode.
# Execution source subject capture

`egggit::capture_git_source_subject` is the read-only, governed Git owner for
execution provenance. It captures exact HEAD and a deterministic SHA-256
digest for bounded dirty state without returning paths or file contents.
Capture is rooted at the scheduler's canonical workspace path. Unsafe paths,
non-Git workspaces, and bound/capture failures return typed unavailability;
callers must not downgrade to a weaker status-only identity.

## Source Verification

Verified 2026-10-06 against `crates/codegg-git/src/`, `crates/egggit/src/`,
and the root `src/git_*.rs` / `src/tool/git.rs` modules.
- Corrected the `GitTool` mutation count **40 → 42** (both occurrences).
  The schema `mutation.enum` at `src/tool/git.rs:140`-`:155` holds exactly 42
  entries; `mutation_enum_includes_phase_f_recovery_aliases` cross-checks them.
- Corrected 9 stale `file.rs:line` refs: `GitMutationExecutor` `471`→`477`,
  `GitEnvPolicy` `51`→`49` (a `pub use` re-export of
  `egggit::process::GitEnvPolicy`, not a local definition),
  `NetworkEnvPolicy` `44`→`46`, `PushForce` `207`→`187`,
  `PushRequest` `248`→`228`, `PullStrategy` `155`→`135`,
  `CleanRequest`/`CleanPreview` `733,714`→`713,694`,
  `NetworkFailureKind` `71`→`72`, `RecoveryOutcome` `27`→`28`,
  `GitTool` `14`→`23`.
- Refreshed the test-count table: `codegg-git` 342→358, `egggit` all-`src` 78
  added as a row, `git_network_integration` 13→29,
  `git_recovery_integration` 13→19.
- Confirmed correct as written: `GitOperation` 54 variants, `GitRiskClass` 11,
  `ALLOWED_ENV_VARS` 21, `ALWAYS_STRIPPED_ENV_VARS` 28,
  `NETWORK_ALLOWED_ENV_VARS` 21, `ALLOWED_MERGE_STRATEGIES` 6 (`src/git_mutations_ops.rs:418`),
  `GitPayload` 13, `GitMutationError` 7, `NetworkFailureKind` 7,
  `RecoveryOutcome` 5, `MutationOutcome` 5, `ConflictKind` 8,
  `RepositoryOperationState` 9, `OperationFamily` 9, `PushForce` 3,
  `PullStrategy` 3, and the `codegg-git/src/workflow.rs` refs (`:15`, `:30`,
  `:64`, `:86`).
