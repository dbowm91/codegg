# Residual Runtime Consolidation Milestone 002 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/residual-runtime-consolidation/002-core-daemon-request-family-decomposition.md`

Source subsystem roadmap:

- `plans/subsystems/residual-runtime-consolidation-roadmap.md#M002--CoreDaemon-request-family-physical-decomposition`

Repository baseline reviewed: `c7f93874` (M001 closure; plan baseline
`9dfbc6e1f842a76ab6154d1bd99cbadfdff4b2e6` plus the M001 retirement, which
touched no daemon request path)

Implementation commits or pull requests:

- Implementation, closure record, and registry updates land in a single
  commit titled "plans: close residual-runtime M002, decompose CoreDaemon
  request families" (this record's own commit; locate with
  `git log --oneline --grep="close residual-runtime M002"`) — request-family
  physical decomposition, routing tests, doc reconciliation, M002 closure
  with M003 unblocked to ready

## 1. Executive finding

M002 is complete. `src/core/daemon.rs` went from 12,007 lines to 6,287
lines by moving all 139 main-dispatch request arms verbatim into nine
coherent `src/core/daemon_<family>.rs` modules behind a thin
`Box::pin` family router (~110 lines) keyed by the single
`DaemonRequestFamily::of` routing table (`src/core/daemon_family.rs`,
exhaustive over all 166 `CoreRequest` variants). `CoreDaemon` remains the
single composition/lifecycle authority: every family handler is a boring
`impl CoreDaemon` method operating on the same daemon-owned state, and no
new store, scheduler, state machine, service bus, actor/DI framework, or
authority was introduced. All protocol/runtime semantics are preserved:
full root lib suite (4,407 tests), core suites (166), and the
real-transport projection suite (58) pass; `cargo fmt --check`, workspace
`clippy --all-targets --all-features -D warnings`, and
`scripts/verify.sh quick` pass.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Work package A: map each `CoreRequest` variant to canonical family/owner, identify cross-family helpers | `DaemonRequestFamily::of` in `daemon_family.rs` (exhaustive match over all 166 variants); ownership table in module docs, `architecture/core.md`, and the core skill; §3 before/after map | pass | 139 routed arms + 17 chat + 10 interactive + `Initialize`; cross-family helpers enumerated in §3 and kept shared as `pub(crate)` |
| Work package B: extract low-coupling families first (provider/project/job/goal/assets), thin delegates where compatible | `daemon_assets` (3 arms), `daemon_providers` (19), `daemon_projects` (20), `daemon_jobs` (27), `daemon_goals` (18) — bodies moved verbatim; `CoreDaemon::handle_request*` remains the top-level typed entry and delegates | pass | Mechanical split script classified every top-level arm; dry-run asserted unanimous family per arm and zero lost arms |
| Work package C: extract session/turn/projection families carefully with transport-owner and cancellation tests | `daemon_sessions` (20 arms), `daemon_turns` (12), `daemon_projection` (11), `daemon_ops` (9); chat keeps its boxed pre-router path, interactive keeps its spawned-task pre-router path with transport-derived ownership; `projection_transport_real` 58/58 pass | pass | Cancellation sources, scheduler admission, transport-derived client ownership, and projection publication order untouched — preamble and both pre-routers are byte-equivalent logic |
| Work package D: reduce dispatcher/import surface, delete dead helpers, update docs and focused tests | 5,827-line inline match → ~110-line router; per-file unused-import cleanup (`cargo fix` + `ckroot` zero warnings); no dead helpers found (all moved arms call live owners); docs updated (§8); 2 new routing tests | pass | No new static guard: the routing test fails compilation-adjacent (new variants must be classified in `of`) and at runtime (representative pins); a raw line-count gate was explicitly rejected by the plan |
| Acceptance: dispatch concise enough to audit request-to-owner routing | Router is one screen per family delegation (§3); `of()` is the only routing table | pass | — |
| Acceptance: each extracted module has one coherent request responsibility | §3 family table; each module doc states its single responsibility and no-new-owner rule | pass | — |
| Acceptance: canonical owners and observable semantics unchanged | §5–§7: owner-by-owner review; 4,407 lib + 166 core + 58 projection-transport tests pass; error codes/envelopes untouched | pass | — |

