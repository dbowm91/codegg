# Context Continuity and Compaction M003 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/context-continuity-compaction/003-bounded-exact-context-recovery-references.md`

Source subsystem roadmap:

- `plans/subsystems/context-continuity-compaction-roadmap.md#8-ordered-milestones`

Repository baseline reviewed: `86ca3a90`

Implementation commits or pull requests:

- `3ea77e9f` — context-continuity M003: bounded exact context recovery references

## 1. Executive finding

M003 is complete. Installed continuation checkpoints now carry exact,
bounded references to evidence intentionally omitted from the active
model prompt, recoverable through the existing durable
`FileArtifactStore` and `context_read` surface. No history database,
semantic search index, or second tool was added. Existing
`ctx://tool/...` handles remain wire-compatible; new checkpoint-scoped
`ctx://evidence/{session}/{checkpoint}/{evidence}` handles resolve
same-session and bounded after restart, degrading to the checkpoint
summary when optional backing artifacts are missing.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Generalized handle without breaking tool handles (§6.1) | `src/context/handle.rs`: `ContextHandle{session_id,kind}`, `ContextHandleKind::Tool{turn_index,tool_call_id}` / `Evidence{checkpoint_id,evidence_id}`, `build_tool`/`build_evidence`/`render`/accessors | pass | Tool wire strings byte-identical; evidence adds second four-segment form with identical safety validation |
| Continuation evidence artifact kind (§6.2) | `ArtifactKind::ContinuationEvidence` (`continuation_evidence`); old artifact JSON test; evidence artifacts use current turn, `tool_call_id=None`, `tool_name=None`, full handle as key | pass | Additive variant only; current runtime reads all old variants |
| Bounded selected materialization (§6.3) | `src/context/evidence.rs`: deterministic `ev-{kind}-{ordinal:04}-{digest12}` IDs; existing `ctx://tool/...` reuse via `tool_call_id` index; no duplicate writes | pass | Semantic model never supplies paths/handles; host chooses candidates deterministically |
| Evidence selection policy (§6.4) | Constants 64 refs / 256 KiB total / 64 KiB single / 280-char summaries; priority user→test→tool→assistant→other, `ToolCall` excluded; recency tie-break + source-order output | pass | Values match plan examples; documented in code and `architecture/` |
| Redaction/content policy (§6.5) | Visible `User`/`Assistant` `Text` only; `Reasoning`/`Image` excluded; `ToolCall` args never persisted; secret-assignment/token + Git URL redaction; digest covers redacted body | pass | Tool results reuse the existing projection/artifact path when available |
| `context_read` support (§6.6) | Both handle forms parse; exact session match; exact-handle store read; bounded UTF-8 ranges; kind/offset metadata; parameter docs updated; no second tool | pass | Category remains read-only; no enumeration surface |
| `EvidenceRef` integration (§6.7) | `EvidenceRef` gains `recovery_handle`/`stable_id`/`checkpoint_id`/`source_ordinal`/`tool_call_id` (`#[serde(default)]`); pass-local `id` retained as diagnostic; `select→persist→verify→attach` API for M004 | pass | Installed refs carry `{checkpoint_id}:{evidence_id}` stable identity, never bare `msg_0001` |
| Restart behavior (§6.8) | File-store reopen read test; digest + session verification; missing file → `NotFound` with summary usable | pass | Tampered-handle mismatch rejected |
| Failure semantics (§8) | Write failure → summary-only + diagnostic, never invalidates checkpoint; deterministic-handle conflict fails closed; identical rewrite converges | pass | Orphan candidates inert and same-session scoped; no GC added (see §10) |
| Compatibility/migration (§9) | No SQLite migration; tool strings parse/render identically; old artifact/event JSON readable; new variant forward-only | pass | No frontend protocol change |

## 3. Production implementation evidence

Final handle grammar:

