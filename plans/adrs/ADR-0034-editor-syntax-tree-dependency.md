# ADR-0034: Syntax-awareness source for TUI editor structural editing

Status: proposed

Date: 2026-10-17

Decision owners: project maintainers

Related specification sections:

- `plans/000-long-term-specification.md`
- `architecture/lsp.md`
- `architecture/tui.md`
- `architecture/client.md`

Affected subsystem roadmaps:

- `plans/subsystems/tui-editor-helix-parity-roadmap.md` (gates M009 and the
  tree-sitter-marked slice of M006)
- `plans/subsystems/tui-editor-correctness-usability-post-closure-corrective-addendum.md`

## Context

The TUI editor's Helix-parity survey found that roughly twenty Helix bindings
are unreachable because CodeGG has no syntax-tree source. There is **no
tree-sitter dependency anywhere in the workspace** today — the gap is at the
dependency level, not a matter of unbound keys.

The unreachable bindings fall into two groups:

- **Selection manipulation:** `Alt-o`/`Alt-i` (expand/shrink to parent syntax
  node), `Alt-p`/`Alt-n` (previous/next sibling), `Alt-a` (all siblings),
  `Alt-I` (all children), `Alt-e`/`Alt-b` (parent node start/end).
- **Unimpaired navigation:** `]f`/`[f` (next/previous function), `]t`/`[t`
  (next/previous type), `]a`/`[a` (next/previous parameter), `]c`/`[c`
  (next/previous comment), `]T`/`[T` (next/previous test).

Text objects and surround (`m a`/`m i`, `m s`/`m r`/`m d`) are partially
reachable without a parser — word and delimiter-pair objects are computable
from bytes — but block objects and syntax-node-aware selection need one.

This decision matters now because
`plans/subsystems/tui-editor-helix-parity-roadmap.md` sequences M009 behind it,
and because the choice sets a durable dependency and ownership contract that
later milestones cannot cheaply reverse.

Existing architecture strongly constrains the answer:

- `egglsp` is the **authoritative** LSP crate; `src/lsp/` is a thin shim.
- ADR-0011 established that the TUI owns cursor/focus/viewport/presentation
  only, that no component gains filesystem or language-server authority, and
  that the daemon owns language-server interaction, access control, and
  warm/cold readiness.
- The predecessor editor roadmap established bounded viewport work: rendering
  must never scan whole logical lines, and expensive indexing is amortized or
  moved off the render path.
- ADR-0021 already established the precedent that a model-facing LSP surface is
  **read-only** and must not gain edit or command execution.

## Decision drivers

- Preserve ADR-0011: the frontend must not gain resource ownership, and no
  rendering path may take unbounded parse cost.
- Preserve the daemon's authority over external processes and readiness state;
  warm/cold LSP lifecycle is not a frontend concern.
- Reuse the existing authorized, revision-bound LSP read transport rather than
  inventing a second structural-read protocol.
- Keep the editor usable when no language server is installed or running.
- Avoid a heavyweight permanent dependency in a workspace whose crate count and
  build cost are actively managed
  (`plans/subsystems/ci-test-throughput-optimization-roadmap.md` is closed on
  build and test cost).
- Make graceful degradation explicit and user-visible rather than silent.

## Considered options

### Option A — Adopt tree-sitter as a frontend-owned parse dependency

Tree-sitter grammars are vendored/instantiated in the TUI process. Structural
reads become local, synchronous, and serverless, matching Helix's own behavior:
structural editing works with no language server installed.

Benefits:

- Matches Helix semantics most closely; users get `TS` bindings with no server.
- No daemon round trip, so `Alt-o` / `Alt-i` feel instantaneous.
- No new protocol; structural reads are pure frontend computation.
- Bounded cost is achievable: parse lazily, cache per revision, and walk only a
  bounded window rather than the whole tree.

Costs:

- Adds a C-parser dependency plus per-language grammars to a workspace that
  currently ships none — a meaningful build-time and binary-size cost against a
  closed throughput roadmap.