## 3. Production implementation evidence

### Before/after ownership map

Before: one `impl CoreDaemon` block in `src/core/daemon.rs` (12,007
lines) containing the struct, all helpers, auth/audit preamble, chat and
interactive handlers, and a ~5,827-line `match request.payload` with 139
inline arms.

After (`wc -l` post-`cargo fmt`):

| Module | Lines | Request responsibility |
|---|---|---|
| `src/core/daemon.rs` | 6,287 | `CoreDaemon` struct + lifecycle/construction (M003 seam) + auth/audit preamble + chat handler + interactive runner + thin family router + shared `pub(crate)` helpers + existing tests |
| `src/core/daemon_family.rs` | 339 | `DaemonRequestFamily::of` (166-variant routing table) + `owner_module()` + routing/delegation tests |
| `src/core/daemon_assets.rs` | 129 | Asset refresh/status/capabilities (3 arms) |
| `src/core/daemon_providers.rs` | 472 | Eggpool + provider-connection lifecycle (19 arms) |
| `src/core/daemon_sessions.rs` | 994 | Session CRUD/selection/messages/import-export-template (20 arms) |
| `src/core/daemon_turns.rs` | 761 | Turn control, agent/model selection, permission/question, transport lifecycle (12 arms) |
| `src/core/daemon_jobs.rs` | 972 | Jobs, schedules, runs, tool programs, legacy tasks (27 arms) |
| `src/core/daemon_projects.rs` | 588 | Project catalog, workspaces, worktrees, daemon/workspace snapshots (20 arms) |
| `src/core/daemon_goals.rs` | 982 | Goals, todos, edit checkpoints, LSP preview apply (18 arms) |
| `src/core/daemon_projection.rs` | 890 | Projection replay + presence leases (11 arms) |
| `src/core/daemon_ops.rs` | 335 | Audit, memory, notifications (9 arms) |

139/139 top-level arms moved; the splitter dry-run asserted every arm's
header variants map to exactly one family and the classified total equals
the arm count, so no arm was lost or split across families.

### Moved families (arm counts)

assets 3, providers 19, sessions 20, turns 12, jobs 27, projects 20,
goals 18, projection 11, ops 9. Chat (17 variants) and interactive-process
(10 variants) are classified in `of()` but continue to be served by their
pre-existing pre-router paths (`handle_chat_request` boxed,
`run_interactive_request` on a spawned task), preserving the documented
stack/cancellation rationale verbatim.

### Landed changes

- New: `src/core/daemon_family.rs`, `daemon_assets.rs`,
  `daemon_providers.rs`, `daemon_sessions.rs`, `daemon_turns.rs`,
  `daemon_jobs.rs`, `daemon_projects.rs`, `daemon_goals.rs`,
  `daemon_projection.rs`, `daemon_ops.rs`; declared in `src/core/mod.rs`.
- `src/core/daemon.rs`: giant match replaced by the thin router
  (destructure envelope → `DaemonRequestFamily::of` → one `Box::pin`
  delegate per family, preserving per-family future boxing discipline);
  22 shared helpers widened `private` → `pub(crate)` so every family
  calls the same canonical implementation (no logic change); 9 free
  DTO/error helpers widened to `pub(crate)` and imported by exactly the
  family that uses them.
- Family handler signatures take owned
  `(request, request_id, trusted_client_id, authority, authz_decision)`
  so moved arm bodies compile verbatim, including their original
  `&authority`/`&authz_decision` borrow shapes and `return`/`?` control
  flow.
- Docs: `architecture/core.md` (module inventory, extraction-target
  note, implementation notes, test coverage incl. corrected
  `turn_submit_uses_injected_runtime` line ref),
  `architecture/worktree.md` (daemon call-site ref → `daemon_projects`),
  `.opencode/skills/core/SKILL.md` (request-family routing table +
  maintenance rules).
