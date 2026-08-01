# CodeGG Active Planning Registry

This file is the compact control surface for interim planning. It links active documents and blockers without duplicating their detailed requirements.

Canonical direction remains in:

- `plans/000-long-term-specification.md`
- `plans/001-terminology-and-domain-model.md`
- `plans/002-long-term-roadmap.md`
- `plans/003-planning-process.md`

## Status vocabulary

- **proposed** — roadmap or plan exists but is not approved for execution.
- **ready** — dependencies and interfaces are satisfied; plan may be handed off.
- **active** — implementation or closure work is in progress.
- **blocked** — a named dependency or evidence requirement prevents progress.
- **closing** — implementation landed and closure evidence is being gathered.
- **closed** — closure record accepted.
- **conditionally closed** — substantial work landed, but a named correctness finding prevents strict closure.
- **superseded** — replaced by another document.
- **archived** — no longer active and retained for traceability.

## Active subsystem roadmaps

| Subsystem | Status | Roadmap | Current milestone | Dependencies or blockers |
|---|---|---|---|---|
| Domain identity and compatibility | closed | `plans/subsystems/domain-identity-roadmap.md` | Milestone 4 closed | — |
| Runtime assets and harness interoperability | closed | `plans/subsystems/runtime-assets-roadmap.md` | Milestone 4 closed | — |
| Provider connections and Eggpool | closing | `plans/subsystems/provider-tool-dvr-independent-closure-ratification-addendum.md` | Milestone 007 conditionally closed | Provider/storage review passes; hosted verify `30681164263` fails on unrelated workspace Clippy dead-code errors in `crates/codegg-core/build.rs` |
| Project catalog and lazy discovery | closed | `plans/subsystems/project-catalog-roadmap.md` | Milestone 4 closed | — |
| Multi-project TUI and sessions | closed | `plans/subsystems/tui-project-sessions-roadmap.md` | Milestones 001–004 closed | — |
| Frontend-neutral session projections | closed | `plans/subsystems/session-projections-roadmap.md` | Milestone 012 closed | — |
| Agent runtime, model adaptation, and ACP | closing | `plans/subsystems/agent-runtime-model-adaptation-acp-corrective-closure-addendum.md` | M013 closure review | M013 production implementation landed; M014–M017 retain explicit predecessor blockers |
| Programmatic tool execution and Tool Programs | closing | `plans/subsystems/provider-tool-dvr-independent-closure-ratification-addendum.md` | Milestone 019 ready | M018 fixture implementation is accepted and green; `018-status.md` remains provisional implementation evidence, and M019 owns independent strict review and isolation ratification |
| Development verification and release | active | `plans/subsystems/provider-tool-dvr-independent-closure-ratification-addendum.md` | Milestone 006 blocked | Final DVR closure requires strict Provider M007 and Tool Programs M019 records before independent DVR review may proceed |

## Dependency-ready implementation plans

| Subsystem | Milestone | Status | Implementation plan | Dependencies |
|---|---|---|---|---|
| Programmatic tool execution and Tool Programs | 019 — independent strict closure and evidence ratification | ready | `plans/implementation/tool-programs/019-independent-strict-closure-and-evidence-ratification.md` | M018 implementation landed; repeated-run and green full/hosted evidence are available for independent review |

Agent-runtime M011 is conditionally closed after a post-closure production-path
audit invalidated the strict disposition. M012 is strictly closed by
`plans/closure/agent-runtime-model-adaptation-acp/012-status.md`; M013 is the
only dependency-ready handoff in the corrective addendum. M014–M017 must not be
implemented out of order.

Tool Programs M019 remains an independent review-only handoff. DVR M006 must
remain blocked until Provider M007 and Tool Programs M019 both have strict
closure records with no unresolved high- or medium-severity finding.

## Active closure work

Agent-runtime M011 historical implementation evidence remains useful, but
`plans/closure/agent-runtime-model-adaptation-acp/011-corrective-status.md`
now governs disposition. Strict closure requires completion and independent
review of M012–M017.

M012 is strictly closed by `plans/closure/agent-runtime-model-adaptation-acp/012-status.md`.
Its closure audit unblocked only M013; M014–M017 retain their predecessor
blockers.

M017 production implementation remains conditionally accepted. `plans/closure/tool-programs/017-status.md` remains absent; final Tool Programs verification responsibility transferred through M018 to M019.