- Places a parser in the TUI process, which brushes against ADR-0011's "no
  component gains language-server authority". Tree-sitter is a library, not a
  server, so this is defensible, but it must be stated explicitly rather than
  assumed.
- Grammar coverage is per-language and must be curated; an unbundled language
  renders plain, exactly as today's syntax highlighting already does.
- Introduces a second, competing notion of syntax structure alongside
  `egglsp`. Divergence between tree-sitter structure and LSP semantic structure
  would be a new class of user-visible inconsistency.

Failure modes:

- Parse cost on a large or pathological file stalls the render path unless
  strictly bounded and cancelled — the risk the predecessor roadmap's C002
  milestone already had to defend against for viewport reads.
- A malformed or partially-written buffer produces a parse error tree; every
  structural binding must degrade rather than panic.
- Grammar version drift against the Rust binding becomes a maintenance tax.

### Option B — Extend the existing daemon-owned LSP read surface with structural reads

Add operations to `LspReadOperation` (or a sibling structural-read type) for
syntax-node/symbol ranges, backed by `egglsp`, carrying the same
revision-binding and authorization contract as the current reads.

Benefits:

- Consistent with ADR-0011 and ADR-0021: the daemon already owns language-server
  interaction, access control, and readiness; structural reads inherit that for
  free rather than creating a parallel authority.
- Reuses the existing authorized, workspace-relative target validation, so
  structural navigation cannot become a path-escape vector.
- One notion of syntax structure instead of two.
- Grammar coverage is free — whatever the installed server supports.

Costs:

- Requires a server. With no server, no syntax-aware bindings; word and
  delimiter-pair text objects remain the ceiling.
- Network/daemon round trip per structural step, so `Alt-o`/`Alt-i` chains are
  slower than Helix's local implementation.
- `LspReadOperation` is deliberately read-only and small; structural reads grow
  it, and `DocumentSymbol`-style payloads are already potentially large, so
  bounding and truncation disclosure are required.
- Extending a protocol is a harder reversal than removing a local parser.

Failure modes:

- A slow or cold server makes `Alt-o` feel broken rather than merely slow; needs
  an explicit unavailable/not-ready state, not a silent no-op.
- LSP symbol responses are coarse relative to tree-sitter nodes, so exact
  Helix semantics may be approximated. That approximation must be documented
  rather than implied.

### Option C — Defer; ship only non-syntax parity

Land M001–M008, M010–M012 and their non-TS slices, and leave every
syntax-dependent binding unbound for now.

Benefits:

- Zero new dependency, zero protocol change, zero ownership question.
- Keeps the roadmap's high-value, low-cost half moving immediately.

Costs:

- Leaves the roadmap permanently unable to close its completion definition as
  written, since ~20 bindings stay unbound.
- Defers the decision rather than making it, so M009 stays blocked with no
  owner for the blocker.

Failure modes:

- Silent drift: the keymap is presented as "Helix" while a visible fifth of it
  does nothing, which is worse than an honest gap.

### Option D — Hybrid: Option A behind Option B

Use daemon LSP structural reads when a server is attached, and a bounded
tree-sitter parse only when none is.

Benefits: best availability; degrades to Helix-like serverless behavior.

Costs: two structural notions, two cost profiles, and a correctness surface
where they disagree. Given that divergence would be user-visible and hard to
diagnose, this is rejected as disproportionate for the value delivered.

## Decision

**No option is selected by this record. Option B is recommended, and its
acceptance is the hard gate for M009 and the tree-sitter-marked slice of M006.**

If Option B is accepted:

- Structural reads are added to the daemon-owned LSP surface in `egglsp` and
  exposed to the TUI through the existing authorized, revision-bound read
  transport. **No new authority boundary is created.**
- `LspReadOperation` remains **read-only**. Structural reads expose ranges; they
  do not format, rename, execute code actions, or perform any mutation. LSP
  write operations remain out of scope under ADR-0021 and ADR-0028.
- The frontend gains **no** parser dependency and **no** parse cost. Bounded
  window reads, truncation disclosure, and cancellation follow the precedent set
  by the closed viewport milestone.
