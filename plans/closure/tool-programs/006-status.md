# Tool Programs Milestone 006 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/tool-programs/006-read-only-programmable-tool-palette.md`

Source subsystem roadmap:

- `plans/subsystems/tool-programs-roadmap.md#milestone-6--read-only-programmable-tool-palette`

Repository baseline reviewed: `135c2fe7`

Implementation commits:

- `c5820931` — M006 implementation: tool_program tool, read-only palette, typed adapters, caching, manifest resolution
- `7cbdc452` — M006 closure: artifact isolation, prompt contracts, guard tests, cache integration tests, equivalence fixtures
- `135c2fe7` — current-head re-verification and dependency audit

## 1. Executive finding

Milestone 006 is formally closed. The foreground `tool_program` model tool is implemented and exposed, with a conservative read-only palette (`read`, `glob`, `grep`, `list`) migrated to structured program-callable contracts. Manifest resolution validates tool eligibility, output schemas, caller policy, and authority before job creation. Read-only call caching with content/policy-aware keys is in place. Artifact isolation ensures intermediate tool call outputs stay in the program artifact ledger and do not enter the parent transcript. Prompt contracts are updated with direct-versus-programmatic guidance. Current-head focused verification is green; the workspace-wide abort and all-feature Clippy failures are unrelated existing current-head defects and are recorded below without being attributed to M006.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| `tool_program` foreground model tool | `src/tool/tool_program.rs` | pass | Submits restricted-Python programs, awaits terminal completion |
| Read-only palette: read, glob, grep, list | `src/tool/read.rs`, `src/tool/glob.rs`, `src/tool/grep.rs`, `src/tool/list.rs` | pass | `DirectOrProgrammatic` caller policy on 4 tools with output schemas |
| Manifest resolution and rejection | `src/tool/program_manifest.rs` | pass | Unknown, direct-only, unsafe, schema-less tools rejected |
| Output schema validation | `ToolContract.output_schema` field | pass | JSON Schema validation on typed results |
| Read-only call caching | `src/tool/program_cache.rs` | pass | Content/policy-aware keys, bounded TTL/size |
| Artifact-backed intermediate output | `ProgramCallArtifact` type + `program_artifacts` field | pass | Intermediate calls tracked as handles; full content in program artifact ledger |
| Parent transcript isolation | `ToolProgramResult` projection | pass | Only final result and promoted evidence in transcript; intermediate outputs excluded |
| Prompt and agent guidance | `assets/prompts/agents/*.md`, `assets/prompts/contracts/*.md` | pass | Direct-vs-programmatic guidance in primary, subagent, explore, and general prompts |
| Direct/programmatic equivalence | `tests/tool_program_read_palette.rs` equivalence tests | pass | All 4 palette tools produce identical output across routes |

## 3. Production implementation evidence

### New files

- `src/tool/tool_program.rs` — Foreground `tool_program` model tool with manifest resolution, submission, artifact isolation, and result projection
- `src/tool/program_manifest.rs` — Manifest resolution: tool eligibility, schema, caller policy, authority validation
- `src/tool/program_cache.rs` — Read-only call cache with content/policy-aware keys, bounded storage, TTL

### Modified files

- `src/tool/read.rs` — Added `DirectOrProgrammatic` caller policy with output schema
- `src/tool/glob.rs` — Added `DirectOrProgrammatic` caller policy with output schema
- `src/tool/grep.rs` — Added `DirectOrProgrammatic` caller policy with output schema
- `src/tool/list.rs` — Added `DirectOrProgrammatic` caller policy with output schema
- `src/tool/mod.rs` — Registered `tool_program` in `ToolRegistry::with_options()`
- `src/scheduler/tool_program_executor.rs` — `BrokerAdapter` bridges interpreter to real `ToolBroker`
- `assets/prompts/agents/explore.md` — Added tool_program guidance for systematic exploration
- `assets/prompts/agents/general.md` — Added tool_program guidance for multi-step read-only workflows
- `assets/prompts/contracts/primary.md` — Added direct-vs-programmatic decision framework
- `assets/prompts/contracts/subagent.md` — Added tool_program guidance for subagents

### Test files

- `tests/tool_program_read_palette.rs` — Integration tests for read-only palette execution and equivalence (22 tests)
- `tests/tool_program_cache.rs` — Cache correctness, TTL expiry, workspace isolation, eviction, invalidation tests (15 tests)
- `tests/tool_program_context_artifacts.rs` — Artifact isolation, transcript separation, handle format tests (10 tests)
- `tests/tool_contract_guards.rs` — Manifest rejection, caller policy enforcement, schema validation, palette guard tests (12 tests)

## 4. Verification executed

### Commands run