- Tests: `request_family_routes_each_coherent_family` (pins one
  representative variant per family, both session-attach spellings,
  `Initialize` → turns, chat/interactive pre-router classification) and
  `thin_dispatcher_delegates_to_family_handlers` (pool-less daemon through
  every capability probe + legacy `TaskList` rejection; fails on any
  `unimplemented` fallthrough).

Deliberate non-change: shared helpers were widened, not relocated —
relocating them into families would duplicate or fork canonical
implementations (e.g. `session_dto`, audit emitters, projection snapshot
builder). Helper relocation, if ever wanted, belongs to a future
correctness-neutral pass, not to this milestone. No raw line-count guard
was added per the plan's explicit prohibition.

## 4. Verification executed

### Commands run

```bash
cargo test -p codegg --lib core::daemon_family
cargo test -p codegg --lib core::
cargo test --features server --test projection_transport_real
CARGO_BUILD_JOBS=1 cargo test -p codegg --lib --locked -- --test-threads=4
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
scripts/verify.sh quick
```

### Results

| Command | Result |
|---|---|
| `cargo test -p codegg --lib core::daemon_family` | pass — 2 passed (new routing + delegation tests) |
| `cargo test -p codegg --lib core::` | pass — 166 passed, 0 failed |
| `cargo test --features server --test projection_transport_real` | pass — 58 passed, 0 failed (transport/cancellation/publication regression evidence) |
| `cargo test -p codegg --lib --locked` (full root suite) | pass — 4407 passed, 0 failed |
| `cargo fmt --all -- --check` | pass |
| `cargo clippy --workspace --all-targets --all-features -- -D warnings` | pass — zero warnings |
| `scripts/verify.sh quick` | pass (builtin-agents check, core-boundary, sandbox contract, execution-ownership, workspace `cargo check --all-targets`) |

The plan's literal `cargo test --workspace core --no-fail-fast` has no
matching package in this repo (no workspace member named `core`); the
equivalent coverage above (full root lib suite plus the named
projection-transport suite) was run instead and recorded here. No
`codegg-core` code changed, so no core-crate test sweep was required;
`check-core-boundary.sh` passes via `verify.sh quick`. All verification
is local; no `CI / verify` hosted evidence is claimed.

## 5. Invariant review

| Plan invariant | Evidence it remains true |
|---|---|
| One daemon state owner | `CoreDaemon` struct, fields, and construction untouched; families are `impl CoreDaemon` methods on the same state; no second daemon type |
| One scheduler | No scheduler code touched; job/schedule/run arms moved verbatim with identical submission-service calls |
| One session/project/workspace authority | Session/project/workspace arms moved verbatim; `WorkspaceRegistry`, `WorkspaceServiceRegistry`, `ProjectActivationRegistry` wiring untouched |
| Transport-derived client ownership | Auth preamble, `request_authority_for_client`, `interactive_authority_for`, and both pre-routers unchanged; family handlers receive (not derive) authority |
| Request/response compatibility | No DTO, variant, envelope, error-code, or capability-negotiation change; moved arms return the same `CoreResponse` shapes |
| Projection publication order | `projection_snapshot_for_session`, seam publish calls, resume/ack semantics moved verbatim; 58 real-transport projection tests pass |
| Cancellation and joined teardown | Turn/interactive cancellation tokens, receiver lifetimes, queue bounds, scheduler permits untouched; Drop impls untouched |
| No request DTO becomes authority | Router classifies by variant only; payloads still carry no identity; `of()` takes `&CoreRequest` and returns a family, never a principal |

## 6. Failure and recovery review

This milestone changes no production failure, cancellation, restart, or
contention behavior (plan §8). Every moved arm keeps its original
cancellation source (turn tokens flow through the same `TurnSubmit` body),
scheduler admission path (job submit through the same submission facade),
idempotency keys, lock ordering (workspace-service acquire order inside
the same bodies), and replay/publication semantics (projection arms call
the same seam). The boxed-per-family router preserves the pre-existing
stack discipline (the dispatch future holds one box, as the authorization
preamble comment requires). Stop conditions never triggered: no extraction
required new durable state, protocol semantics, scheduler authority,
DTO-derived authority, or a coordination framework — chat/interactive
stayed on their dedicated paths for exactly this reason.

