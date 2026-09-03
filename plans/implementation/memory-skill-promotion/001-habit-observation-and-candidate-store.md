# Memory-to-Skill Promotion M001 — Habit Observation and Candidate Store

Status: ready

Repository baseline: `1bee32578566cc6cdf4025002af781309d8f29f4`

Source subsystem roadmap:

- `plans/subsystems/memory-skill-promotion-roadmap.md`

Long-term requirements:

- `plans/000-long-term-specification.md#4-architectural-principles`
- `plans/000-long-term-specification.md#12-repository-asset-and-harness-interoperability`
- `plans/000-long-term-specification.md#26-reliability-and-recovery`
- `plans/000-long-term-specification.md#27-security-requirements`
- `plans/000-long-term-specification.md#29-system-invariants`
- `plans/002-long-term-roadmap.md#phase-1--runtime-asset-registry-interoperability-and-refresh-correctness`
- `plans/003-planning-process.md`

Closed/current dependencies:

- runtime-assets and skill-registry foundation is closed;
- `MemoryStore`, pattern consolidation, session/run/tool metadata, and project identity are implemented;
- no new ADR is required because M001 adds internal file-backed observational state without changing canonical project/session identity, scheduler authority, permissions, or an external protocol. Stop if implementation discovers otherwise.

Primary class: infrastructure / privacy invariant

## 1. Objective

Add a host-owned, deterministic, privacy-bounded workflow observation pipeline that can recognize repeated successful project workflows and persist them as habit candidates. M001 must stop at candidate state: no model drafting, no skill proposal, no skill file write, and no runtime asset refresh.

The feature should answer:

> “Have we seen the user successfully follow essentially the same safe workflow often enough, across independent sessions, that it is worth proposing as reusable behavior?”

It must not answer that question by retaining raw commands or asking an LLM to summarize every finished session.

## 2. Explicit non-goals

M001 must not:

- modify `MemoryStore::consolidate_session()` so it consumes arbitrary tool output;
- persist raw Bash/terminal commands, arbitrary tool arguments, raw URLs, environment variables, tool results, prompts, or hidden reasoning;
- call an LLM to classify or name habits;
- create `SKILL.md`, proposal files, scripts, resources, plugins, MCP config, or agent definitions;
- write `.agents`, `.opencode`, `.claude`, or `.codegg/skills`;
- infer a ready habit from one session;
- promote failed/cancelled/no-progress loops as successful habits;
- add a vector database or semantic embedding dependency;
- add a daemon-wide workflow engine;
- add a new CI workflow.

## 3. Current implementation evidence to re-inspect

Before editing, inspect current versions of:

- `crates/codegg-core/src/memory/mod.rs` — project namespace helpers, file locking, temp/rename durability, memory save/load bounds and consolidation lifecycle;
- `crates/codegg-core/src/memory/patterns.rs` — deterministic text-only pattern detection and why tool outputs are intentionally excluded;
- `src/agent/loop.rs` / tool batch execution result path — stable place where canonical tool name, structured execution status, effect class, session/turn identity, and state-change signals are available;
- `src/tool/contract.rs` and tool metadata — safe typed effect/category information;
- supervised test, typed Git, LSP, skill activation, and task/delegation result types for semantic action variants that do not require retaining raw arguments;
- session completion/`AgentFinished` memory auto-consolidation wiring;
- `codegg_core::identity` project/workspace/session/turn/run types.

The implementation should add one bounded observation adapter at an authoritative application boundary rather than having every tool write to the habit store independently.

## 4. Workflow observation contract

### 4.1 Safe action vocabulary

Add a serializable bounded action enum/record under `codegg-core::memory` or a dedicated adjacent `habit` module, for example:

```rust
struct WorkflowAction {
    kind: WorkflowActionKind,
    variant: Option<String>,
    effect: WorkflowEffectClass,
}

enum WorkflowActionKind {
    FileRead,
    Search,
    Edit,
    Patch,
    Test,
    Lint,
    Build,
    Format,
    GitRead,
    GitWrite,
    LspRead,
    LspRefactor,
    SkillActivate,
    Delegate,
    DeterministicValidate,
    ShellExec,
}
```

Exact variants should reuse existing enums where practical. The important requirement is that persisted action identity is safe structural metadata.

Allow safe variants only when the owning subsystem already exposes a bounded enum/name, for example:

