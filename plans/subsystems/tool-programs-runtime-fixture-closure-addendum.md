# Tool Programs Runtime Fixture and Verification Closure Addendum

Status: conditionally closed — M018 implementation complete; independent review remains

Canonical subsystem roadmap:

- `plans/subsystems/tool-programs-roadmap.md`

Predecessor corrective control document:

- `plans/subsystems/tool-programs-correctness-closure-addendum.md`

Predecessor strict-closure implementation:

- `plans/implementation/tool-programs/017-semantic-recovery-confirmation-and-evidence-closure.md`

Current corrective implementation:

- `plans/implementation/tool-programs/018-runtime-fixture-contract-alignment-and-dvr-unblock.md`

Cross-subsystem blocker record:

- `plans/closure/development-verification-release/006-stop-condition.md`

Target independent closure record:

- `plans/closure/tool-programs/018-status.md`

## 1. Purpose

M017 landed the final production-path notification recovery corrections, but later canonical workspace verification exposed a stale M005-era Tool Programs runtime integration fixture.

The fixture still constructs an empty runtime contract set. Current production enforcement correctly rejects that state before interpreter execution. Six integration tests therefore fail in routine hosted verification and block Development Verification and Release M006.

This addendum transfers final Tool Programs closure ownership to M018 for one narrow purpose:

> Align the runtime integration fixture with the already-approved frozen-contract model, preserve empty-contract rejection, clear the Tool Programs workspace-test blocker, and then re-evaluate the remaining DVR gate without broadening production scope.

## 2. Accepted predecessor implementation

The following M001–M017 outcomes remain accepted and must not regress:

- restricted Python and bounded interpreter semantics;
- scheduler-owned execution;
- accepted-decision authority grants;
- canonical frozen runtime contract snapshots;
- read-only programmatic tool palette;
- native-only production execution;
- typed broker and storage errors;
- replay, checkpoint, descendant, artifact, and notification recovery behavior;
- M017 semantic event confirmation and direct durable restart evidence.

M018 is not authorization to revisit those mechanisms.

## 3. New verification finding

The failing fixture in `tests/tool_program_runtime.rs` uses:

```text
contract_snapshot_json = {"contracts":[]}
allowed_tools = []
empty contract digest
```

for positive execution tests.

The current runtime correctly returns:

```text
Tool Programs require at least one frozen runtime contract
```

Affected tests:

- `emit_constant_completes`
- `for_loop_program_completes`
- `if_else_program_completes`
- `nested_loop_program_completes`
- `list_operations_program_completes`
- `string_operations_program_completes`

This is a fixture compatibility defect. It does not justify changing production contract resolution or permitting empty contracts for programs that happen not to call tools.

## 4. M018 ownership boundary

M018 owns:

- one test-local read-only fixture tool;
- canonical contract entry, snapshot, and digest generation;
- consistent allowed-tool, authority-digest, grant, and job construction;
- positive runtime test repair;
- focused negative tests preserving empty and mismatched contract rejection;
- bounded adjacent Tool Programs regression evidence;
- canonical quick/full re-evaluation;
- planning reconciliation and independent Tool Programs closure.

M018 does not own:

- production runtime changes;
- contract-policy weakening;
- Tool Programs feature expansion;
- CI or verification resource changes;
- the projection-transport daemon-socket stack overflow;
- DVR M006 strict closure;
- actual release work.

## 5. Dependency graph

```text
M001–M016 historical foundations and corrective implementation
        |
        v
M017 production notification/recovery implementation
(conditionally accepted; independent closure not completed)
        |
        v
DVR M006 canonical workspace verification
        |
        v
Stale runtime fixture failure demonstrated
        |
        v
M018 runtime fixture contract alignment
        |
        +--> Tool Programs independent closure
        |
        +--> DVR full-gate re-evaluation
                |
                +--> green: DVR independent closure
                |
                +--> projection failure remains:
                     register one projection-owned corrective plan
```

M018 is dependency-ready against repository baseline `9686338ad6aa8b0ff5ebfe8b07d74e1451180791`.

## 6. Closure authority

After the M018 implementation pass:

- M017 remains a conditionally accepted production implementation;
- `plans/closure/tool-programs/017-status.md` remains absent;
- M018 is the sole Tool Programs closure record for this corrective handoff;
- `plans/closure/tool-programs/018-status.md` records conditional closure because the
  focused Tool Programs evidence is green but the canonical full gate is blocked by an
  unrelated codegg-core migration assertion;
- Tool Programs documentation may claim fixture-contract closure, but not complete DVR
  verification closure;
- DVR M006 remains blocked.

## 7. Strict M018 closure requirements

M018 may close only when:

1. positive runtime fixtures contain one canonical non-empty frozen contract;
2. the same tool set drives snapshot, digest, authority, grant, and job fields;
3. the fixture tool is read-only and test-local;
4. emit-only programs invoke no fixture tool;
5. empty and mismatched contract state remains rejected;
6. all `tests/tool_program_runtime.rs` tests pass;
7. selected adjacent authority and contract tests pass;
8. no production enforcement or authority surface changes;
9. the original Tool Programs failure is absent from canonical verification logs;
10. planning state records any remaining projection blocker separately;
11. a separate reviewer creates `plans/closure/tool-programs/018-status.md`;
12. no unresolved high or medium Tool Programs finding remains.

## 8. Milestone disposition

| Milestone | Status | Disposition |
|---|---|---|
| 001–016 | historical closed/conditionally closed records | Foundations and corrective implementations retained |
| 017 | conditionally accepted implementation | Production semantic recovery corrections retained; strict closure transferred to M018 after workspace verification exposed stale runtime fixtures |
| 018 | conditionally closed | `tests/tool_program_runtime.rs` and adjacent Tool Programs evidence are green; the full gate is blocked by the unrelated codegg-core migration assertion recorded in the M018 closure record |

No projection-transport plan is registered in this addendum. The repository must first remove the deterministic Tool Programs failures and rerun the canonical full gate. Only then may the next single dependency-ready handoff be registered if the projection failure remains reproducible.