## 7. Migration and compatibility review

- Schema migrations: none (no tables touched; `STORAGE_LAYOUT_VERSION`
  unchanged).
- Wire protocol: unchanged — all 166 `CoreRequest` variants, envelopes,
  error codes, and capability responses are byte-identical; the
  `unimplemented` fallthrough contract is preserved in every family
  catch-all and the router's unreachable Chat/Interactive arm.
- Configuration: no keys added, removed, or renamed.
- Stored runs: no format change; historical-name readers untouched.
- Public paths: `CoreDaemon`, `handle_request`, `handle_request_for_client`
  unchanged; new modules are crate-internal request handling (`pub mod`
  declaration, `pub(crate)` handlers) with no new public API.
- Rollback: revert the single M002 commit; the router and families land
  together so there is no intermediate mixed-dispatch state.

## 8. Security review

- No authorization seam changed: the M003 gate (`authorize_request` +
  denial/audit emitters) runs before the router exactly as before; denied
  requests still return with zero side effect.
- No new filesystem, network, or process-spawn site: pure code motion
  plus `pub(crate)` visibility; `check_execution_ownership.py` passes.
- Transport-derived authority preserved: family handlers cannot mint
  principals; presence/projection artifact paths receive the same boxed
  canonical contexts.
- No secret, credential, or redaction boundary touched; rotation/refresh
  arms moved verbatim with identical error mapping.
- No denial-of-service surface change: same bounded tasks, same queue
  bounds, same permits; per-family boxing keeps dispatch futures small.

## 9. Documentation and operations

- Updated: `architecture/core.md` (family inventory, M002-done
  extraction note, implementation notes, new test coverage),
  `architecture/worktree.md` (daemon call-site ref),
  `.opencode/skills/core/SKILL.md` (routing table + maintenance rules).
- New: `src/core/daemon_family.rs` module docs carry the canonical
  before/after ownership table.
- Historical closure records untouched (no rewrite of M001 conclusions).
- No new operator diagnostics, static guards, or recovery instructions
  required. The deliberate non-addition of a line-count guard is
  recorded: `DaemonRequestFamily::of` plus the two routing tests enforce
  the ownership invariant instead.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| — | None | — | — |

No critical/high/medium/low findings remain. The shared-helper widening
(`private` → `pub(crate)`, §3) is an accepted, recorded consequence of
physical decomposition — not a finding: it changes no public API and no
runtime behavior.

## 11. Roadmap disposition

Milestone closed and the hard-blocked successor may proceed. M002 closure
unblocks Residual M003 (construction/lifecycle decomposition), whose plan
states M002 as its only hard dependency:

- M002: `ready` → `closed` (this record).
- M003: `blocked` (on M002) → `ready` — no other dependency gates it;
  its stop condition ("M002 not closed") no longer holds.

## 12. Registry updates

- `plans/registry.md`: residual-roadmap row `M001 closed, M002 ready` →
  `M001 closed, M002 closed, M003 ready`; M002 row removed from the
  dependency-ready table and recorded in the control-points table; M003
  row added to the dependency-ready table; execution-order gate 1 updated
  (M002 closed, M003 ready); blocked-work M003 row removed (unblocked;
  no other plan lists M002 as a hard/interface dependency, so the unblock
  audit moves nothing else to `ready`).
- `plans/subsystems/residual-runtime-consolidation-roadmap.md`: M002 row
  `ready` → `closed` with closure-record link; M003 row `blocked` →
  `ready`.
- `plans/implementation/residual-runtime-consolidation/002-core-daemon-request-family-decomposition.md`:
  status `ready for handoff` → `implemented` (closed via this record).
- `plans/implementation/residual-runtime-consolidation/003-core-daemon-construction-lifecycle-decomposition.md`:
  status `blocked` → `ready for handoff` (M002 hard dependency satisfied).