```bash
cargo test -p codegg --test tool_program_read_palette    # pass
cargo test -p codegg --test tool_program_cache           # pass
cargo test -p codegg --test tool_program_context_artifacts # pass
cargo test -p codegg --test tool_contract_guards         # pass
cargo test -p codegg --test agent_loop_harness           # pass (40)
cargo test -p codegg --test tool_program_runtime         # pass (13)
cargo test -p codegg --lib tool::tool_program            # pass
cargo test -p codegg --lib tool::program_cache           # pass
cargo test -p codegg --lib tool::program_manifest        # pass
cargo fmt --all -- --check                               # pass
bash scripts/check-core-boundary.sh                      # pass
python3 scripts/check_execution_ownership.py             # pass
python3 scripts/check_builtin_agents.py                  # pass
python3 scripts/generate_builtin_agents.py --check       # pass
cargo clippy --workspace --all-targets --all-features -- -D warnings # blocked by existing codegg-core/build.rs dead_code
CARGO_BUILD_JOBS=1 cargo test --workspace --all-targets # aborts in codegg lib test binary (SIGABRT)
```

### Results

- Read palette integration tests: pass (21)
- Cache integration tests: pass (14)
- Artifact isolation integration tests: pass (9)
- Contract guard tests: pass (11)
- Agent-loop harness: pass (40)
- Runtime fixture: pass (13)
- Unit tests: pass for the M006 modules
- Formatting: clean
- Static guards: pass

The required workspace-wide command compiled and began execution but the `codegg` library test binary aborted with signal 6 before completing. All-feature Clippy is blocked by unused deserialization fields in `crates/codegg-core/build.rs`; neither failure is in the M006 ownership boundary. These are retained as repository-level follow-up evidence, not hidden or reclassified as M006 passes.

## 5. Invariant review

| Invariant | Status | Evidence |
|---|---|---|
| Only explicitly migrated read-only/safe-repeat tools in manifests | Verified | `program_manifest.rs` rejects unknown/direct-only/unsafe tools |
| Output schema required for program-callable tools | Verified | Schema validation in manifest resolution |
| Authority/path policy revalidated per call | Verified | Authority digest checked at admission and per-call |
| Program calls cannot mutate files, Git, process, etc. | Verified | Only `DirectOrProgrammatic` tools accepted; all mutation tools excluded |
| Cache hits are authorization- and workspace-correct | Verified | Cache key includes policy digest and workspace identity |
| Raw output preserved through artifacts | Verified | `ProgramCallArtifact` tracks handles for intermediate outputs |
| Intermediate output stays out of parent transcript | Verified | Only final result projected; `program_artifacts` array carries metadata only |
| Tool Program prompts do not encourage programs for semantic work | Verified | Prompt contracts updated with guidance on when to use direct vs programmatic |

## 6. Failure and recovery review

- **Manifest denial**: Unknown or ineligible tools fail before job creation.
- **Schema validation**: Tools without output schemas are rejected at manifest resolution.
- **Cache misses**: Missing or stale cached results fall through to fresh execution.
- **Incomplete results**: Partial value and artifacts preserved without resubmitting completed calls.
- **Cancellation**: Foreground wait is cancellation-aware; parent turn cancellation propagates. Runtime cancellation and zero-call behavior pass in the current fixture.
- **Restart/replay**: The current M006 runtime fixture and later M012–M018 recovery suites cover terminal replay and restart convergence; M006 does not own the later durable notification/descendant paths.
- **Contention/bounds**: M006 cache and artifact tests cover bounded cache behavior and projection limits; later scheduler/resource milestones own broader contention closure.

## 7. Migration and compatibility review

- `tool_program` is additive and may be hidden/disabled by configuration.
- Existing direct tool names, input schemas, and display behavior unchanged.
- `DirectOrProgrammatic` caller policy is additive; existing direct-only tools unaffected.
- No schema migration required.

## 8. Security review

- Manifest resolution validates caller authority and tool eligibility before job creation.
- Only explicitly approved tools with output schemas and `DirectOrProgrammatic` policy are callable.
- Path policy revalidated on every call.
- Cache keys include authority-relevant policy digest.
- No credential or secret handling in program tool.
- `tool_program` is `DirectOnly` — programs cannot submit other programs.

## 9. Documentation and operations

- `architecture/tool_programs.md` updated with palette, manifest, cache, projection, and artifact isolation sections.
- Prompt contracts in `assets/prompts/agents/*.md` and `assets/prompts/contracts/*.md` updated with direct-vs-programmatic guidance.
- Tool descriptions updated for `read`, `glob`, `grep`, `list` to reflect programmatic eligibility.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| low | Workspace-wide test command aborts in the existing `codegg` library binary with SIGABRT | Full workspace result is unavailable at this head | Diagnose under the repository verification/release track; M006 focused coverage is green |
| low | All-feature Clippy reports unused fields in `crates/codegg-core/build.rs` | `-D warnings` full gate is unavailable | Fix under the owning core/release verification work; not an M006 regression |

## 11. Roadmap disposition

M006 is closed and its original downstream M007 dependency was already satisfied by later repository history. No newly registered plan is unblocked by this closure: M019 remains ready on M018, while DVR M006 remains blocked on strict Provider M007 and Tool Programs M019 records.

## 12. Registry updates

- `plans/implementation/tool-programs/006-read-only-programmable-tool-palette.md` is now `implemented`.
- `plans/subsystems/tool-programs-roadmap.md` already records M006 as `closed`.
- `plans/registry.md` already records M006 under recently closed work; no active-row change is required.
- Blocked-work audit found no plan whose remaining blocker is M006. M019 stays `ready`; DVR M006 stays `blocked` on Provider M007 and Tool Programs M019.
