# TUI editor Helix parity Milestone 013 — LSP read delivery and editor reachability

Status: ready for handoff
Repository baseline: `84440fe7f` (`feat/auto-20261018-12255c45`)

Source roadmap: `plans/subsystems/tui-editor-helix-parity-roadmap.md` §7 M013
Long-term requirements: `plans/000-long-term-specification.md` (IDE-grade
language intelligence); `plans/002-long-term-roadmap.md` (LSP capability
sequencing)
Applicable ADRs: `plans/adrs/ADR-0011-editor-document-ownership-and-frontend-replication.md`,
`plans/adrs/ADR-0012-native-lsp-read-surface-delivery-and-authorization.md`
Primary class: invariant (delivery correctness), enabling capability (reach)

> **Note on numbering.** This milestone is numbered **013** even though no M001–M003
> implementation plan shares that number, because it is a **corrective
> insertion** ahead of M010 rather than a successor to it. M010 depends on it.
> The number is chosen to sort after the already-closed 001–003 without
> implying it depends on un-planned M004–M012. Per the planning process, a
> corrective pass is a new plan, not an amendment to the closed milestones that
> failed to notice the defects.

## 1. Objective

Make the editor's existing LSP read surface **actually deliver**, then bind it
to keys.

Two defects, both verified against source at this baseline, mean the read path
that F2–F9 already call returns `NotReady` in every ordinary configuration.
Neither is a missing feature; both are undriven or ambiguous delivery.

1. **Diagnostics are never driven.** `publish_changed_diagnostics` and
   `DIAGNOSTICS_POLL_INTERVAL` have **zero callers** outside their own
   definitions. The store therefore stays empty, so every diagnostics read is
   `NotReady` forever.
2. **Warm-client resolution is ambiguous on polyglot roots.** The read path
   resolves a client with no `server_id`, and the resolver returns `Err` when
   more than one client shares a root. That `Err` is mapped flatly to
   `NotReady`, making a live, perfectly healthy server indistinguishable from a
   cold one.

After fixing both, expose the read operations the editor cannot currently
reach at all — implementation, declaration, document highlight, signature
help, document formatting, rename, code actions — **within the existing
read-only contract**, and map the Helix bindings onto them.

## 2. Why this milestone is ready

- **No new LSP protocol surface for defect 1.** The publisher function already
  exists, is documented, returns a published-envelope count "so a caller — and
  a test — can tell 'nothing changed' from 'the publisher did not run'", and is
  warm-only by design. It is an undriven delivered function, which is a
  wiring gap, not an architectural decision.
- **No ADR needed for defect 2.** ADR-0012 §3 mandates warm-only reads. Resolving
  *which* warm client to use is an implementation detail that must **preserve**
  warm-only semantics; adding a `server_id` to the request DTO is additive and
  does not change the authorization model.
- **ADR-0011 preserved.** No app-side server startup. This milestone makes a
  daemon-owned service reachable, which is the direction ADR-0011 requires.

No closed milestone's behaviour is changed. This is a corrective insertion.

## 3. Current implementation evidence

### Defect 1 — the diagnostics publisher is undriven

`src/core/daemon_lsp.rs` declares:

- `pub const DIAGNOSTICS_POLL_INTERVAL: Duration = 750ms` — documented as
  deliberately not tight, because the underlying data is push-based (the server
  sends `textDocument/publishDiagnostics` and `egglsp` caches it).
- `pub async fn publish_changed_diagnostics(daemon, project_id, session_id) -> usize`
  — walks `client_keys()` (**existing** clients only, matching warm-only),
  publishes only on a digest change, and returns how many envelopes it
  published.

Both symbols have **zero callers**. Repository-wide search finds exactly one
production write to `daemon.lsp_diagnostics`:

```rust
let change = {
    let mut store = daemon.lsp_diagnostics.lock();
    store.tracker_mut(project_id).observe(&relative, payload)
};
```

and it lives **inside** `publish_changed_diagnostics`. That makes it the only
writer, and the function never runs.

Downstream consequence: `src/core/lsp_diagnostics_store.rs`'s `files_for`
returns `None` for any scope that was never observed, and
`src/core/daemon_lsp.rs`'s `diagnostics_result` maps that to `NotReady`. The
skill guidance is explicit that `NotReady` is **not** an empty clean set, so the
honest state today is "never ready" — which is indistinguishable in the UI from
a broken integration.

This defect was already suspected by the planning corpus: the `lsp-ide` skill
states "the current diagnostics change publisher has no production driver, so
do not promise ongoing push updates or add implicit polling in a
presentation-only change." That suspicion is confirmed here.

### Defect 2 — ambiguous warm-client resolution

`src/core/daemon_lsp.rs` read path:

```rust
// Warm-only. This is the whole of ADR-0012 §3: with no client running
// for this root the answer is `NotReady` — never a create, never an
// empty success that would read as "clean".
if service
    .find_existing_client_for_root_hint(Some(&root), None)
    .await
    .is_err()
{
    return Ok(LspReadResultDto::not_ready(operation));
}
```

`server_id` is `None`. In `crates/egglsp/src/service.rs`, the resolver's
terminal cases are:

```rust
if matching.len() == 1 { return Ok(...); }
if matching.is_empty() {
    return Err(LspError::ServerNotFound(format!(
        "no LSP client for root {}; provide file_path to initialize one", ...)));
}
Err(LspError::ServerNotFound(format!(
    "multiple LSP clients for root {}; specify server_id to disambiguate", ...)))
```

Two distinct failure modes collapse into one `NotReady`:

| Situation | Resolver | Editor sees |
|---|---|---|
| Cold root, no client | `Err(no LSP client)` | `NotReady` — correct |
| Warm root, 2+ clients | `Err(multiple LSP clients)` | `NotReady` — **wrong** |

Any workspace with rust-analyzer + gopls, or a JS/TS project where
typescript-language-server and vue-language-server both attach, has F3–F9
silently dead **today**, while the same project with a single server works.

The resolver's error text even tells the caller to `specify server_id`. Nothing
in the read request can.

### What `egglsp` already implements

The crate is substantially richer than the editor. Real implementations exist
for document highlights, implementation, declaration, signature help, semantic
tokens, document formatting, rename (prepare + `preview_workspace_edit` +
`apply_text_edits`), and code actions. All are reachable today **only** through
the agent's `lsp` tool — the editor cannot invoke them.

### What the editor can reach today

`crates/codegg-protocol/src/lsp.rs` defines `LspReadOperation` as read-only by
construction, with six variants (hover, definition, references, document
symbols, workspace symbols, semantic tokens). The editor binds these to F2–F9.

`src/tui/commands/editor.rs`'s mode-independent guard is the single interception
point for function keys, and `src/tui/editor_lsp.rs` binds results to active
editor identity and generation. That structure is sound and is reused, not
replaced.

### `LspPreviewApply` already exists

`LspPreviewApply` is wired end-to-end (`src/tui/app/mod.rs`, with
`LspPreviewApplyFinished`), and `crates/egglsp/src/edit.rs` provides
`preview_workspace_edit`, `preview_text_edits_for_file`, and `apply_text_edits`.
So the **preview and apply machinery for rename/code-action is already
present**; what is missing is binding it to a key and an authorization
decision, not building it.

## 4. Invariants that must not regress

1. **Warm-only reads (ADR-0012 §3).** No milestone work may cause a read to
   start a language server. A cold root returns `NotReady`. Fixing defect 2
   must not become a licence to lazily initialize.
2. **`NotReady` is not a clean set.** The skill contract is explicit. Distinct
   causes must stay distinguishable in the UI — that is the whole point of
   fixing defect 2.
3. **No app-side LSP startup (ADR-0011).** The frontend never launches or
   configures a server process.
4. **Read-only by construction.** `LspReadOperation` stays read-only. Adding
   *new read* variants (implementation, declaration, highlights, signature
   help, formatting-preview) is additive and in scope; adding a **write**
   variant is **not** — see §5.
5. **Workspace containment.** Location DTOs keep display `path` separate from
   `workspace_relative_path`. Only the daemon derives the relative locator, only
   after canonical containment under the session-authorized workspace. Never
   infer a path from a basename; never trust an LSP URI.
6. **Capability-gated.** An unsupported server capability must return the
   existing typed unsupported result, not an unqualified request.
7. **Generation binding.** Semantic tokens are a presentation overlay for the
   exact controller revision queried; malformed UTF-16 ranges, overlap,
   unsupported types, or revision change fall back to plain text.
8. **Editor key authority.** Function keys are already owned by the
   mode-independent guard; new Helix bindings must not collide with them.

## 5. Scope

### In scope

**Defect repair (the priority):**

- Drive `publish_changed_diagnostics` from a daemon-owned cadence at
  `DIAGNOSTICS_POLL_INTERVAL`, scoped to active sessions with warm clients.
- Add an optional `server_id` to the LSP read request; have the daemon prefer an
  explicit `server_id`, else fall back to unambiguous-by-extension resolution,
  else report a **typed ambiguous-client** result that is distinct from
  `NotReady`.

**Reachability, read-only:**

- Extend `LspReadOperation` with implementation, declaration, document
  highlight, and signature help.
- Expose document **formatting preview** and **rename preview** as read
  operations returning a preview DTO — no apply.

**Binding:**

- Map Helix's LSP bindings onto the now-reliable surface: `space k` hover,
  `space s` document symbols, `space S` workspace symbols, `space d`
  diagnostics, `gd` definition, `gy` type definition, `gi` implementation,
  `gr` references, `space h` select references, `space f`/`space F` file
  symbol picker, `]`/`[` diagnostic navigation (`space D`/`[D` workspace
  diagnostics where the server supports it).

