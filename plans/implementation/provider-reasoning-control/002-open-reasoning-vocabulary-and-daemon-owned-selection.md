# Provider Reasoning Control Milestone 002 — Open reasoning vocabulary and daemon-owned selection

Status: closed (M001 closed; ADR-0044 accepted 2026-10-21; closure record `plans/closure/provider-reasoning-control/002-status.md`)

Repository baseline: `c932115421c4f26ca3c9c76ff134754ae4a6ceb5` (branch `feat/auto-20261021-d6b5ffc5`)

Source roadmap:

- `plans/subsystems/provider-reasoning-control-roadmap.md#milestone-2--open-reasoning-vocabulary-and-daemon-owned-selection`

Long-term requirements:

- `plans/000-long-term-specification.md#11-daemon-owned-provider-connections-and-eggpool` — provider-facing request policy is CodeGG-owned.
- `plans/000-long-term-specification.md#24-protocol-and-storage-requirements` — protocol operations are versioned and capability-negotiated; new clients tolerate unknown optional fields.
- `plans/000-long-term-specification.md#26-reliability-and-recovery` — daemon restart recovers or deterministically interrupts sessions and turns.

Applicable ADRs:

- `plans/adrs/ADR-0044-open-provider-scoped-reasoning-control-vocabulary.md` (§4 the open vocabulary, §5 the daemon-owned selection)

Primary class: capability

## 1. Objective

Two coupled outcomes in one coherent contract:

1. The reasoning effort becomes an **open, provider-scoped label** rather than a closed four-value enum, so a vendor label such as `xhigh` or `max` is selectable and reaches the provider verbatim.
2. The selection moves from **frontend-local state into a daemon-owned, principal-scoped, durable preference**, so every daemon turn-admission path resolves it from one place instead of each path having to carry it.

Either outcome alone leaves the control broken: an open vocabulary with a frontend-local selection still vanishes on the three turn-reconstructing paths and on a reconnect, and a daemon-owned selection of a closed vocabulary still cannot express a vendor label.

## 2. Why this milestone is ready

- **Hard dependency:** M001 closed. M001 restores the wire-intent fidelity this milestone's open labels depend on; without it, a new label would still be dropped at the encoder on the surfaces that cannot express effort.
- **ADR:** ADR-0044 accepted, and specifically its §5 (daemon-owned selection). See §14.
- **Interface dependencies, both already stable in the repository:**
  - `runtime_preferences` (`crates/codegg-core/src/approval.rs`, table created at migration v58, extended additively at v62) is an existing daemon-owned, principal-scoped, durable, revision-gated preference store with an established store API and an established protocol seam. This milestone adds a column and a store method to an existing table; it does not introduce a new store.
  - The daemon's `*Capabilities` request family (`ProjectionCapabilities`, `AuditCapabilities`, `ChatCapabilities`, and the others in `src/core/daemon_family.rs`) is the established capability-negotiation pattern, and the canonical specification requires capability-negotiated protocol behaviour.
- **Predecessor roadmaps:** the closed provider-wire-kernel consolidation roadmap (single encoder; provider-facing policy stays in CodeGG) and the closed model-adaptation roadmap (the adapter registry is the single compiled model-policy authority). Both are preserved.

## 3. Current implementation evidence

Repository baseline `c93211542`.

**The vocabulary is closed.** `crates/codegg-protocol/src/core.rs`:

```rust
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ReasoningEffort { Minimal, Low, Medium, High }
```

`ReasoningOverride::Effort { level: ReasoningEffort }` carries it, and `ReasoningMode::cycle()` reaches only `default → medium → high → off → default`. `Minimal`, `Low`, and `Budget` are unreachable from the TUI command; `ReasoningMode::cycle()` maps `Budget(_)` straight to `Off` and never enters `Budget`.