- supervised test kind `test|lint|build|format`;
- typed Git operation class/subcommand enum;
- LSP action name;
- canonical skill name after normal validation;
- delegated agent name after existing validation;
- canonical deterministic tool name.

For generic `bash`/terminal, persist only `ShellExec` in M001. Do not include command text, executable/argv, current directory, env, stdout/stderr, or a hash of raw command text. Hashing sensitive raw input does not make it appropriate durable habit data.

For generic native tools, do not store complete JSON arguments. If a safe variant cannot be derived from typed host metadata, leave `variant=None`.

### 4.2 Observation envelope

At a stable execution boundary, construct:

```rust
struct WorkflowObservation {
    project_namespace: String,
    session_id: String,
    turn_id: Option<String>,
    root_or_run_id: Option<String>,
    action: WorkflowAction,
    outcome: WorkflowOutcome,
    occurred_at: i64,
}

enum WorkflowOutcome {
    Succeeded,
    Failed,
    Cancelled,
    TimedOut,
}
```

The persisted candidate store does not need to keep every observation forever. The observation is input to a bounded aggregator; retain only the minimum evidence needed to establish distinct occurrence/session counts and last-seen provenance.

If exact session/run IDs are sensitive for a future team protocol, store existing opaque typed IDs as CodeGG already does, not user prompt text or paths.

### 4.3 Stable collection point

Prefer one adapter fed by existing tool execution/checkpoint/event structures. It should observe accepted completed tool calls after canonical name/effect/status normalization.

Do not add a habit-store dependency to every individual tool implementation.

Candidates should be finalized/aggregated at a logical workflow boundary such as root `AgentFinished`, explicit session consolidation, or a bounded turn-completion hook. Avoid writing the store for every streaming token/tool event.

## 5. Workflow occurrence construction

### 5.1 Sequence normalization

For one completed logical turn/run segment:

1. retain only safe allowlisted structural actions;
2. drop failed/cancelled actions from a successful workflow skeleton or record them only as bounded negative evidence if useful;
3. collapse immediate identical no-op/read repetitions where existing progress fingerprints classify them as equivalent;
4. cap sequence length (recommended <=32 actions);
5. require a minimum useful shape (recommended >=2 semantically distinct actions) before automatic habit tracking;
6. normalize to a deterministic versioned representation;
7. hash with a domain separator and project scope.

Representative fingerprint input:

```text
codegg-habit-v1\0
project/<namespace>
read|search|edit|test:test|git_read:diff
```

Do not use filesystem paths as habit identity.

### 5.2 Success definition

A successful occurrence requires the enclosing operation to have a reliable successful terminal signal and no known failed/cancelled terminal status. Prefer root/turn completion plus structured test/run outcomes where available.

Do not mark “model stopped talking” as success by itself.

If a turn produced mutations but ended stalled, cancelled, or with a failed supervised validation recorded as part of the same operation, it must not increment the successful occurrence count. It may update `last_seen`/negative count if that helps diagnostics, but keep the first implementation simple.

### 5.3 Independent occurrence rule

Automatic readiness needs independent repetitions. At minimum:

- repeated identical action loops within one turn collapse to one observation;
- multiple turns in one session may increase raw occurrence count but cannot satisfy the distinct-session floor alone;
- `Ready` requires observations from at least two different session IDs;
- default ready threshold is at least three successful occurrences across at least two sessions.

Expose configuration only if current config conventions make it useful. A hard minimum of two sessions must remain non-configurable or clamped upward.

## 6. Habit candidate domain

Add typed bounded candidate state, approximately:

```rust
enum HabitCandidateStatus {
    Observing,
    Ready,
    Dismissed,
    Promoted,
    Superseded,
}

struct HabitCandidate {
    id: HabitId,
    project_namespace: String,
    workflow_version: u16,
    workflow_fingerprint: String,
    actions: Vec<WorkflowAction>,
    successful_occurrences: u32,
    distinct_sessions: u32,
    recent_session_ids: Vec<String>,
    first_seen: i64,
    last_seen: i64,
    status: HabitCandidateStatus,
    related_memory_ids: Vec<String>,
    promoted_skill: Option<PublishedSkillRef>,
}
```

M001 may leave `related_memory_ids` and `promoted_skill` empty but should reserve typed bounded fields if doing so does not create unnecessary compatibility burden.

