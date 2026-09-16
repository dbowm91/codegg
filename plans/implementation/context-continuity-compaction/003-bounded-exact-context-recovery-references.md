# Context Continuity and Compaction M003 — Bounded Exact Context Recovery References

Status: implemented

Repository baseline: `db3b69d94fa790bf59a9b0ac093572754eb133af`

Source subsystem roadmap:

- `plans/subsystems/context-continuity-compaction-roadmap.md`

Long-term requirements:

- `plans/000-long-term-specification.md#4.6-progressive-disclosure`
- `plans/000-long-term-specification.md#24-protocol-and-storage-requirements`
- `plans/000-long-term-specification.md#26-reliability-and-recovery`
- `plans/000-long-term-specification.md#27-security-requirements`
- `plans/000-long-term-specification.md#29-system-invariants`
- `plans/003-planning-process.md`

Applicable ADRs:

- None. M001's accepted checkpoint identity/storage contract is required.

Primary class: capability / security-sensitive infrastructure

## 1. Objective

Give an installed continuation checkpoint exact, bounded references to important evidence that is intentionally omitted from the active model prompt.

M003 reuses CodeGG's existing durable `FileArtifactStore` and `context_read` surface. It does not add a history database or semantic search index.

The desired model-facing pattern is:

```text
continuation state:
  test failure: cargo test foo failed in parser tests
  evidence: ctx://evidence/<session>/<checkpoint>/<evidence-id>

model calls context_read(handle)
  -> exact same-session redacted evidence slice
```

Existing `ctx://tool/<session>/<turn>/<tool-call>` handles remain unchanged and should be reused whenever they already point to the required detail.

## 2. Why this milestone is dependency-gated

Evidence references must be scoped to a durable checkpoint/epoch identity so that IDs remain stable across later compactions and restart. M001 provides that identity.

M003 can execute in parallel with M002 after M001 closure, but M004 requires both.

## 3. Current implementation evidence

Implementation must re-inspect:

- `src/context/artifact.rs`
- `src/context/handle.rs`
- `src/context/read_tool.rs`
- `src/context/projection.rs`
- `src/context/compaction.rs` (`EvidenceRef`, evidence index)
- `src/agent/context_frame.rs`
- `src/tool/factory.rs`
- tool-result insertion/projection call sites in the agent loop
- M001 checkpoint identity/types
- `architecture/context-ledger.md`
- `architecture/compaction.md`

Baseline facts:

1. production session tools already create `FileArtifactStore::new(&execution.workspace_root)`;
2. artifact writes are bounded to 10 MiB records, use temp-file + rename, reject symlink reads, validate stored handle equality, and survive daemon restart;
3. `context_read` requires exact same-session handle matching and bounded byte-range reads;
4. `ContextHandle` currently recognizes only `ctx://tool/{session}/{turn}/{tool_call_id}`;
5. `ArtifactKind` has tool/read/diff/test/web/image kinds but no conversation/continuation evidence kind;
6. `EvidenceRef` has an ephemeral pass-local ID and digest/summary but no resolvable backing handle;
7. `ContextLedgerState` records artifact handles but M002 is responsible for carrying a bounded set into continuation state;
8. persistent session `MessageStore` data is not a clean role-aware recovery API and may include reasoning/tool parts, so M003 must not expose it wholesale as “history.”

## 4. Invariants

M003 MUST preserve:

- exact compatibility of all existing `ctx://tool/...` handles;
- same-session authorization for every new handle;
- existing `context_read` read-only tool category;
- UTF-8-safe, bounded range reads;
- durable artifact files under the current workspace-owned artifact store;
- no cross-session lookup by substring/prefix;
- no provider-hidden reasoning persistence;
- no raw secret-bearing tool arguments copied into evidence;
- no unrestricted database/event dump through `context_read`;
- no semantic-search/vector index;
- checkpoint references remain valid after later compactions and daemon restart while their bounded retention policy keeps the backing artifact;
- failure to resolve an optional evidence reference degrades to its checkpoint summary rather than invalidating the whole checkpoint.

## 5. Scope

### In scope

- a new exact evidence handle kind;
- compatibility-safe `ContextHandle` refactor;
- new `ArtifactKind` for continuation/conversation evidence;
- materializing selected compaction evidence into `FileArtifactStore`;
- mapping `EvidenceRef` to stable checkpoint-scoped references;
- `context_read` support for the new handles;
- evidence selection/bounds/redaction;
- restart/read/security tests;
- architecture documentation.

### Out of scope

- free-form history list/search;
- exposing `MessageStore` or `EventStore` generically;
- vector retrieval;
- cross-session/cross-project evidence access;
- indefinite retention of every removed message;
- storing full hidden reasoning;
- replacing existing tool artifacts;
- remote artifact replication beyond current workspace/node behavior.

## 6. Required production changes

### 6.1 Generalize ContextHandle without breaking tool handles

Refactor the fixed-shape `ContextHandle` into a representation capable of parsing at least:

```text
ctx://tool/{session_id}/{turn_index}/{tool_call_id}
ctx://evidence/{session_id}/{checkpoint_id}/{evidence_id}
```