```text
ctx://tool/{session_id}/{turn_index}/{tool_call_id}
ctx://evidence/{session_id}/{checkpoint_id}/{evidence_id}
```

Ownership preserved: `src/context/compaction.rs` remains the reduction
owner (`EvidenceRef`, `build_evidence_index` with new
`source_ordinal`/`tool_call_id` provenance); `src/context/evidence.rs`
is a bounded persistence helper it and M004 call — no second engine, no
provider/model calls, no search index. `codegg-core` untouched; no
migration.

M004 call order exposed by `src/context/evidence.rs`:

```text
build evidence index (compaction.rs)
select_materializable_evidence(evidence, checkpoint_id)
persist_selected_evidence(store, session, checkpoint, turn, messages, selected, existing_tool_handles)
verify_evidence_artifacts(store, session, refs)
attach verified handles to checkpoint candidate
```

Collision semantics: `FileArtifactStore::put` and
`InMemoryArtifactStore::put` check `ctx://evidence/...` targets only.
Same handle + identical `content_hash`/`redacted_content` converges
idempotently; same handle + different content bails with
`context evidence handle collision with different content`. Tool handles
retain historical behavior (unique per turn/call).

Material deviation from the plan (recorded, not hidden): the plan asked
to preserve existing tool-handle tests byte-for-byte. The clean
enum refactor (`ContextHandleKind` now carries data, so it is no longer
`Copy` and `turn_index`/`tool_call_id` moved from struct fields to
`turn_index()`/`tool_call_id()` accessors) required updating four
field-access assertions in `src/context/handle.rs` to the equivalent
accessor + `render()` assertions. Wire-format assertions (parse/build
strings, error variants, session matching) are unchanged in intent, and
the plan's suggested struct shape was explicitly non-normative
("Exact structure may differ").

No ADR was required: durable authority stayed in the existing workspace
artifact store, no public cross-service contract was introduced, and no
provider-specific semantics were added.

## 4. Verification executed

### Commands run

```bash
cargo test -p codegg --lib -- context
cargo test -p codegg --lib -- context::handle context::artifact context::evidence context::read_tool
cargo test --test compaction --locked -- --test-threads=1
cargo test --test tool_program_m011_correctness --locked -- --test-threads=1
cargo test --test tool_program_m015_artifact_pipeline --locked -- --test-threads=1
cargo fmt --all -- --check
cargo clippy -p codegg --all-targets --locked -- -D warnings
scripts/verify.sh quick
```

### Results

- `cargo test -p codegg --lib -- context`: 530/530 pass (handle
  tool+evidence parse/build, unsafe-ID rejection, session matching,
  artifact round-trips, evidence selection/redaction/persist/verify,
  `context_read` tool+evidence recovery, UTF-8 safety).
- Focused subset `context::handle context::artifact context::evidence
  context::read_tool`: 100/100 pass.
- `cargo test --test compaction`: 65/65 pass (production behavior
  preserved; `EvidenceRef` extension is additive).
- `cargo test --test tool_program_m011_correctness`: 5/5 pass.
- `cargo test --test tool_program_m015_artifact_pipeline`: 4/4 pass.
- `cargo fmt --all -- --check`: pass.
- `cargo clippy -p codegg --all-targets --locked -- -D warnings`: pass
  (two M003 lints fixed: `unnecessary_cast`, `cloned_ref_to_slice_refs`).
  The plan's suggested workspace `--all-features` sweep was deliberately
  not used per repo no-`--all-features` policy; the narrowest affected
  crate plus `verify.sh quick`'s workspace check is the justified
  substitute (same convention as M001/M002 closures).
- `scripts/verify.sh quick`: pass (fmt, builtin-agents check,
  core-boundary, sandbox, execution-ownership, tui-authority, workspace
  check).