- Word and delimiter-pair text objects are implemented frontend-side from bytes
  and are explicitly **not** gated on this decision — they do not need a
  parser. Only block and syntax-node-aware objects are gated.
- Every syntax-dependent binding reports an explicit unavailable/not-ready state
  when no server is attached, matching the existing diagnostics and hover
  panels. Silent no-ops are prohibited.
- Where LSP symbol granularity is coarser than Helix's notion (notably
  `Alt-o`/`Alt-i` node expansion and `Alt-I` all-children), the approximation is
  documented in `docs/tui.md` rather than implied.

Selecting Option A instead would require a separate ADR recording the
dependency adoption and the explicit reinterpretation of ADR-0011's frontend
ownership boundary, plus a build-cost reassessment against the closed CI
throughput roadmap.

Selecting Option C is valid but must be recorded as an explicit deferral with
the roadmap's completion definition amended, so the gap stays owned rather than
becoming a silent permanent omission.

## Consequences

### Positive

- The decision gets made explicitly rather than by default, with an owner.
- Option B preserves the existing authority model and adds no dependency to a
  build-cost-sensitive workspace.
- Word and delimiter-pair text objects become unblocked regardless of outcome.
- Graceful degradation becomes a specified requirement rather than an accident.

### Negative

- Under Option B, syntax-aware editing requires an installed language server, so
  users without one get strictly less than Helix.
- Structural reads grow the LSP protocol surface and require bounding and
  truncation work that must be done properly, not retrofitted.
- Under Option B, exact Helix node semantics may be approximated, which is a
  permanent fidelity gap.

### Neutral or deferred

- LSP write operations (`=` format, `space r` rename, `space a` code action) are
  deferred under ADR-0021 regardless of this outcome.
- Macros, named registers, and shell filters are deferred by
  `plans/subsystems/tui-editor-helix-parity-roadmap.md` §3.
- `Ctrl-w` window mode remains a pane-layout workstream, not a keybinding one.

## Compatibility and migration

No storage migration and no durable schema change under any option. The
selection set, prefix state, and history remain frontend presentation state.

Under Option B the protocol change is additive: existing `LspReadOperation`
consumers and older frontends keep working, and a frontend that does not know
the new operation reports it as unsupported rather than failing. No negotiation
version bump is required for an additive read operation, but the daemon must
not assume a frontend understands it.

## Security and reliability implications

- **Authorization.** Structural reads must carry the same workspace-relative
  target validation as the current reads. Structural navigation must never
  become a path-escape vector; ADR-0018's workspace-relative navigation identity
  applies unchanged.
- **Resource ownership.** Option A places a parser in the frontend process and
  requires strict parse bounding plus cancellation to keep a large file from
  stalling the render path. Option B keeps that cost on the daemon, where it is
  already governed.
- **Denial of service.** A structural-read payload must be bounded and truncated
  with disclosure, consistent with the existing bounded-response work. An
  unbounded symbol-tree response is a denial-of-service vector regardless of
  option.
- **No new authority.** Structural reads are read-only. They must not enable
  command execution, code actions, or any mutation, consistent with ADR-0021.
- **Reliability.** A cold or failing server must surface an explicit
  unavailable/not-ready state. A structural binding that silently does nothing
  is treated as a defect.

## Verification

Evidence required before this ADR can be marked accepted:

1. A recorded decision with a named owner, selecting exactly one option.
2. For Option B: a bounded structural-read contract in `egglsp` with
   truncation behavior and revision binding, plus explicit not-ready and
   unavailable outcomes.
3. For Option A: a dependency adoption record, an explicit reinterpretation of
   ADR-0011's frontend ownership boundary, and a build-cost measurement against
   the closed CI throughput roadmap's baseline.
4. For either: evidence that a large or adversarial file cannot stall the render
   path via structural navigation, following the precedent of the closed
   viewport milestone.
5. For either: a negative test proving a structural binding reports an
   unavailable state rather than silently no-opping when no server is attached.

Acceptance is a maintainer decision. The presence of this proposed record is
not acceptance and confers no implementation authorization, matching the
precedent set by ADR-0030.

## Supersession

None.