One suitable shape is:

```rust
pub enum ContextHandleKind {
    Tool {
        turn_index: usize,
        tool_call_id: String,
    },
    Evidence {
        checkpoint_id: String,
        evidence_id: String,
    },
}

pub struct ContextHandle {
    pub session_id: String,
    pub kind: ContextHandleKind,
}
```

Exact structure may differ. Preserve convenience accessors/builders and public/internal call sites for tool handles.

Handle segments must retain existing safety validation: no empty, slash, control, or whitespace-containing IDs unless a future encoded format is explicitly adopted.

Add `build_evidence(session_id, checkpoint_id, evidence_id)`.

### 6.2 Add continuation evidence artifact kind

Add an `ArtifactKind` such as `ConversationEvidence` or `ContinuationEvidence`.

Existing serialized artifact records must remain deserializable. Adding an enum variant is forward-only for old runtimes, which is acceptable for files written by the new runtime; current runtime must continue reading all old variants.

Do not redesign `ContextArtifact` solely for the new handle. Evidence artifacts may use:

- current turn index for the existing required `turn_index` field;
- `tool_call_id = None`;
- `tool_name = None`;
- full evidence handle as the store key.

If making `turn_index` optional provides a cleaner backward-compatible contract, do so only after inspecting every caller/serialized fixture.

### 6.3 Materialize only bounded selected evidence

Extend the compaction evidence stage so selected omitted evidence may be persisted before the provider-visible transcript drops it.

Candidate evidence types:

- exact user text that exceeded the inline intent budget;
- important assistant text used as decision/progress evidence;
- compact test/error output when no existing tool artifact handle is available;
- other textual evidence explicitly selected by the checkpoint reducer.

Prefer existing `ctx://tool/...` handles when the detail already lives in `FileArtifactStore`. Do not write duplicate evidence artifacts just to create a new handle namespace.

Use deterministic checkpoint-scoped evidence IDs, for example based on source ordinal/kind plus digest, not random IDs the semantic model invents.

The checkpoint stores:

```text
ref handle
kind
short summary
content digest
optional source ordinal
```

The backing artifact stores only the redacted exact text needed for recovery.

### 6.4 Evidence selection policy

Keep evidence growth bounded independently from the 10 MiB per-artifact storage maximum.

Define constants such as:

- max evidence refs per checkpoint: e.g. 64;
- max new evidence artifact bytes per checkpoint: e.g. 256 KiB total;
- max single continuation-evidence artifact: e.g. 64 KiB;
- model-visible summary per ref: small fixed bound.

Exact values should be selected from current context limits/tests and documented.

Selection priority should favor:

1. user steering omitted from inline intent;
2. unresolved failures/errors;
3. test evidence tied to current task;
4. explicit decisions/constraints with source text;
5. recent high-value assistant evidence;
6. lower-priority historical chatter is omitted.

The host chooses evidence candidates deterministically. A semantic checkpoint may nominate an existing `EvidenceRef` ID, but must not supply arbitrary filesystem paths or handles.

### 6.5 Redaction/content policy

For provider `Message` inputs:

- persist visible `User`/`Assistant` text only;
- never persist hidden reasoning because `Message` does not expose provider-private reasoning through this surface;
- for tool results, use the existing projection/artifact path when available;
- do not persist tool-call argument JSON as conversation evidence;
- strip or reject any content class that current export/projection policy marks sensitive.

If evidence comes from another durable store, pass it through the same redaction/bounds policy before creating the artifact.

Content digest must match the redacted stored body, so read verification is meaningful.

### 6.6 context_read support

`ContextReadTool` should continue to:

- parse typed handles;
- enforce exact session match;
- call the artifact store by exact handle;
- return bounded byte ranges;
- include kind/offset metadata;
- never enumerate other sessions.

Update its parameter description to document both handle forms.

Do not add a second `history_read` tool unless implementation evidence shows the existing resolver cannot cleanly support the evidence handle. Avoid expanding the default tool palette unnecessarily.

### 6.7 EvidenceRef integration

Make compaction `EvidenceRef` capable of carrying an optional stable recovery handle and a stable checkpoint-scoped identity.

Do not use `msg_0001` / `tool_0001` alone as durable identity after installation. Those ordinals may remain local diagnostics, but installed checkpoint refs must include checkpoint scope/digest.

M003 should expose an API M004 can call in this order:

```text
build evidence index
select materializable evidence
persist selected evidence with candidate checkpoint ID
verify artifacts
attach handles to checkpoint candidate
```

If checkpoint installation later fails, these artifact files may become orphan candidates. Do not block M003 on garbage collection; filenames/handles are content/identity safe, and M004/closure can document a bounded cleanup follow-up if measured need exists.

### 6.8 Restart behavior

Add tests that:

1. write evidence through `FileArtifactStore`;
2. destroy/recreate store/tool objects;
3. read the evidence via the same `ctx://evidence/...` handle;
4. verify content digest and same-session enforcement.

A missing file after restart returns `NotFound` and leaves the checkpoint summary usable.

## 7. Ordered work packages

### WP1 — Handle compatibility refactor