**Explicitly requiring a maintainer decision (see §14):**

- `space r` rename and `space a` code action apply. The **preview** machinery
  exists; the **authorization model for an LSP-driven mutation** does not.
  `=` format is the same shape.

### Out of scope

- **LSP write authorization** (rename/code-action apply, `=` format). The plan
  may deliver preview; apply needs a maintainer decision recorded in an ADR
  before implementation. Roadmap §10 risk 2 tracks this.
- Any app-side server configuration or startup.
- Syntax-node selection (ADR-0034, proposed) — an independent blocker.
- Semantic-token retuning.
- Desktop/client LSP parity beyond keeping the protocol DTOs shared.

## 6. Required production changes

### Daemon diagnostics driver

- Drive the publisher from a daemon-owned task on a warm-client cadence. The
  task must be scoped so it stops when no warm client exists, must not survive
  as a busy loop, and must not run for sessions with no attached editor.
- Keep readiness separate from contents: preserve the last complete view until a
  `Ready` pull arrives. Do not replace a good view with an empty one because a
  poll found nothing new.
- The publisher's return value is the test seam — a test can distinguish "no
  change" from "did not run".

### Client disambiguation

- Add optional `server_id` to the read request DTO.
- Resolution order: explicit `server_id` → language/extension match
  (`find_server_for_extension` already exists in
  `crates/egglsp/src/server.rs`) → exactly-one-warm-client → typed ambiguity
  error.
- Map the ambiguity error to a **distinct** result, not `NotReady`. This is the
  user-visible half of the fix: someone in a polyglot workspace must be able to
  tell "no server" from "two servers, pick one".

### Protocol surface

- Extend `LspReadOperation` with the new read variants. Each maps to an
  existing `egglsp` call.
- Add preview DTOs for formatting and rename that carry a **previewed** edit, not
  an applied one.
- Every new variant is capability-gated at the daemon.

### Editor binding

- Add the Helix leaves to the **space-mode leaf table** introduced in
  `plans/implementation/tui-editor-helix-parity/005-textual-motions-and-goto-mode.md`
  W1, and the `g`-prefix leaves to the goto table. Do not build a second prefix
  mechanism.
- Each LSP leaf must **degrade visibly** when no server is attached: the status
  row says so. A silently ignored key is the M002 failure mode repeated.
- F2–F9 must keep working. Helix bindings are additive; if a collision is
  unavoidable, record the decision rather than silently shadowing.

## 7. Ordered work packages

**W1 — Drive the diagnostics publisher.** Add the daemon-owned cadence; add a
test that runs a warm client, publishes, and asserts the store received an
observation and the read path returns `Ready` with contents. **Before
implementing, demonstrate the defect**: a test asserting today's `NotReady`
behavior, per the `check_daemon_cwd_usage.py` self-test precedent.
Acceptance: a test proves both the old and new behaviour.

**W2 — Client disambiguation.** Add `server_id`; add a **two-server same-root
regression test** that fails today. The test must exercise the real resolver, not
a mock.
Acceptance: polyglot-root test green; ambiguity surfaces as a distinct result.

**W3 — Protocol extension.** New read variants + preview DTOs; capability
gating; client round-trip.
Acceptance: `cargo check -p codegg-protocol -p codegg-core -p codegg-client`.

**W4 — Editor binding.** Helix leaves via the M005 leaf tables; visible
degradation; F-key preservation.
Acceptance: dispatch tests + `check_tui_editor_key_authority.py` green.

**W5 — Docs.** §12.

**W6 — Full verification.** §11.

## 8. Failure, cancellation, restart, contention semantics

- **Cold root** stays `NotReady`. Never lazily start a server (ADR-0012 §3).
- **Ambiguous root** is a distinct typed result; the status row must name the
  conflict, not say "no server".
- **Poller lifecycle**: the diagnostics driver must terminate when the daemon
  shuts down and when the last warm client goes away. It must not be a
  process-global leak.
- **Readiness vs contents**: a `NotReady` pull must preserve the last complete
  view. An empty `Ready` replaces it — those are different events.
- **Concurrency**: `daemon.lsp_diagnostics` is behind a mutex; the poller and
  the read path must not deadlock or starve. The read path must never hold the
  lock across an await on the LSP service.
- **Restart**: diagnostics are derived state, not durable. After a daemon
  restart the store is empty and reads return `NotReady` until the next
  publish. That is correct; do not persist it.

## 9. Compatibility and migration

- Additive protocol variants. Older clients must tolerate unknown read
  operations via the existing typed unsupported result.
- The disambiguation change **alters observable behaviour** in the polyglot case:
  a binding that returned `NotReady` may now succeed. This is the fix, and it
  belongs in `CHANGELOG.md`.