Candidate names/descriptions should initially be host-generated neutral summaries such as `read -> edit -> test -> diff`; do not invoke a model to create a catchy title.

Hard bounds:

- maximum actions per candidate;
- maximum retained recent session IDs;
- maximum candidates per project namespace;
- maximum string/variant bytes;
- maximum file size and decoded record count.

Use LRU/oldest-observing pruning only for non-ready/non-promoted candidates when limits are reached. Never silently discard a promoted provenance link merely to admit a new low-confidence observation.

## 7. Candidate lifecycle semantics

### Observing -> Ready

Transition deterministically when the configured threshold is met, clamped to the hard distinct-session minimum.

### Ready -> Dismissed

User dismissal is explicit and stores the fingerprint/version so the identical workflow does not immediately return.

### Dismissed -> Ready

Do not automatically reopen the same fingerprint. A materially changed normalized workflow creates a different fingerprint/candidate. A future explicit “restore” operation may reopen a dismissed candidate but is not required in M001.

### Ready -> Promoted

M001 does not perform promotion but the store API may expose a host-only transition reserved for M003. Do not mark promoted from model input.

### Supersession

If normalization format changes or a new workflow version intentionally replaces an old candidate, mark old record `Superseded` with a bounded link if available. Do not silently mutate an old fingerprint's meaning.

## 8. File-backed store

### 8.1 Location

Keep state under the CodeGG config/memory ownership tree, not in the repository itself. A preferred shape is:

```text
<config>/codegg/memory/habits/project/<sha256-namespace>.json
```

Reuse the same project namespace helper/domain identity as `MemoryStore`; do not derive a second truncated project hash.

### 8.2 Durability

Implement:

- safe namespace/path construction;
- advisory lock at an appropriate shared or per-habit-file scope;
- write complete JSON to a temp file;
- flush + `sync_all`;
- atomic rename;
- cleanup/recovery of stale temp file where safe;
- bounded load with explicit diagnostics for malformed/oversized content.

If practical, extract a tiny internal atomic-file helper shared with `MemoryStore` so the locking/rename behavior does not drift. Keep that refactor bounded and test both memory and habit persistence afterward.

### 8.3 Store API

Required operations:

- observe/merge one normalized successful occurrence;
- get/list candidates by project and status with hard result caps;
- dismiss candidate;
- host-only mark promoted/superseded for later milestones;
- save/load/reload;
- optional `clear` only if existing memory UX has an equivalent explicit operation.

Do not expose a generic mutable `update(candidate)` that bypasses lifecycle validation.

## 9. Integration with text memory

Do not merge `HabitCandidate` into `Memory`. They have different semantics:

```text
Memory
  -> human-readable durable preference/convention
  -> may enter model context

HabitCandidate
  -> structural repeated-workflow evidence
  -> does not enter model context automatically
```

M001 may associate a habit with existing memories using safe keyword/action matches, but this is optional and must not block closure. The important compatibility rule is that existing `MemoryStore`, `MEMORY.md`, consolidation scores, `/memory-*` commands, and prompt injection do not change behavior.

Do not inject all habit candidates into the system prompt. They are inspectable product state, not ambient instructions.

## 10. Configuration and user surface

Prefer a small experimental/feature config if needed, for example:

```jsonc
{
  "experimental": {
    "habit_candidates": false
  }
}
```

Default may remain disabled for the first milestone if the current project prefers staged opt-in. If enabled by default, observation still must be privacy-bounded structural data and should have a clear command to inspect/delete candidates.

Add discoverable read/dismiss operations such as:

```text
/habits
/habits ready
/habit <id>
/habit-dismiss <id>
```

Exact command names should follow current TUI command conventions. M001 UI must not promise skill creation yet; it may label ready state as “eligible for skill proposal in a later milestone.”

If protocol projection is added, publish only bounded metadata (id/status/action summary/counts/timestamps), not entire session histories.

## 11. Expected production-code touch set

Expected core/application areas:

- `crates/codegg-core/src/memory/` plus new `habit.rs`/`workflow.rs` modules or equivalent;
- shared safe atomic-file helper only if justified;
- root/application turn/tool observation adapter;
- session/agent-finished consolidation hook;
- `crates/codegg-config/src/schema.rs` only if enablement/threshold config is exposed;
- TUI commands/tasks for list/show/dismiss;
- projection types only if current command flow requires daemon-provided summaries;
- `architecture/memory.md`, `architecture/config.md` if applicable.