M018 implementation has landed. `plans/closure/tool-programs/018-status.md` is retained as provisional conditional implementation evidence, not independent strict approval. M019 owns the independently attributable strict decision.

Provider M006 implementation and evidence have landed. `plans/closure/provider-connections/006-status.md` is retained as provisional implementation-authored evidence. M007 is conditionally closed by `plans/closure/provider-connections/007-status.md`; strict closure awaits the named hosted workspace-gate evidence.

Development verification and release M006 in-scope work has landed, but its
closure record remains absent. DVR M006 is blocked until Provider M007 and Tool
Programs M019 are both strictly closed; only then may its independent reviewer
perform final DVR closure.

## Blocked work

| Subsystem | Milestone | Status | Implementation plan | Blocker |
|---|---|---|---|---|
| Development verification and release | 006 — final evidence and release documentation closure | blocked | `plans/implementation/development-verification-release/006-final-evidence-and-release-documentation-closure.md` | Requires strict `plans/closure/provider-connections/007-status.md` and `plans/closure/tool-programs/019-status.md` with no unresolved high/medium findings |
| Agent runtime, model adaptation, and ACP | 014 — canonical prompt and context-plan convergence | blocked | `plans/implementation/agent-runtime-model-adaptation-acp/014-canonical-prompt-and-context-plan-convergence.md` | Requires M013 strict closure |
| Agent runtime, model adaptation, and ACP | 015 — adapter-driven reasoning safety | blocked | `plans/implementation/agent-runtime-model-adaptation-acp/015-adapter-driven-reasoning-safety.md` | Requires M014 strict closure |
| Agent runtime, model adaptation, and ACP | 016 — descendant admission, cancellation, and execution context | blocked | `plans/implementation/agent-runtime-model-adaptation-acp/016-descendant-admission-cancellation-and-execution-context.md` | Requires M015 strict closure |
| Agent runtime, model adaptation, and ACP | 017 — corrective integration evidence and closure | blocked | `plans/implementation/agent-runtime-model-adaptation-acp/017-corrective-integration-evidence-and-closure.md` | Requires M012–M016 strict closure records |

## Newly dependency-ready work

| Subsystem | Milestone | Status | Implementation plan | Dependencies |
|---|---|---|---|---|
| Agent runtime, model adaptation, and ACP | 013 — specialized runtime finalization and research coordination | closing | `plans/implementation/agent-runtime-model-adaptation-acp/013-specialized-runtime-finalization-and-research-coordination.md` | Implementation landed; closure evidence under review |

The previously reported projection stack failure did not reproduce after M018
and remains unregistered unless it reappears reproducibly.

## Deferred unregistered product work

These are not dependency-ready correctness plans and remain outside the active handoff:

- cross-tab artifact hand-off UX;
- numeric acknowledgement/resync hot-key UX;
- plugin-specific `ProjectionEvent::PluginUi` semantics;
- final removal of legacy remote variants after the compatibility window;
- final team roles, presence, and chat;
- production hosted Tool Program transport.

## Recently closed or conditionally closed work