Generalize parser/builders and preserve all existing tool-handle tests byte-for-byte.

### WP2 — Evidence artifact type and bounded persistence

Add the artifact kind, selection/materialization helper, byte/ref caps, and redaction tests.

### WP3 — Compaction EvidenceRef bridge

Attach checkpoint-scoped handles/digests/summaries to selected evidence. Reuse existing tool handles when present.

### WP4 — context_read exact recovery

Teach `context_read` the new handle shape without adding cross-session/list/search behavior.

### WP5 — Restart/security integration tests

Prove evidence survives store re-open, offsets remain UTF-8 safe, tampered/mismatched handles fail, and sensitive input fixtures are not exposed.

### WP6 — Documentation

Update context-ledger/compaction/tool docs with handle grammar, retention bounds, and ownership.

## 8. Failure, cancellation, restart, and contention semantics

Evidence persistence is best-effort only for evidence classified optional by M002. It is mandatory for any checkpoint field that explicitly claims exact recoverability.

If an artifact write fails:

- do not emit its handle;
- retain the bounded inline summary;
- record a checkpoint diagnostic;
- M004 decides whether failure is severe enough to prevent installation based on whether the evidence was required.

Cancellation before checkpoint installation may leave already-written orphan evidence files. They are inert and same-session scoped.

Concurrent writes to the same deterministic handle must converge on identical content digest. If content differs for the same handle, fail rather than overwrite silently.

`FileArtifactStore::put()` currently replaces by hashed handle. Add a collision/content check for evidence handles if needed so deterministic identity cannot overwrite different content.

## 9. Compatibility and migration

No SQLite migration is expected.

Existing `ctx://tool/...` strings must parse/render identically.

Existing artifact JSON must remain readable. New evidence variant/files are new-runtime data.

Older clients/frontends are unaffected because handles are model/tool-internal strings, but any public docs/schema that constrains handle grammar must be updated.

## 10. Required tests

Handle tests:

- existing tool parse/build round trips unchanged;
- evidence parse/build;
- missing/extra segments;
- unsafe checkpoint/evidence IDs;
- exact same-session matching;
- cross-session substring denial.

Artifact tests:

- continuation evidence write/read;
- content digest;
- max single/total bytes;
- deterministic same-content rewrite;
- conflicting same-handle content rejection;
- symlink/oversize protections remain.

Compaction tests:

- selected omitted user steering receives a recoverable ref;
- existing tool artifact handle is reused;
- ref cap and priority ordering;
- low-priority evidence omitted deterministically;
- installed-style EvidenceRef IDs are checkpoint-scoped.

Security tests:

- tool arguments containing secret-like data are not copied to continuation evidence;
- reasoning fixture is excluded;
- cross-session evidence read denied;
- no directory enumeration/path disclosure in output.

Restart tests:

- reopen store and read evidence handle;
- missing artifact returns bounded NotFound;
- tampered stored handle mismatch rejected.

## 11. Verification commands

Expected narrow verification:

```text
cargo test -p codegg -- context
cargo test -p codegg --test compaction
cargo test --test tool_program_m011_correctness
cargo test --test tool_program_m015_artifact_pipeline
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
scripts/verify.sh quick
```

Use actual focused target names after implementation.

## 12. Documentation

Update:

- `architecture/context-ledger.md`
- `architecture/compaction.md`
- tool documentation for `context_read`
- any prompt/tool-surface docs that show its parameter grammar.

Correct `architecture/context-ledger.md` if it still describes production artifacts as session-local/in-memory; production currently constructs `FileArtifactStore`.

## 13. Acceptance criteria

M003 is complete only when:

1. existing tool handles remain backward compatible;
2. stable checkpoint-scoped evidence handles exist;
3. selected evidence uses the existing durable artifact store;
4. exact evidence can be recovered through `context_read` after restart;
5. recovery remains same-session and bounded;
6. no new transcript/history database or search index exists;
7. sensitive tool arguments/hidden reasoning are excluded;
8. evidence-ref/byte caps are deterministic and tested;
9. existing tool artifacts are reused rather than duplicated where possible;
10. missing optional evidence degrades to checkpoint summary;
11. focused and quick verification pass.

## 14. Stop conditions

Stop and create a corrective/ADR decision if:

- exact recovery requires generic raw access to `MessageStore`/`EventStore`;
- `context_read` cannot support evidence handles without breaking tool handles;
- provider-visible reasoning would have to be persisted;
- safe evidence selection requires an unbounded model-generated index;
- artifact retention must become a cross-node distributed storage service to make the milestone useful.

## 15. Closure evidence required

Record:

- final handle grammar;
- backward-compat tool-handle tests;
- evidence selection/bounds;
- redaction/security tests;
- restart read evidence;
- reused-vs-new artifact behavior;
- collision semantics;
- docs updated;
- verification outcomes;
- any measured orphan-artifact cleanup need.

## 16. Handoff notes

M004 should prepare/persist evidence before installing replacement history and should attach only verified handles to the final checkpoint payload.

M004 must not treat every evidence-write failure as fatal; it should use the M002 required/optional classification and preserve inline summaries for optional misses.