**The configuration layer is already open.** `crates/codegg-config/src/schema.rs` has `ModelProfileConfig::default_reasoning_effort: Option<String>` — a free-form string with no enumeration — and `crates/codegg-core/src/model_profile/types.rs` carries it through as `default_reasoning_effort: Option<String>`. So a user can already write `xhigh` into config today; it reaches the request and is then dropped by the surfaces that cannot express effort.

**The selection is frontend-local.** `src/tui/app/mod.rs` holds the current mode in `self.ui_state.reasoning_mode` and maps it into the turn payload; the mode's `label()` and `detail()` appear nowhere in the render path, so the mode is surfaced only in the transient toast emitted by the command.

**Three daemon paths drop the selection.** Each reconstructs a turn with the field set to `None`:
- `src/core/daemon_queue_delivery.rs` — the user-prompt-queue delivery path;
- `src/core/daemon_continuation.rs` — the idle-continuation path;
- `src/core/daemon_turns.rs` — the `SessionPromptSubmit` path.

The `SessionPromptSubmit` protocol variant additionally carries **no reasoning field at all**, so the desktop frontend cannot express one. `crates/codegg-client/src/compose.rs` hardcodes `reasoning: None` when composing a turn.

**The store that fits already exists.** `runtime_preferences` is created at migration v58 and extended additively at v62:

```sql
CREATE TABLE IF NOT EXISTS runtime_preferences (
    principal_id TEXT PRIMARY KEY,
    approval_mode TEXT CHECK (...),
    sandbox_profile TEXT CHECK (...),
    last_provider_connection_id TEXT,
    last_model_id TEXT,
    revision INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
)
```

with `last_task_provider_connection_id` / `last_task_model_id` added by `migrate_v62` as nullable `ALTER TABLE … ADD COLUMN`. `RuntimePreferenceStore` (`crates/codegg-core/src/approval.rs`) is the daemon-owned, principal-scoped wrapper, and `architecture/session.md` records its documented precedence: *"explicit turn overrides win, then project ceiling, then persisted, then `Interactive`/`WorkspaceWrite` defaults."* The protocol seam is `ApprovalPreferenceGet`, `ApprovalModeSet`, `SandboxProfileSet`, `RuntimePolicySet`, `TaskModelPreferenceSet`, `ExecutionPolicyGet` — principal derived server-side, payloads carrying no identity — dispatched in `src/core/daemon_ops.rs` and classified in `src/core/daemon_family.rs`.

**The storage-layout guard is strict.** `STORAGE_LAYOUT_VERSION` is `83` (`crates/codegg-core/src/storage/mod.rs`), the highest wired migration is `migrate_v83` (`crates/codegg-core/src/session/schema.rs`), and `scripts/check_project_catalog_invariants.py` requires the constant to equal the highest wired migration with a contiguous chain. A new migration therefore requires a version bump.

**The compatibility hazard is concrete.** If the protocol's effort type becomes a free-form string, a **new** client can send a label an **old** daemon cannot deserialize. The old daemon's `ReasoningOverride` deserialization would fail, failing the whole turn request rather than degrading. The repository already has a capability-negotiation mechanism for exactly this class of problem.

## 4. Invariants that must not regress

- CodeGG runs against direct providers without requiring an EggPool process.
- Provider credentials and endpoint/session-affinity policy remain CodeGG-owned and never enter `eggpool-wire`.
- One CodeGG semantic request is encoded once into the selected target grammar.
- Private reasoning history is never returned to a provider unless the resolved CodeGG model/wire policy explicitly allows that provider-private round trip.
- Provider/model-specific transforms have one CodeGG authority.
- The shared-kernel dependency is immutable-pinned.
- **The existing `runtime_preferences` semantics are untouched.** The reasoning selection is a **separate store method with its own column**, exactly as the Task-composer model scope was added at v62. The approval-mode and sandbox-profile columns, their `CHECK` constraints, their revision semantics, and their existing write paths are not modified, and a policy write preserves the reasoning selection and vice versa.
- **The preference carries no authorization meaning.** It grants no permission, tool, path, or credential scope, and its protocol operation is classified with the same `Global` scope and no capability as the other principal-scoped preference operations.
- **The principal is derived server-side from transport authority.** The payload carries no identity.
- **Frontends remain projections.** A frontend's local mode is a display of daemon state, never the authority.
- The existing user-prompt-queue ownership, delivery, and wakeup authority rules (ADR-0035, ADR-0037, ADR-0038, ADR-0039) are untouched. This milestone does not change who delivers a queued prompt or when; it changes where the turn's reasoning selection comes from.