| Subsystem | Milestone | Closure record | Closed/reviewed at commit | Follow-up |
|---|---|---|---|---|
| Agent runtime, model adaptation, and ACP | 011 — integration evidence and closure | `plans/closure/agent-runtime-model-adaptation-acp/011-corrective-status.md` | conditionally closed at reviewed head `7d8657e` | Strict closure withdrawn; M012 ready and M013–M017 registered through corrective addendum |
| Agent runtime, model adaptation, and ACP | 012 — ACP turn lifecycle and correlation correctness | `plans/closure/agent-runtime-model-adaptation-acp/012-status.md` | implementation and closure commits | Strictly closed; M013 unblocked, M014–M017 remain predecessor-blocked |
| Agent runtime, model adaptation, and ACP | 009 — context-plan and cache convergence | `plans/closure/agent-runtime-model-adaptation-acp/009-status.md` | implementation and closure commit | Historical implementation retained; M014 owns corrective prompt/context identity convergence |
| Agent runtime, model adaptation, and ACP | 010 — ACP v1 daemon/projection adapter | `plans/closure/agent-runtime-model-adaptation-acp/010-status.md` | implementation and closure commit | Historical implementation retained; M012 owns lifecycle/correlation correctness |
| Agent runtime, model adaptation, and ACP | 008 — reasoning preservation and Poolside Laguna adapter | `plans/closure/agent-runtime-model-adaptation-acp/008-status.md` | implementation and closure commit | Historical implementation retained; M015 owns UTF-8 and adapter-authority correction |
| Agent runtime, model adaptation, and ACP | 006 — progress, loop, and tool recovery controller | `plans/closure/agent-runtime-model-adaptation-acp/006-status.md` | implementation and closure commit | Historical implementation retained |
| Agent runtime, model adaptation, and ACP | 007 — declarative model-adapter registry | `plans/closure/agent-runtime-model-adaptation-acp/007-status.md` | implementation and closure commit | Historical implementation retained; M015 owns typed request-transform authority |
| Agent runtime, model adaptation, and ACP | 005 — specialized research runtime | `plans/closure/agent-runtime-model-adaptation-acp/005-status.md` | `e3db48c` implementation; closure commit | Historical preparation/types retained; M013 owns host coordination and finalization |
| Agent runtime, model adaptation, and ACP | 004 — specialized security-review runtime | `plans/closure/agent-runtime-model-adaptation-acp/004-status.md` | implementation/closure commit | Historical preparation retained; M013 owns authoritative finalization |
| Agent runtime, model adaptation, and ACP | 001 — prompt compilation and agent registry correctness | `plans/closure/agent-runtime-model-adaptation-acp/001-status.md` | `3cb6c0e` implementation | Historical implementation retained; M014 owns complete block/fingerprint convergence |
| Agent runtime, model adaptation, and ACP | 003 — bounded nested agent delegation | `plans/closure/agent-runtime-model-adaptation-acp/003-status.md` | `b893462` implementation | Historical functional delegation retained; M016 owns atomic admission and lineage cancellation |
| Provider connections and Eggpool | 006 — storage layout assertion and verification reconciliation | `plans/closure/provider-connections/006-status.md` | implementation `139c832`; merged at `7d8657e` | Executable correction and evidence retained; strict closure authority transferred to independent Provider M007 because the M006 status was authored on the implementation branch |
| Provider connections and Eggpool | 007 — independent closure ratification and governance reconciliation | `plans/closure/provider-connections/007-status.md` | review head `04f4bb2`; review-state commit `ebd7c11` | Conditionally closed; provider/storage evidence passes, but hosted workspace Clippy failed on unrelated build-script dead-code errors |
| Programmatic tool execution and Tool Programs | 018 — runtime fixture contract alignment and DVR unblock | `plans/closure/tool-programs/018-status.md` | implementation `4235442`; merged at `c0aa785` | Provisional conditional implementation evidence only; independent strict review and evidence ratification transferred to Tool Programs M019 |
| Programmatic tool execution and Tool Programs | 017 — semantic recovery confirmation and evidence implementation | — | implementation landed before reviewed head `9686338` | Conditionally accepted production implementation; strict closure transferred through M018 to M019 after canonical workspace verification exposed stale M005-era runtime fixtures |
| Development verification and release | 005 — green verification and crates.io correctness implementation | — | implementation series `e90a78e` through reviewed head `db890ac` | Conditionally accepted implementation; strict closure transferred to M006 because final-head hosted evidence, package inventory, release documentation, and Tokio guard closure were incomplete |
| Development verification and release | 006 — final evidence and release documentation closure | — | M006 in-scope implementation `80e0919`; hosted evidence update `9686338` | In-scope work landed; strict closure now blocked on independent Provider M007 and Tool Programs M019 records. The projection failure did not reproduce. |
| Development verification and release | 004 — optional integration evidence cleanup and closure | `plans/closure/development-verification-release/004-status.md` | `9425938` | Historical conditional record; structural LSP/evidence cleanup retained, but strict subsystem closure transferred through M005 to M006 |
| Development verification and release | 003 — manual crates.io release ownership | `plans/closure/development-verification-release/003-status.md` | `d4d57d2` | Historical conditional record; automated release removal retained, with final release-contract closure transferred through M005 to M006 |
| Development verification and release | 002 — canonical local verification contract | `plans/closure/development-verification-release/002-status.md` | `75b5dc0` | Historical conditional record; script/document consolidation retained, with final verification evidence transferred through M005 to M006 |
| Development verification and release | 001 — routine CI contraction | `plans/closure/development-verification-release/001-status.md` | `986d516` with amendment `6730213` | Historical conditional record; one-job contraction retained, with final green/evidence closure transferred through M005 to M006 |
| Programmatic tool execution and Tool Programs | 016 — notification replay polish implementation | `plans/closure/tool-programs/016-status.md` | implementation `f4101b9`; conditional review in the M017 registration series | Historical conditional record; M017 retained semantic confirmation and durable evidence, with final verification closure transferred to M018 |
| Programmatic tool execution and Tool Programs | 015 — final production-path implementation and review | `plans/closure/tool-programs/015-status.md` | implementation `247ef50`; independent approval `230f435`; original closure `9bd9d0b`; post-closure reconciliation in the M016 registration series | Historical conditional record; strict closure transferred through M016–M018 |
| Programmatic tool execution and Tool Programs | 014 — production-boundary implementation | `plans/closure/tool-programs/014-status.md` | implementation/closure head `c9559d2`; post-implementation reconciliation in the M015 registration series | Historical conditional implementation record; strict closure transferred through M015–M018 |
| Programmatic tool execution and Tool Programs | 013 — production authority, descendant, delivery, and recovery implementation | `plans/closure/tool-programs/013-status.md` | implementation/closure head `58e87ff`; post-implementation reconciliation at `7b782da` | Historical conditional implementation record; strict closure transferred through M014–M018 |
| Programmatic tool execution and Tool Programs | 012 — authority, recovery, delivery, and child-ownership corrective implementation | `plans/closure/tool-programs/012-status.md` | `d056e42` implementation; later reviews transferred strict closure through M013–M018 | Historical conditional implementation record |
| Programmatic tool execution and Tool Programs | 011 — production correctness and ownership closure | `plans/closure/tool-programs/011-status.md` | `0ae1067` implementation; `705ae2c` original closure; post-closure review at `d71a5ee` | Historical conditional implementation record; remaining findings transferred through M012–M018 |
| Programmatic tool execution and Tool Programs | 010 — harness, Eggpool, chaos, performance, and closure | `plans/closure/tool-programs/010-status.md` | `2f5e3d3` implementation; `b62686e` closure/reconciliation | Historical conditional closure; later milestones own final production and verification depth |
| Programmatic tool execution and Tool Programs | 009 — OpenAI Responses hosted-program adapter | `plans/closure/tool-programs/009-status.md` | HEAD implementation | Historical capability/library closure; production Tool Programs remain native-only |
| Programmatic tool execution and Tool Programs | 008 — background programs, projections, and parent notification | `plans/closure/tool-programs/008-status.md` | HEAD implementation | Historical closure; later milestones own final notification recovery and verification depth |
| Programmatic tool execution and Tool Programs | 007 — build/test child-job composition | `plans/closure/tool-programs/007-status.md` | HEAD implementation | Historical closure; later milestones own final production and verification depth |
| Programmatic tool execution and Tool Programs | 006 — read-only programmable tool palette | `plans/closure/tool-programs/006-status.md` | HEAD implementation | Read-only palette retained; M018 must not broaden authority |
| Programmatic tool execution and Tool Programs | 005 — durable interpreter, watchdog, and recovery | `plans/closure/tool-programs/005-status.md` | `75f3c5ae` implementation | Historical component closure; later milestones own final production and verification depth |
| Programmatic tool execution and Tool Programs | 004 — restricted-Python frontend and static bounds | `plans/closure/tool-programs/004-status.md` | `dcd2024e` implementation | Restricted language and static-bound foundation retained |
| Programmatic tool execution and Tool Programs | 003 — program domain, storage, and call ledger | `plans/closure/tool-programs/003-status.md` | `733993b` implementation + docs follow-up | Durable domain foundation retained and extended by later milestones |
| Programmatic tool execution and Tool Programs | 002 — tool contracts and canonical broker | `plans/closure/tool-programs/002-status.md` | HEAD implementation | Historical closure; frozen-contract and broker authority must not regress in M018 |
| Programmatic tool execution and Tool Programs | 001 — scheduler-owned Python execution | `plans/closure/tool-programs/001-status.md` | HEAD implementation | Scheduler-owned ordinary Python foundation retained |
| Frontend-neutral session projections | 012 — TUI disconnect lifecycle and final evidence closure | `plans/closure/session-projections/012-status.md` | `0672044` implementation; `f046de5` corrective test evidence; final reviewed head `f046de5` | Closed historically; reported daemon-socket stack behavior awaits post-M018 re-evaluation before any new plan is registered |
| Frontend-neutral session projections | 011 — evidence correctness and mechanism verification closure | `plans/closure/session-projections/011-status.md` | `560b8b7` main implementation; final reviewed head `1a93167` | Historical conditional closure; M012 accepted the remaining lifecycle, evidence, stability, and reconciliation work |
| Frontend-neutral session projections | 010 — mechanism-faithful transport verification and final closure | `plans/closure/session-projections/010-status.md` | `a3ab136` implementation/evidence; final reviewed M10 head `8bd59b2` | Historical conditional record; M011/M012 own final depth |
| Frontend-neutral session projections | 009 — production-shaped transport verification and strict closure | `plans/closure/session-projections/009-status.md` | `3406c742` implementation/evidence; `426dfffe` follow-up | Historical conditional record; M10–M12 own final depth |
| Frontend-neutral session projections | 008 — final transport lifecycle and replay evidence polish | `plans/closure/session-projections/008-status.md` | `6975050a` implementation; `ea6e38d` original closure | Historical conditional record; later milestones own final depth |
| Frontend-neutral session projections | 007 — corrective transport lifecycle and evidence closure | `plans/closure/session-projections/007-status.md` | `9887c2d` implementation; `922333b` original closure | Historical conditional record |
| Frontend-neutral session projections | 006 — atomic control delivery, transport verification, and raw compatibility hardening | `plans/closure/session-projections/006-status.md` | `270cc5f` closure; `8ca570f` implementation | Historical conditional record |
| Frontend-neutral session projections | 005 — remote transport isolation, resume, compatibility closure | `plans/closure/session-projections/005-status.md` | `4c751ff` | Historical implementation retained |
| Frontend-neutral session projections | 004 — frontend adoption and compatibility | `plans/closure/session-projections/004-status.md` | `4c751ff` | — |
| Frontend-neutral session projections | 003 — visibility, redaction, and artifact handles | `plans/closure/session-projections/003-status.md` | `bac73ce` | — |
| Frontend-neutral session projections | 002 — scoped subscriptions and durable replay | `plans/closure/session-projections/002-status.md` | `c1d910a` corrective integration; library at `8dc4b85` | — |
| Frontend-neutral session projections | 001 — projection contracts and canonical reducer | `plans/closure/session-projections/001-status.md` | `f6c8669` | — |
| Multi-project TUI and sessions | 004 — persistent restoration, resource bounds, and closure | `plans/closure/tui-project-sessions/004-status.md` | `0d98576` | — |
| Multi-project TUI and sessions | 003 — event routing and lifecycle | `plans/closure/tui-project-sessions/003-status.md` | `6ad9952` closure completion; implementation at `248aa32` | — |
| Multi-project TUI and sessions | 002 — project picker and tab navigation | `plans/closure/tui-project-sessions/002-status.md` | `f569386` | — |
| Multi-project TUI and sessions | 001 — project-aware state and catalog client | `plans/closure/tui-project-sessions/001-status.md` | `62e26b1` | — |
| Project catalog and lazy discovery | 004 — protocol, server migration, and closure | `plans/closure/project-catalog/004-status.md` | `d1e5b70` | — |
| Domain identity and compatibility | 004 — closure and legacy-removal criteria | `plans/closure/domain-identity/004-status.md` | `c4e9cf8` | — |
| Runtime assets and harness interoperability | 004 — immutable runtime pinning and closure | `plans/closure/runtime-assets/004-status.md` | `2293a11` | — |
| Provider connections and Eggpool | 005 — corrective lifecycle, rotation, health, and closure | `plans/closure/provider-connections/005-status.md` | `0eadc85` | — |

## Registry maintenance rules

1. Add a subsystem roadmap when it becomes active, not merely because it is a possible future track.
2. Register an implementation plan as dependency-ready only after dependency and handoff review.
3. Move a plan from ready to active when implementation begins.
4. Move it to closing when production work lands and closure review starts.
5. Mark it closed only when the linked closure record says closed and no unresolved high/medium finding remains.
6. Use conditionally closed when a post-closure correctness finding invalidates a strict claim.
7. Record blockers precisely and link the document that owns their resolution.
8. Remove closed rows from active sections after recording them under recently closed work.
9. Periodically archive old closed interim documents while preserving links.
10. Do not copy detailed milestone requirements into this registry.
11. When one milestone closes, create/register only the next dependency-ready handoff.
