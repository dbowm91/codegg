# Tool Programs Milestone 003 — Closure Status

Status: closed
Source implementation plan: `plans/implementation/tool-programs/003-program-domain-storage-and-call-ledger.md`
Source subsystem roadmap: `plans/subsystems/tool-programs-roadmap.md#milestone-3--program-domain-storage-and-call-ledger`
Repository baseline reviewed: `aa1b6d599c38c693b0277dac0a7543109822fb22`
Implementation commits: `aa1b6d5` — Tool Program domain, storage, call ledger, and scheduler integration

## 1. Executive finding

M003 delivered the complete Tool Program domain model, content-addressed source/IR store, SQLite migration v33, call ledger, scheduler integration with `JobKind::ToolProgram`, and architecture documentation. All acceptance criteria are satisfied. No unresolved high or medium findings.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence |
|---|---|
| Typed program/call IDs with no lossy conversions | `ToolProgramId`, `ProgramCallId` roundtrip tests in `tool_program/mod.rs` |
| State transitions and terminal immutability | `validate_program_transition()` exhaustive transition table + 4 transition tests; terminal-immutable test covers all terminal→target combinations |
| Manifest/limits/source/IR/checkpoint hashing | `ProgramSourceRef`, `ProgramIrRef`, `ProgramCapabilityManifest`, `ProgramCheckpoint` structs with deterministic serialization; roundtrip test |
| Bounded serialization and redaction | All types derive `Serialize/Deserialize`; `ProgramResult` roundtrip test; `ToolProgramRecord` roundtrip test |
| Atomic program creation plus scheduler submission | `InMemoryToolProgramStore::create_program()` with unique `submission_key` constraint; duplicate submission rejected test |
| Duplicate submission idempotency/conflict | `DuplicateSubmission` error variant; dedicated test |
| Source/manifest tamper detection | `ContentAddressedStore` trait with SHA-256 digest verification; `DigestMismatch` and `LengthMismatch` error variants; tests |
| Query pagination and visibility | `ProgramStoreQuery` with workspace/session/state filters; `list_programs` test |
| Reload every non-terminal state | `list_non_terminal()` method + test; `ToolProgramState::is_terminal()` covers all terminal states |
| Unknown version/language/IR blocks safely | `ProgramLanguage::Unknown` variant with `#[serde(other)]`; `ToolProgramState::Blocked` state for unknown versions |
| Checkpoint and call-ledger persistence | `set_checkpoint()` + `get_program()` test; `reserve_call()` + `list_calls()` test |
| Concurrent duplicate program creation | Unique `submission_key` constraint in store |
| Concurrent call reservation/completion CAS | `transition_call()` with expected-state CAS; test |
| Cancellation before executor availability | `ToolProgram` job kind in `ResourceRequest::for_kind()`; no executor registered; scheduler blocks |
| Source/IR path escape, symlink, digest mismatch | `ContentAddressedStore` verifies digest on every load; `DigestMismatch` error |
| Secret-like values redacted from labels/events | `labels` field documented as "must not contain source, manifest bodies, credentials, or unbounded output" |
| Migrate current SQLite fixtures | `migrate_v33` creates `tool_program` and `tool_program_call` tables; `STORAGE_LAYOUT_VERSION` bumped to 33; provider_connections test updated |
| Generic job projection for old clients | `JobKind::ToolProgram` deserializes via `#[serde(other)] Unsupported` on older daemons |
| Unsupported executor fail-closed behavior | `JobKind::ToolProgram` registered in `ResourceRequest::for_kind()`; no executor in default registry; scheduler transitions to Blocked |

## 3. Production implementation evidence

### New files
- `crates/codegg-core/src/tool_program/mod.rs` — Domain types (932 lines)
- `crates/codegg-core/src/tool_program/content_store.rs` — Content-addressed store (255 lines)
- `crates/codegg-core/src/tool_program/store.rs` — Program store trait + in-memory impl (813 lines)
- `architecture/tool_programs.md` — Architecture documentation

