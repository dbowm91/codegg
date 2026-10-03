# Desktop Frontend and IDE Foundation Post-Closure Corrective C001 — M004 Prompt-Composition Error Boundary and Hosted Strict Closure

Status: implemented

Repository baseline: `995d5cf8dd74009ea12cbcb9220594b49998853f`

Source corrective addendum:

- `plans/subsystems/desktop-frontend-ide-foundation-post-closure-corrective-addendum.md`

Historical M004 closure:

- `plans/closure/desktop-frontend-ide-foundation/004-status.md`

Current hosted verdicts on the baseline:

- `Desktop E2E` `37130587855` — SUCCESS
- root `CI / verify` `37130587841` — FAILURE at Workspace Clippy

Primary class: invariant / maintainability / verification corrective

## 1. Objective

Repair the M004 daemon prompt-composition helper so its internal error channel is narrow and domain-specific rather than `CoreResponse`, preserve the exact public `SessionPromptSubmit` failure contract, obtain green hosted root CI plus Desktop E2E on the corrected tree, and reconcile desktop planning state.

Do not change desktop/session behavior except where required to restore the intended internal ownership boundary.

## 2. Confirmed defect

Current helper:

```rust
pub(crate) async fn resolve_prompt_submit_composition(
    &self,
    session_id: &str,
    text: &str,
) -> Result<
    (
        String,
        Vec<crate::protocol::dto::Agent>,
        Vec<crate::protocol::dto::ProviderMessage>,
    ),
    CoreResponse,
>
```

Hosted stable Clippy reports:

```text
src/core/daemon_turns.rs:1160:10
clippy::result_large_err
Err variant is at least 1072 bytes
```

`CoreResponse` is a protocol response sum type. Using the whole enum as an internal composition error:

- inflates the result representation;
- couples composition logic to unrelated response variants;
- makes a private helper responsible for protocol construction;
- breaks routine hosted `-D warnings` verification.

The correct boundary is a narrow private error converted to `CoreResponse::Error` by the request handler.

## 3. Invariants

- `SessionPromptSubmit` wire/API behavior is unchanged.
- Stable failure codes are preserved.
- Prompt bodies and provider secrets are never logged.
- Durable model selection remains daemon authority.
- Workspace root for agent resolution remains session-bound daemon authority.
- Desktop/renderer never supplies provider/agent identity.
- Active-turn at-most-once behavior remains unchanged.
- No generic protocol-error refactor is introduced.
- Root MSRV remains 1.89.
- Desktop Tauri workspace remains separately owned at Rust 1.90.
- Historical M004 closure record remains immutable.

## 4. Work package A — Freeze the public error contract

Before changing the helper return type, enumerate every `CoreResponse::Error` path emitted by `resolve_prompt_submit_composition`.

Add focused assertions for code + meaningful message shape.

At minimum preserve:

```text
prompt_text_empty
prompt_text_too_long
model_unselected
selection_lookup_failed
model_unresolved
session_unbound
agents_unresolvable
agents_invalid
```

If source review finds more codes, include them.

Tests must exercise request-level mapping for representative failures so the internal refactor cannot silently change the wire contract.

## 5. Work package B — Introduce the narrow internal error type

Preferred shape:

```rust
#[derive(Debug)]
struct PromptCompositionError {
    code: &'static str,
    message: String,
}

impl PromptCompositionError {
    fn into_core_response(self) -> CoreResponse {
        CoreResponse::Error {
            code: self.code.to_owned(),
            message: self.message,
        }
    }
}
```

A private enum is acceptable if it keeps the stable code mapping obvious and compact.

Requirements:

- no `CoreResponse` stored inside the error;
- no `Box<CoreResponse>` as the primary solution;
- no `#[allow(clippy::result_large_err)]`;
- no anyhow/string-only conversion that loses typed/stable codes;
- helper success tuple remains unchanged unless an independent compiler warning requires a local cleanup.

Keep the type private to the daemon turns module unless a second real consumer appears.

## 6. Work package C — Move protocol construction to the request boundary

Update the `CoreRequest::SessionPromptSubmit` arm so it:

1. validates/authorizes exactly as today;
2. calls the narrow composition helper;
3. maps `PromptCompositionError` to `CoreResponse::Error`;
4. delegates successful composition to the existing `TurnSubmit` body;
5. preserves existing audit and active-turn behavior.

Do not duplicate `TurnSubmit` execution logic.

## 7. Work package D — Focused regression matrix

Required:

- empty prompt;
- text bound;
- no model;
- legacy unresolved model;
- selection-store failure;
- unknown/unbound session;
- agent resolution failure;
- agent DTO conversion failure if fixtureable without production hooks;
- successful composition;
- successful `SessionPromptSubmit` delegation;
- duplicate active submit rejected;
- deterministic M004 mock-provider live turn still reaches permission + deny + completion.

Prefer existing fixtures. Do not add a second model/provider test harness.

## 8. Work package E — Desktop E2E trigger ownership

Audit `.github/workflows/desktop-e2e.yml`.

The M004 live-turn trajectory now depends on shared root modules outside `apps/desktop/**`. Ensure the path filter includes the narrow critical surfaces proven during M004, including the prompt-composition handler.

At minimum assess:

- `src/core/daemon_turns.rs`;
- `src/core/transport/daemon_socket.rs`;
- `crates/codegg-core/src/projection_replay/**`;
- `crates/codegg-core/src/storage/**`;
- `crates/codegg-client/**`;
- `crates/codegg-protocol/**`;
- `apps/desktop/**`;
- the workflow itself.

Do not use a blanket `src/**` trigger if narrower ownership paths cover the vertical slice.

If the workflow already covers these through broader but intentional patterns, document that and make no change.

## 9. Work package F — Planning/current-authority cleanup

Preserve:

- `plans/closure/desktop-frontend-ide-foundation/004-status.md` as historical closure evidence.

Create on completion:

- `plans/closure/desktop-frontend-ide-foundation-post-closure-corrective/001-status.md`.

Update current authority:

- post-closure corrective addendum C001 → closed;
- foundation roadmap M004 → strict closed with C001 pointer;
- foundation roadmap milestone-status table → current values;
- registry foundation row → M004 strict closed;
- registry M003 lifecycle-corrective row → closed;
- stale M003 C001/C002 “hosted pending” wording → historical/closed-current wording without rewriting old closure records;
- registry C001 row → closed;
- M005 remains deferred but becomes dependency-eligible for its required fresh audit.

## 10. Required verification

Local minimum:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test -p codegg --lib
cargo test -p codegg --test single_daemon_lifecycle --locked
scripts/verify.sh quick
git diff --check

cd apps/desktop
npm ci
npm run typecheck
npm test
npm run bindings:check
rustup run 1.90.0 cargo test --manifest-path src-tauri/Cargo.toml --locked --lib
```

Also run the focused M004 live-turn/projection tests affected by the production helper.

Hosted closure evidence:

- root `CI / verify` SUCCESS on corrected head;
- `Desktop E2E` SUCCESS on corrected head.

If a later unrelated hosted flake appears, reproduce/classify it using the repository's existing closure rules. Do not call C001 closed while the corrected head has a reproducible M004-owned failure.

## 11. Acceptance criteria

C001 is complete when:

1. the internal composition helper uses a narrow private error.
2. no Clippy suppression is used.
3. no protocol enum is boxed merely to silence the warning.
4. all stable error codes are regression-tested.
5. success behavior is unchanged.
6. root Clippy passes on current hosted toolchain.
7. hosted root CI is green.
8. hosted Desktop E2E is green.
9. Desktop E2E trigger ownership covers the shared daemon/client/projection paths the vertical slice depends on.
10. roadmap/registry current authority is reconciled.
11. M003 corrective is current-state closed.
12. no unresolved high/medium M004 correctness finding remains.

## 12. Stop conditions

Stop and register a wider corrective if:

- multiple unrelated daemon handlers use protocol enums as oversized internal error channels and a shared domain-error abstraction is justified;
- error-code compatibility requires a protocol migration;
- the live-turn E2E reveals a new projection/storage/controller correctness defect;
- fixing the helper changes model/provider/session authority semantics;
- the only path to green is weakening Clippy or excluding the code from CI.

## 13. Closure record

Create:

- `plans/closure/desktop-frontend-ide-foundation-post-closure-corrective/001-status.md`

Record:

- implementation commit(s);
- old/new helper signatures;
- stable error-code matrix;
- Clippy result;
- focused/local verification;
- hosted root CI run;
- hosted Desktop E2E run;
- workflow path-filter audit;
- current-authority planning reconciliation;
- M004 final disposition;
- M005 dependency disposition.

## 14. Handoff note

This is intentionally a small corrective. Do not use it as an opportunity to redesign `CoreResponse`, turn submission, provider selection, or the desktop bridge.

Once C001 is green and closed, the next desktop work should be a fresh M005 document/buffer/LSP ownership audit, not more M004 feature expansion.