## 5. Scope

### In scope

- The protocol's reasoning effort type becomes an open, provider-scoped label.
- The daemon-owned, principal-scoped, durable selection: a column on `runtime_preferences`, a store method, a protocol request/response pair, and a daemon handler.
- The turn-admission resolution point: explicit per-turn override → persisted principal preference → agent/model-profile default.
- The resolution applied at every turn-admission path, including the three that currently drop the field.
- Capability negotiation so a client never sends a non-enumerated label to a daemon that cannot decode it.
- The additive migration and the `STORAGE_LAYOUT_VERSION` bump.
- The frontend's `/reasoning` command writing the selection to the daemon and reading it back.

### Explicitly out of scope

- **The capability record.** Which labels a model accepts, and whether reasoning can be disabled, is M003's subject. This milestone makes a label *expressible*; it does not make it *correct for the model*.
- **The frontend option surface.** Which labels the UI offers, and the persistent working-strip presentation, is M004's subject. This milestone makes the selection durable and shared.
- **The wire-intent fidelity.** M001's subject.
- **The discovery overlay.** M005's subject.
- **Any change to the agent-layer precedence order** between agent config, model-profile defaults, and the selection. The selection enters the existing chain as the explicit-override layer it already is; the chain's order is unchanged.
- **Any change to the queue's ownership, delivery, or wakeup authority.** The queue's *entries* gain no reasoning column; the delivered turn reads the daemon-owned selection.
- **Any change to the `eggpool-wire` pin.**
- **A general per-model metadata expansion.**

## 6. Required production changes

### Core/domain