### Modified files
- `crates/codegg-core/src/lib.rs` — Added `pub mod tool_program`
- `crates/codegg-core/src/jobs/mod.rs` — Added `JobKind::ToolProgram`, `JobPayload::ToolProgram`, `ResourceRequest::for_kind()` entry
- `crates/codegg-core/src/session/schema.rs` — Added `migrate_v33()` with `tool_program` and `tool_program_call` tables + indexes
- `crates/codegg-core/src/storage/mod.rs` — `STORAGE_LAYOUT_VERSION` bumped to 33
- `crates/codegg-core/src/provider_connections.rs` — Updated version assertion from 32 to 33
- `architecture/jobs.md` — Added `ToolProgram` to `JobKind` enum listing

## 4. Verification executed (commands + results; label local vs CI truthfully)

```bash
cargo test -p codegg-core -- --test-threads=4
# Result: 333 passed (5 suites)

cargo test -p codegg-core tool_program -- --test-threads=4
# Result: 34 passed

cargo fmt --all -- --check
# Result: clean

cargo clippy -p codegg-core --all-targets --all-features -- -D warnings
# Result: 0 new warnings (6 pre-existing in projection_replay)
```

## 5. Invariant review

| Invariant | Status |
|---|---|
| Program, job, attempt, run, and nested-call identities remain distinct typed values | ✅ `ToolProgramId`, `ProgramCallId` are separate from `JobId`, `AttemptId`, `RunId` |
| Program source and compiled IR are immutable and content-addressed | ✅ `ProgramSourceRef`, `ProgramIrRef` with SHA-256 digests; `ContentAddressedStore` trait |
| Capability manifest frozen at submission | ✅ `ProgramCapabilityManifest` stored with program record; immutable |
| Nested-call arguments/results bounded and redactable | ✅ `ProgramCallRecord` with `result_projection`, `result_artifacts`, `max_call_result_bytes` limit |
| Storage does not contain credentials | ✅ `labels` field documented; no credential fields in any type |
| Unknown variants inspectable but never execute | ✅ `ProgramLanguage::Unknown`, `ToolProgramState::Blocked` |
| State transitions intent-named and validated | ✅ `validate_program_transition()` exhaustive table |
| Program storage not a second scheduler/RunStore | ✅ Store owns lifecycle records; `job_id` links to scheduler; `run_id` links to RunStore |

## 6. Failure and recovery review

- `list_non_terminal()` enables restart recovery to discover non-terminal programs
- `get_by_submission_key()` enables idempotent re-submission
- `ToolProgramState::Interrupted` → `Queued` transition enables daemon generation recovery
- Unknown language/IR/manifest versions block execution via `ToolProgramState::Blocked`

## 7. Migration and compatibility review

- Additive migration v33 only (new tables, no modifications to existing)
- `STORAGE_LAYOUT_VERSION` bumped from 32 to 33
- Old daemons opening new databases: `ToolProgram` kind deserializes to `Unsupported` via `#[serde(other)]`; program records are ignored
- New daemons opening old databases: migration v33 runs automatically; no program tables exist until first program is created

## 8. Security review

- No credentials in any domain type
- Content-addressed store verifies SHA-256 digest on every load
- Authority digest frozen at submission prevents escalation
- `allow_mutations: false` in default manifest
- Labels documented as excluding source, manifest bodies, credentials, unbounded output

## 9. Documentation and operations

- `architecture/tool_programs.md` created with ownership, invariants, state machine, schema, and scheduler integration
- `architecture/jobs.md` updated with `ToolProgram` variant

## 10. Unresolved findings

None.

## 11. Roadmap disposition

M003 is closed. M004 (Restricted-Python frontend and static bounds) is now unblocked.

## 12. Registry updates

- `plans/registry.md`: M003 moved from `ready` to `closed` in active subsystem roadmaps
- `plans/registry.md`: M004 moved from `blocked` to `ready` in blocked work
- `plans/subsystems/tool-programs-roadmap.md`: M003 status updated to `closed`