Do not touch `src/skills` publication/write paths in M001.

## 12. Required tests

### Privacy/safe vocabulary

- Bash commands with obvious secrets/tokens/paths never appear in `WorkflowAction` or persisted habit file;
- arbitrary tool JSON arguments/results are not serialized;
- hidden reasoning/message text does not enter observations;
- safe typed test/Git/LSP/skill/delegation variants are retained as intended;
- unknown tools degrade to a coarse safe class or are ignored rather than storing raw names/arguments if the name itself is untrusted/unbounded.

### Fingerprint normalization

- identical safe workflow in two sessions yields same fingerprint;
- action-order change yields different fingerprint where order is semantically meaningful;
- repeated immediate no-op duplicates normalize consistently;
- workflow version/domain separator changes are explicit;
- different projects do not collide even for identical action sequences.

### Candidate thresholds

- one session cannot reach `Ready` regardless of repeated loops;
- three successes across two sessions reaches default `Ready`;
- failed/cancelled/stalled operation does not increment successful count;
- duplicate observation identity in one completed turn is idempotent;
- dismissal suppresses identical fingerprint;
- changed fingerprint becomes a new observing candidate.

### Store

- bounded load/save round trip;
- concurrent writers through lock do not truncate/corrupt records;
- temp+rename crash-safety helper tests where current memory tests support it;
- malformed/oversized file produces diagnostics/fails safely rather than allocating unbounded data;
- project namespace traversal impossible;
- candidate-count pruning preserves ready/promoted state according to policy.

### Regression

- existing `cargo test -p codegg-core memory` remains green;
- existing `MEMORY.md` output is unchanged for identical inputs;
- no habit data is injected into prompt memory summary.

## 13. Verification commands

Required after implementation:

```bash
cargo fmt --all -- --check
git diff --check
cargo test -p codegg-core memory --locked
cargo test -p codegg-core habit --locked
```

Run the focused TUI/command test target if commands are added.

Then:

```bash
scripts/verify.sh quick
```

No model/network call is needed for M001 verification.

## 14. Acceptance criteria

M001 may close only when:

1. Automatic workflow observations contain only allowlisted safe structural metadata.
2. Raw command text, tool arguments/output, prompts, environment data, secrets, and hidden reasoning are excluded by construction.
3. One authoritative application boundary feeds observations rather than per-tool habit-store writes.
4. Workflow normalization/fingerprinting is deterministic, versioned, project-scoped, and bounded.
5. A candidate cannot become `Ready` from one session; default readiness requires at least three successful occurrences across at least two sessions.
6. Failed/cancelled/stalled operations do not increase successful habit confidence.
7. Candidate lifecycle supports observing/ready/dismissed plus reserved host transitions for promoted/superseded.
8. Dismissed identical fingerprints do not immediately reappear.
9. File-backed persistence is locked, atomic, bounded, path-safe, and restart-safe.
10. Existing text-memory consolidation/prompt behavior remains unchanged.
11. No model call or skill write occurs in the habit pipeline.
12. User can inspect and dismiss candidates through a discoverable bounded surface.
13. Architecture/config documentation is current.
14. Focused tests and `scripts/verify.sh quick` pass.

## 15. Stop conditions

Stop and register a follow-up rather than broadening M001 if:

- useful habit identity requires storing raw shell commands or arbitrary tool arguments;
- candidate durability requires moving memory ownership into SQLite/a new daemon database subsystem;
- exact observation semantics require invasive rewrites of every tool;
- the feature cannot avoid automatic model calls at session end;
- supporting team/global habits requires changing principal/project authorization before a safe project-local slice can land.

## 16. Closure evidence required

Create `plans/closure/memory-skill-promotion/001-status.md` with:

- implementation revision;
- safe observation field inventory and explicit excluded-data evidence;
- fingerprint/threshold/idempotency tests;
- persistence/lock/atomicity/path-bound evidence;
- regression result for existing memory behavior;
- TUI/command inspection evidence if added;
- focused and quick verification results;
- unresolved findings and closure recommendation.

Only after accepted M001 closure should M002 move to `ready` in the registry.