- `crates/codegg-protocol/src/core.rs` — the reasoning effort becomes an open, provider-scoped label carried as a string. The **serialized shape for the four known labels must stay identical** (`{"mode":"effort","level":"medium"}`), so an existing payload still decodes. The known labels remain recognized as a convenience set for the fallback ordering when no per-model capability is available (M003's subject); an unrecognized label is preserved verbatim, never rejected for being unrecognized, and never translated.
- `crates/codegg-core/src/approval.rs` (or a sibling module in `codegg-core`) — a `RuntimePreferenceStore` method for the reasoning selection, following the Task-composer scope's shape: its own column, its own read, its own write, its own bounded value, and a revision-gated CAS. The value is bounded (a label length bound and an allowed-shape check) and is validated on write, not trusted verbatim.
- `crates/codegg-core/src/session/schema.rs` — a new migration adding one **nullable** column to `runtime_preferences`, following the `migrate_v62` pattern exactly. Existing rows are untouched and read as "no explicit selection".
- `crates/codegg-core/src/storage/mod.rs` — `STORAGE_LAYOUT_VERSION` bumped to match the new highest wired migration, as the project-catalog invariants guard requires.

### Storage and migrations

- Exactly one additive nullable column on `runtime_preferences`, one new migration, one `STORAGE_LAYOUT_VERSION` bump. No other table is touched. `cached_models` is **not** touched (that is M005's optional subject). The `user_prompt_queue_entry` table is **not** touched — the queue's delivered turn reads the daemon-owned selection rather than carrying a per-entry copy.

### Protocol and DTOs

- A new principal-scoped request/response pair for reading and writing the selection, following the established `*Preference*` shape: the principal is derived server-side, the payload carries no identity, and the response is tolerant of absence. The new request is classified in `src/core/daemon_family.rs` with the same `Global` scope and no capability as the other principal-scoped preference operations, and its operation descriptor is registered in `crates/codegg-core/src/authorization/policy.rs`.
- The daemon's capability advertisement gains an entry for open-label support, in the established `*Capabilities` family. A client that has not observed that capability MUST NOT send a non-enumerated label.
- `CoreRequest::TurnSubmit.reasoning` keeps its `#[serde(default)]` tolerance: an absent field continues to mean "no explicit per-turn override".

### Runtime and concurrency

- A single **turn-admission resolution point** in the daemon, invoked by every path that admits a turn. It resolves the effective selection as: the turn's explicit per-turn override → the persisted principal preference → no selection (which lets the agent/model-profile defaults apply unchanged).
- The three paths that currently set `reasoning: None` (queue delivery, idle continuation, `SessionPromptSubmit`) resolve through that point rather than hard-coding `None`. `SessionPromptSubmit` does **not** need a new protocol field: it reads the persisted selection, which is the point of daemon ownership.
- The frontend's `/reasoning` command writes the selection through the new request and updates its local display from the daemon's response. A write failure is reported to the user rather than silently leaving the local display out of sync.
- The write is revision-gated, so a concurrent write is a typed conflict rather than a silent last-write-wins.

### Frontend or operator surface

- The `/reasoning` command continues to cycle, but the cycle is bounded by what the client may send: an advertised-capability client may cycle any label the model offers (M004's subject for the full option surface); a client without that capability cycles only the known labels. The command's transient toast behaviour is unchanged in this milestone.

### Security and authorization

- The selection is **request-shaping configuration, not a permission**. It grants no permission, tool, path, or credential scope, and its protocol operation carries the same `Global` scope and no capability as the other principal-scoped preference operations.
- The principal is derived server-side from transport authority; the payload carries no identity and a client cannot write another principal's selection.
- The stored label is bounded and validated on write. A value that is not a plausible label shape is rejected at the store boundary, not forwarded.

### Documentation and static guards

- `architecture/provider.md` — the "Extended-thinking control (`/reasoning`)" section records the open vocabulary and the daemon-owned selection with its precedence.
- `architecture/session.md` — the `runtime_preferences` section records the new column, its store method, and the precedence.
- `architecture/authorization.md` — the operation table records the new operation's scope and capability.
- `architecture/protocol.md` — the request/response lists gain the new pair and the capability entry.
- A static guard is added that asserts the protocol carries no closed reasoning-effort enum, so the vocabulary cannot be re-closed. It is registered in `scripts/verify.sh quick` **and** in the CI `verify` job, and carries a `--self-test` that injects a closed enum and proves the guard rejects it.

## 7. Ordered work packages

### Work package A — Open the protocol label

Intent: Replace the closed effort enum with an open, provider-scoped string that is wire-compatible for the known labels.

Required changes:
- Change the protocol's reasoning effort representation to an open string. The known labels are recognized as a convenience set; an unrecognized label is preserved verbatim.
- Keep the serialized shape for the four known labels byte-identical.
- Update every consumer of the closed enum — the protocol's own `ReasoningMode`/`ReasoningOverride`, the agent layer's `apply_reasoning_choice`, the TUI's mode mapping, and the configuration path.

Acceptance evidence:
- A test decoding the legacy payload `{"mode":"effort","level":"medium"}` and asserting it still yields the medium label.
- A test decoding `{"mode":"effort","level":"xhigh"}` and asserting the label is preserved **verbatim** — not normalized, not rejected, not mapped to a known label.
- A test asserting the known labels' ordering helper still produces the legacy ladder for the fallback cycle.
- A test asserting no path converts a label into a different label or into a token budget.

### Work package B — Add the daemon-owned durable selection

Intent: Give the daemon a principal-scoped, durable, revision-gated home for the selection.

Required changes:
- The nullable column on `runtime_preferences`, the additive migration, and the `STORAGE_LAYOUT_VERSION` bump.
- The store method: read, write, bounded value, revision-gated CAS. It does **not** modify the approval-mode, sandbox-profile, or model-preference columns or their write paths, and a write to one scope preserves the others — the same separation `migrate_v62` established for the Task-composer scope.
- The protocol request/response pair and the daemon handler, with the principal derived server-side.
- The operation descriptor and its `daemon_family` classification.

Acceptance evidence:
- A storage test asserting the migration preserves every existing `runtime_preferences` row and that a pre-migration database opens cleanly with the new column absent-valued.
- A storage test asserting the approval mode, sandbox profile, and last-used model survive a reasoning-selection write unchanged, and that a reasoning write does not disturb the preference's revision semantics for those scopes.
- A test asserting a stale-revision write is a typed conflict, not a last-write-wins.
- A test asserting the stored label is bounded and that an over-long or malformed value is rejected at the store boundary.
- A test asserting the project-catalog invariants guard passes at the new layout version, and that the migration chain is contiguous.

### Work package C — Resolve the selection at turn admission

Intent: Make every turn-admission path resolve the selection from one place.

Required changes:
- The turn-admission resolution point, with the precedence: explicit per-turn override → persisted principal preference → no selection.
- The three paths that currently hard-code `reasoning: None` resolve through it.
- The `SessionPromptSubmit` path resolves the persisted selection without a new protocol field.

Acceptance evidence:
- A test per path asserting the resolved selection: the queue delivery path, the idle-continuation path, the `SessionPromptSubmit` path, and the ordinary direct submit path.
- A test asserting an explicit per-turn override beats the persisted preference, and that the persisted preference beats the absence of a selection.
- A test asserting that with no selection and no override, the agent/model-profile defaults apply exactly as at the baseline.
- A restart test asserting the persisted selection is reloaded and applied after a daemon restart.

### Work package D — Negotiate the open vocabulary

Intent: Stop a new client from sending a label an old daemon cannot decode.

Required changes:
- The daemon's capability advertisement gains an open-label entry, in the established `*Capabilities` family.
- The frontend consults that capability before sending a non-enumerated label, and falls back to the known ladder when it is absent.

Acceptance evidence:
- A test asserting a client that has not observed the capability sends no non-enumerated label.
- A test asserting a daemon that has not advertised the capability and receives a non-enumerated label **fails that request with a typed error** rather than failing the whole daemon or silently mis-decoding.
- A compatibility test decoding the legacy `ReasoningOverride` payloads for all three legacy variants.

### Work package E — Wire the frontend and reconcile the documentation

Intent: Make the selection durable and shared, and make the documentation describe it.

Required changes:
- The `/reasoning` command writes the selection through the new request and updates its local display from the daemon's response; a write failure is reported to the user.
- The native client (`crates/codegg-client`) needs no change to *inherit* the daemon-owned selection — it composes a turn with no explicit override and the daemon resolves the persisted preference. It needs a change only if it is to express a per-turn override.
- The documentation updates listed in §6.
- The static guard against a re-closed vocabulary.

Acceptance evidence:
- A test asserting a failed write is surfaced to the user and the local display is not left claiming a selection the daemon rejected.
- The static guard passes on the tree and rejects an injected closed enum.

## 8. Failure, cancellation, restart, and contention semantics

- **Partial failure.** The migration is a single additive statement; a failure leaves the table as it was. A failed preference write is reported (`user_choice_unpersisted`-style, matching the existing preference seam) and does not leave the frontend claiming a selection the daemon rejected.
- **Cancellation.** The selection is not turn-scoped state, so a cancelled turn leaves it untouched. A cancelled turn's explicit override is discarded with the turn.
- **Restart.** The persisted selection reloads on restart with the same properties as the existing `runtime_preferences` rows. A turn admitted before a restart resolves the same selection after it, because the resolution is against daemon state rather than in-memory process state.
- **Duplicate delivery.** A redelivered turn resolves the selection again; the resolution is idempotent for a given turn and a given persisted value.
- **Contention.** The write is revision-gated, so two concurrent writes produce a typed conflict rather than a silent last-write-wins. A turn admitted concurrently with a selection write resolves against a consistent read of the preference row; the revision carried by the turn's resolution identifies which value it used.
- **Stale generations.** A turn admitted against an older preference revision uses the value it resolved and records it; it does not silently re-resolve mid-turn.

## 9. Compatibility and migration

- **Protocol — the effort label.** The serialized shape for the four known labels is unchanged. A new client against an old daemon that sends a **known** label is fine. A new client that sends a **non-enumerated** label to an old daemon would fail that daemon's deserialization, so the client MUST consult the capability advertisement first. This is the compatibility requirement that Work package D exists to satisfy, and it is a hard requirement, not a nicety.
- **Protocol — the new request/response pair.** Additive. A client that does not know it is unaffected. A new client against an old daemon receives no selection and falls back to the configured default, which it must state rather than present as the model's full option set.
- **Protocol — `CoreRequest::TurnSubmit.reasoning`.** Unchanged in shape; still `#[serde(default)]`, so an absent field continues to mean "no explicit per-turn override".
- **Storage.** One additive nullable column, one migration, one `STORAGE_LAYOUT_VERSION` bump. Existing rows read as "no explicit selection", preserving current behaviour exactly. The migration is restart-safe and preserves every existing row, as `migrate_v62` did on the same table.
- **Configuration.** `default_reasoning_effort` and `default_thinking_budget` keep their types and meaning. A configured vendor label now reaches the wire instead of being discarded, which changes behaviour from "silently dropped on some surfaces" to "sent as selected" — the intent of the change, and it must be called out in release notes.
- **Operator-visible behaviour.** The selection becomes **shared** across frontends on the same daemon, and it survives a reconnect. A user who assumed the control was private to their frontend sees a behaviour change. This is the intent of daemon ownership and is called out in release notes.
- **No removal criteria are triggered by this milestone** beyond the internal removal of the closed enum, which is complete within the milestone.

## 10. Required tests

### Focused unit tests

- The protocol's open label preserves an unrecognized label verbatim.
- The legacy payload `{"mode":"effort","level":"medium"}` still decodes to the medium label.
- The known-label ordering helper still yields the legacy ladder.
- The agent-layer precedence is unchanged: an explicit selection beats agent config and model-profile defaults, and effort and budget remain mutually exclusive.
- The stored label is bounded and a malformed value is rejected at the store boundary.

### Integration tests

- The turn-admission resolution, exercised per path: direct submit, queue delivery, idle continuation, and `SessionPromptSubmit`.
- The selection write/read round trip through the daemon, with the principal derived server-side.
- The capability negotiation: a client without the capability sends no non-enumerated label; a daemon without the capability refuses one with a typed error.
- The frontend write path: a successful write updates the local display from the daemon's response; a failed write is surfaced.

### Restart and recovery tests

- The persisted selection survives a daemon restart and is applied to the next admitted turn.
- A pre-migration database opens cleanly after the migration, with the new column absent-valued and the existing columns intact.

### Contention and cancellation tests

- A stale-revision write is a typed conflict, not a last-write-wins.
- A cancelled turn leaves the persisted selection untouched.
- Two concurrent selection writes produce exactly one success and one typed conflict.

### Security and negative tests

- A client cannot write another principal's selection; the payload carries no identity and the principal comes from transport authority.
- The selection grants no permission, tool, path, or credential scope — asserted as a negative over the operation descriptor and the resolution path.
- A non-enumerated label sent to a daemon that has not advertised the capability is refused with a typed error and does not fail the daemon.

### Migration and compatibility tests

- The migration preserves every existing `runtime_preferences` row.
- A reasoning write preserves the approval mode, sandbox profile, and last-used model.
- The project-catalog invariants guard passes at the new `STORAGE_LAYOUT_VERSION`, and the migration chain is contiguous.
- All three legacy `ReasoningOverride` payload variants still decode.

## 11. Required verification commands

```bash
# narrow tests first
cargo test -p codegg-protocol
cargo test -p codegg-core approval::tests
cargo test -p codegg-core storage::tests

# the new static guard, including its self-test
python3 scripts/check_reasoning_vocabulary_open.py
python3 scripts/check_reasoning_vocabulary_open.py --self-test

# the storage-layout guard
python3 scripts/check_project_catalog_invariants.py

# the guard must be wired into the routine suite
bash scripts/verify.sh quick

# formatting and linting
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings

# broader suite appropriate to the change
cargo test -p codegg-core -p codegg-protocol -p codegg-providers
```

Do not claim commands that were not actually run in the closure record. The repository's existing verification policy applies: no new CI lane, no coverage or benchmark gate, no permanent external service.

## 12. Documentation updates

- `architecture/provider.md` — the "Extended-thinking control (`/reasoning`)" section: the open vocabulary, the daemon-owned selection, and the precedence.
- `architecture/session.md` — the `runtime_preferences` section: the new column, its store method, its bound, and its precedence.
- `architecture/authorization.md` — the operation table: the new operation's scope and capability.
- `architecture/protocol.md` — the request and response lists: the new pair and the capability entry.
- `architecture/config.md` — the model-profile section: that a configured vendor label now reaches the wire.
- `docs/providers.md` — if it documents the thinking control, reconcile it with the landed contract.
- `CHANGELOG.md` — under `### Changed`: the selection is now daemon-owned and shared across frontends, and a configured vendor label now reaches the wire.

## 13. Acceptance criteria

1. An unrecognized label such as `xhigh` or `max` is preserved verbatim through the protocol, the agent layer, the request, and the encoded body, with no normalization, rejection, or translation.
2. The legacy payload `{"mode":"effort","level":"medium"}` still decodes to the medium label, and all three legacy `ReasoningOverride` variants still decode.
3. A client that has not observed the open-vocabulary capability sends no non-enumerated label, and a daemon that has not advertised it refuses one with a typed error rather than failing the daemon.
4. The selection is durable, principal-scoped, and revision-gated, held in `runtime_preferences`; the principal is derived server-side and the payload carries no identity.
5. The selection survives a daemon restart and is applied to the next admitted turn.
6. Every turn-admission path resolves the same selection: the ordinary direct submit path, the user-prompt-queue delivery path, the idle-continuation path, and the `SessionPromptSubmit` path. The `SessionPromptSubmit` path resolves the persisted selection without a new protocol field.
7. The precedence is: explicit per-turn override → persisted principal preference → agent/model-profile defaults, with the defaults applying exactly as at the baseline when neither of the first two is present.
8. The migration is additive and preserves every existing `runtime_preferences` row; a reasoning write preserves the approval mode, sandbox profile, and last-used model; `STORAGE_LAYOUT_VERSION` matches the highest wired migration and the chain is contiguous.
9. The selection grants no permission, tool, path, or credential scope, and its operation carries the same `Global` scope and no capability as the other principal-scoped preference operations.
10. The static guard against a re-closed vocabulary is registered in `scripts/verify.sh quick` and in the CI `verify` job, carries a self-test, and fails on an injected closed enum.
11. The protocol carries no closed reasoning-effort enum anywhere.
12. `architecture/provider.md`, `architecture/session.md`, `architecture/authorization.md`, `architecture/protocol.md`, and `architecture/config.md` describe the landed contract.

## 14. Stop conditions

The agent must stop and report rather than improvise when:

- **M001 has not closed.** This milestone's open labels depend on M001's faithful wire intent; without it, a new label still vanishes at the encoder on effort-inexpressible surfaces.
- **ADR-0044 is not `accepted`**, or it was amended in a way that changes the daemon-ownership decision in its §5.
- The additive migration cannot be made safely — for example if the `runtime_preferences` table's shape differs from the baseline evidence in §3, or if the contiguous-chain requirement of the project-catalog invariants guard cannot be satisfied. The agent reports the actual shape rather than forcing a migration.
- The capability-negotiation mechanism cannot be expressed through the established `*Capabilities` family. A new negotiation channel is an architecture decision and is not this milestone's to make.
- The `runtime_preferences` revision semantics would have to change to accommodate the selection. That would alter the existing approval/sandbox contract and is out of scope.
- Storing the selection would require granting any authorization meaning to it. The selection is request-shaping configuration only.
- Repository evidence contradicts a canonical invariant in §4.
- The work would expand into the capability record, the frontend option surface, or the wire-intent fidelity — all of which are other milestones.

## 15. Closure evidence required

The closure record (`plans/closure/provider-reasoning-control/002-status.md`) must contain:

- the requirement-to-evidence matrix covering all twelve acceptance criteria;
- the legacy-payload decode results and the verbatim-label round-trip result, quoted;
- the per-path turn-admission resolution results, quoted per path;
- the migration evidence: the preserved-rows assertion, the pre-migration database opening cleanly, the `STORAGE_LAYOUT_VERSION` value, and the project-catalog invariants guard's result;
- the capability-negotiation results for both the client-without-capability and the daemon-without-capability cases;
- the restart result for the persisted selection;
- the contention result for the revision-gated write;
- the static guard's pass result on the tree **and** its rejection of an injected closed enum, plus its `--self-test` result;
- the `scripts/verify.sh quick` result and the Clippy and `cargo fmt` results;
- the authorization result proving the selection grants nothing;
- the documentation changes made, listed by file, and the `CHANGELOG.md` entry;
- any finding the milestone surfaced for M003 or M004, recorded rather than absorbed;
- an explicit statement that the `eggpool-wire` pin, the provider-to-surface mapping, the queue's ownership and delivery authority, and the agent-layer precedence order did not change.

A closure that shows only compilation, or that asserts a path resolves the selection without showing that path's result, is not a closure.

## 16. Handoff notes

- **Read `migrate_v62` and the Task-composer scope before writing the migration.** It is the exact precedent: two nullable columns added to `runtime_preferences` with a plain `ALTER TABLE … ADD COLUMN`, existing rows untouched, bounds enforced in Rust rather than by a `CHECK` constraint. Follow it.
- **The approval and sandbox columns are not yours to touch.** Their `CHECK` constraints, their revision semantics, and their write paths stay exactly as they are. The new scope gets its own method and its own column.
- **`RuntimePreferenceStore`'s writes are full-row upserts.** Adding a column means the existing upserts must preserve the new column rather than nulling it. Check every upsert site, not just the new one — this is the same class of defect the Task-composer scope had to solve.
- **The compatibility hazard in Work package D is the real risk in this milestone.** A new client sending `xhigh` to an old daemon fails that daemon's deserialization and takes the whole turn request with it. Do not treat the capability check as optional.
- **`SessionPromptSubmit` does not need a new field.** That is the point of daemon ownership: it reads the persisted selection. Adding a field there would reintroduce the per-path threading this milestone exists to remove.
- **Do not add a reasoning column to `user_prompt_queue_entry`.** The queue's delivered turn reads the daemon-owned selection. A per-entry copy would re-create the divergence this milestone removes, and would need its own migration.
- **The known-label ordering helper is a fallback, not the contract.** Once M003 lands, the order comes from the model's capability. Keep the helper honest about being the no-capability fallback, and do not present it as the model's option set.
- **The native client needs no change to inherit the selection.** It composes a turn with no explicit override and the daemon resolves the persisted preference. Only add a client-side override if the milestone actually needs one.
- **Do not widen scope to the capability record.** If a label seems wrong for a model, that is M003. Record the finding and move on.