Focused §10 coverage: tool round-trips unchanged; evidence parse/build;
missing/extra segments; unsafe checkpoint/evidence IDs; empty-segment
rejection; exact same-session matching; cross-session substring denial;
continuation write/read; digest agreement; single/total byte caps;
deterministic rewrite convergence; conflicting-handle rejection;
symlink/oversize protections intact; omitted user steering receives a
recoverable ref; existing tool handle reused; ref cap + priority order;
`ToolCall` never persisted; reasoning fixture excluded; secret-bearing
fixtures redacted (assignments, tokens, URL credentials); restart
reopen read; missing artifact `NotFound`; tampered-handle mismatch
rejected; no directory enumeration in output.

## 5. Invariant review

| Plan invariant (§4) | Evidence |
|---|---|
| Existing `ctx://tool/...` compatibility | Wire strings unchanged; `build_tool` signature unchanged; tool tests preserve assertions via accessors |
| Same-session authorization for every new handle | `parse` + `same_session` on both forms; `persist` indexes only same-session handles; `verify` rejects cross-session; `context_read` checks before store read |
| `context_read` read-only category | Unchanged; no list/search behavior added |
| UTF-8-safe bounded range reads | `clamp_to_char_boundary` on both forms; byte caps on writes and reads |
| Durable workspace-owned artifact files | `FileArtifactStore` under `.codegg/context_artifacts`, temp-file + rename, restart test |
| No cross-session lookup by substring/prefix | Exact `==` session match; substring-attack tests for both namespaces |
| No provider-hidden reasoning persistence | `extract_text_from_content` + `visible_text_for_materialization` take `Text` only; reasoning-fixture test |
| No raw secret-bearing tool args in evidence | `ToolCall` excluded from selection and refused in persist; secret-fixture test |
| No unrestricted DB/event dump | `context_read` resolves by exact handle only; no `MessageStore`/`EventStore` exposure |
| No semantic-search/vector index | None added |
| Refs valid after later compactions/restart while retained | Checkpoint-scoped `stable_id` + content digest; reopen test; identical rewrite converges |
| Optional-evidence failure degrades to summary | Write/verify failures clear `recovery_handle`, keep summary; `NotFound` path tested |

## 6. Failure and recovery review

- Write failure (store error, bad handle, byte caps, collision):
  summary-only ref + bounded diagnostic; checkpoint as a whole stays
  valid. Covered by persist tests.
- Missing artifact after restart: `store.get` → `None` →
  `context_read` `NotFound`; `verify` degrades to summary-only.
  Covered by missing-artifact tests.
- Tampered stored handle: `artifact.handle != handle` bails with
  handle mismatch. Covered by tamper test.
- Digest mismatch (expected vs stored, or stored body vs stored hash):
  degraded to summary-only in `verify`. Covered.
- Cancellation: persistence helpers are per-ref async store ops with no
  partial checkpoint install; an abandoned candidate leaves inert
  same-session files only (M004 owns install boundaries).
- Contention: deterministic same-handle/same-content converges;
  same-handle/different-content fails closed in both stores. Covered by
  collision tests.
- Stale generation: checkpoint scope is embedded in every new handle
  and `stable_id`; reuse keeps the tool handle but assigns a fresh
  checkpoint-scoped `stable_id`.

## 7. Migration and compatibility review

- No SQLite migration; storage layout version unchanged.
- Existing `ctx://tool/...` strings parse/render identically (see §3
  deviation note for Rust field→accessor migration only).
- Existing artifact JSON (all old `ArtifactKind` variants) remains
  readable; new `continuation_evidence` files are new-runtime data
  (forward-only for old runtimes, accepted by the plan).
- `EvidenceRef` extension is `#[serde(default)]` + `skip_serializing_if`,
  so old serialized refs decode and new refs omit `None` fields.
- No frontend/ACP/projection protocol change; handles are
  model/tool-internal strings. Public `context_read` parameter grammar
  docs updated to both forms.
- Rollback: downgrading past this change leaves evidence files inert
  but present; tool artifacts and checkpoints are unaffected.