- No storage or migration.

## 10. Required tests

- **Defect-regression tests** (the important ones):
  - warm client + driven publisher → `Ready` with contents;
  - two warm clients on one root + no `server_id` → ambiguous result (fails
    today);
  - two warm clients + explicit `server_id` → correct client's result;
  - cold root → `NotReady`, and **no server process started** (assert via the
    warm-only contract).
- Poller lifecycle: terminates on last-client-gone and on daemon shutdown.
- Readiness-vs-contents: `NotReady` preserves the previous view.
- Capability gating: unsupported variant → typed unsupported.
- Containment: a location outside the session workspace is not openable; a
  display-only `path` with no relative locator is not opened.
- Semantic-token generation binding preserved.
- Editor: each LSP leaf degrades visibly with no server; F2–F9 unaffected;
  Insert mode never captures a leaf.
- Daemon LSP suites plus the existing `tests/lsp_read_surface_trajectory.rs`
  and `tests/lsp_completion_trajectory.rs` remain green.

## 11. Required verification commands

```bash
cargo fmt
cargo test -p codegg-core --lib authorization
cargo test -p codegg-client
cargo test --test lsp_read_surface_trajectory
cargo test --test lsp_completion_trajectory
python3 scripts/check_execution_ownership.py
python3 scripts/check_scheduler_bypass.py
python3 scripts/check_tui_editor_key_authority.py
scripts/verify.sh quick
```

Because this touches LSP and daemon delivery, run the LSP harness suite:

```bash
cargo test -p egglsp --features lsp-test-support --test scenario_engine
```

**Never** `--all-features` (pulls real-server tests). `verify.sh full` is
required before closure. If `lsp-real-server-tests` behaviour changes, that
needs separately installed servers and is out of scope for this milestone.

## 12. Documentation updates

- `architecture/lsp.md` — record the diagnostics driver, the disambiguation
  order, and the typed ambiguity result. This is the authoritative doc.
- `architecture/tui.md` — the editor's LSP keymap and its degradation contract.
- `docs/tui.md` — user-facing keymap.
- `docs/LSP.md` — user-facing behaviour.
- `.opencode/skills/lsp-ide/SKILL.md` — **it currently states the publisher has
  no production driver.** Once fixed, that sentence is false and must be
  updated in the same change; a skill that documents a fixed defect as current
  is worse than no skill.

## 13. Acceptance criteria (externally observable)

1. With a warm language server, the editor's diagnostics read returns `Ready`
   with real contents.
2. In a workspace with two warm servers, an LSP read resolves rather than
   reporting a false `NotReady`; with no `server_id` it reports a **distinct
   ambiguity**, not "no server".
3. An explicit `server_id` selects the correct server.
4. A cold root still returns `NotReady` and starts no process.
5. Every Helix LSP binding either produces a result or states visibly why it
   cannot.
6. F2–F9 continue to work unchanged.
7. No LSP **write** path exists: rename and code actions offer a preview only,
   or are unbound pending §14.

## 14. Stop conditions

Report rather than improvise if:

- Driving the publisher would require the frontend to start a server
  (violates ADR-0011), or would change warm-only semantics (ADR-0012 §3).
- Disambiguation cannot be resolved by extension/language in a realistic
  polyglot case, and adding a heuristic would risk reading the wrong server —
  report; a wrong-server LSP answer is worse than an honest ambiguity.
- The `egglsp` implementation for a requested capability turns out to be a stub
  or to ignore required inputs rather than honour them. **Probe behaviour, do
  not infer it from signatures.**
- Any write path appears to need an authorization model that has no accepted
  ADR. Stop and request the decision; do not invent one.
- The desired-column/leaf-table assumptions from M005 W1 do not hold. This plan
  depends on that table.

## 15. Closure evidence required

Implementation commits; §11 output labelled local vs CI; an explicit
before/after demonstration for **both** defects; the cold-root assertion that no
process started; the updated `lsp-ide` skill diff; and a note that
`architecture/lsp.md`'s previously-deferred items are now either delivered or
still deferred, with reasons.

## 16. Handoff notes

W1 and W2 are small, independent, and each closes a defect that makes the UI
lie. Do them first and separately — they are the reason this milestone exists.

W3–W4 are additive and can follow once the delivery is trustworthy. **Binding
keys before W1/W2 is the mistake this milestone exists to prevent**: a `space d`
that opens an always-`NotReady` panel looks broken in a way that is harder to
diagnose than an absent key.

Before W3, probe the `egglsp` implementation of each capability you plan to
expose. The crate's function signatures are broader than its behaviour, and the
planning corpus already contains one instance of a "delivered" symbol with no
driver.

For the write family, prepare the decision, not the code: the preview and apply
machinery exists; the authorization contract does not.