## 8. Security review

- `ToolCall` argument JSON never becomes evidence (selection skips,
  persist refuses, dedicated test with secret-bearing args).
- Reasoning excluded by construction (`Text`-only extraction at both
  index and materialization layers; fixture test with mixed
  `Text`+`Reasoning` parts).
- Evidence bodies redacted for secret assignments, standalone secret
  tokens, and URL-embedded credentials before hashing/storage; digest
  covers the redacted body so verification is meaningful.
- Same-session enforcement at parse, persist-index, verify, and read
  layers; substring/prefix attacks tested for both namespaces.
- No directory enumeration or path disclosure: `context_read` output
  carries handle/kind/byte-range metadata only; store paths are
  content-hashed filenames never exposed.
- Identifiers validated (non-empty, bounded by handle safety, no `/`,
  whitespace, or control chars); store I/O rejects symlinks and
  oversize records as before.
- No new network, auth, permission, or export surface; continuation
  records remain excluded from exports pending an explicit contract
  (roadmap §10 rule preserved).

## 9. Documentation and operations

Updated:

- `architecture/context-ledger.md` — dual handle grammar, new
  builder/accessors, `ContinuationEvidence` kind + M003 bounds table,
  redaction/collision/degradation semantics, `context_read` both-forms
  contract.
- `architecture/compaction.md` — extended `EvidenceRef` contract,
  pass-local vs stable identity rule, M003 materialization flow and
  bounds for M004.
- `src/context/evidence.rs` module docs — ownership, bounds, priority,
  and M004 call order.
- `context_read` tool description + parameter grammar — both handle
  forms documented.

Operator notes: per-checkpoint caps 64 refs / 256 KiB total / 64 KiB
single / 280-char summaries. Diagnostics to watch:
`evidence_persist(materialized, reused, summary_only, bytes)` and
`evidence_verify(verified, degraded)`, plus per-ref `tool_call
skipped`, `no source text`, `total bytes cap`, `write failed`,
`missing artifact`, and `digest mismatch` lines. Checkpoint bodies and
evidence contents are never logged.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| Low | Orphan candidate evidence files may accumulate when checkpoint installation later fails or candidates are abandoned | Inert same-session files under `.codegg/context_artifacts`; bounded by per-checkpoint byte caps | No action in M003 per plan §6.7. If M004 measures real accumulation, write a bounded retention follow-up; do not widen M003 |
| — | No other open items | — | — |

No stop condition triggered (no generic `MessageStore`/`EventStore`
access needed, `context_read` supports both handles without breaking
tool handles, no hidden reasoning persisted, no unbounded model index,
no cross-node storage service required).

## 11. Roadmap disposition

Milestone closed; dependencies update as follows:

- M003 (bounded exact context recovery references): hard dependency on
  M001 satisfied — close.
- M004 (transactional rollover and multi-compaction qualification):
  hard dependencies were M002 **and** M003 accepted closure. M002 is
  closed and M003 now closes, so unblock M004 `blocked` → `ready`.
- No other active milestone depends on M003.

## 12. Registry updates

- `plans/registry.md`: M003 `ready` → `closed` with closure link and
  implementation commit `3ea77e9f`; M004 `blocked` → `ready`;
  subsystem row current milestone M001+M002 closed, M003 closed, M004
  ready; execution-order item 1 rewritten to reflect the unblock;
  closure-work control row updated; M003 appended to recently-closed
  work.
- `plans/subsystems/context-continuity-compaction-roadmap.md`: status
  line, M003 section (`ready` → `closed` with closure link), M004
  section (`blocked on M002 and M003` → `ready`).
- `plans/implementation/context-continuity-compaction/003-*.md`:
  `Status: ready` → `Status: implemented`.
- `plans/implementation/context-continuity-compaction/004-*.md`:
  `Status: blocked on M002 and M003 accepted closure` →
  `Status: ready`.
