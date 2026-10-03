# Desktop Frontend and IDE Foundation — Post-Closure Corrective Addendum

Status: active

Repository baseline reviewed: `995d5cf8dd74009ea12cbcb9220594b49998853f`

Predecessor roadmap:

- `plans/subsystems/desktop-frontend-ide-foundation-roadmap.md`

Historical closure evidence:

- `plans/closure/desktop-frontend-ide-foundation-corrective/003-status.md` — M003 strict closure
- `plans/closure/desktop-frontend-ide-foundation/004-status.md` — M004 historical closure record

Current implementation:

- M004 implementation through `995d5cf8`
- hosted Desktop E2E run `37130587855` — SUCCESS on `995d5cf8`
- hosted root CI run `37130587841` — FAILURE on `995d5cf8`, Workspace Clippy

## 1. Purpose and corrective trigger

M004 was recorded as closed after its deterministic live-turn trajectory passed locally and the built-app Desktop E2E path was implemented. The closure revision then produced a hosted split verdict:

- `Desktop E2E` `37130587855`: green on the exact M004 closure head;
- root `CI / verify` `37130587841`: red on the exact same head, failing at `Workspace Clippy` before the workspace tests ran.

The Clippy failure is attributable to new M004 production code:

```text
src/core/daemon_turns.rs:1160
clippy::result_large_err
Err variant >= 1072 bytes
```

`resolve_prompt_submit_composition` currently uses `CoreResponse` as its internal error type:

```rust
Result<
    (String, Vec<Agent>, Vec<ProviderMessage>),
    CoreResponse,
>
```

This couples an internal composition helper to the entire protocol response enum and makes the hot result type unnecessarily large. Because routine hosted CI treats warnings as errors, M004 does not yet satisfy its own strict hosted-closure requirement.

A second, documentation-only defect is now visible: the desktop roadmap and registry disagree about the state of M003/M004, and the M003 lifecycle-corrective row remains marked active despite its addendum/C003 being closed.

This addendum preserves the historical M004 closure record. It does not rewrite that evidence. C001 is the current strict-closure authority.

## 2. Work classification

### Invariant

Internal prompt-composition failures use a narrow domain error type. Protocol response enums are constructed only at the daemon request boundary.

### Verification

The exact corrected revision must pass hosted root CI and the existing path-gated Desktop E2E workflow.

### Documentation

Current-authority roadmap and registry state must agree with the historical closure + post-closure corrective model.

### Capability impact

No user-facing capability change.

## 3. Non-goals

- Redesigning `CoreResponse`.
- Boxing large variants throughout the protocol.
- Adding `#[allow(clippy::result_large_err)]` to suppress the finding.
- Changing `SessionPromptSubmit` request/response semantics.
- Changing model selection, agent resolution, prompt text bounds, or at-most-once turn behavior.
- Reopening projection, permission/controller, artifact, E2E, or Tauri architecture.
- Implementing M005 document/buffer work.
- Expanding Windows support; M002 remains separately conditional.

## 4. Corrective milestone

### C001 — M004 prompt-composition error boundary and hosted strict closure

Status: ready.

Plan:

- `plans/implementation/desktop-frontend-ide-foundation-post-closure-corrective/001-m004-prompt-composition-error-boundary-and-hosted-closure.md`

C001 owns:

1. replacing the oversized internal `CoreResponse` error type;
2. preserving exact public error-code/message behavior;
3. hosted root-CI + Desktop-E2E confirmation on the corrected head;
4. current-authority roadmap/registry reconciliation;
5. strict M004 disposition and M005 dependency release decision.

## 5. Current evidence

Hosted root CI `37130587841` failed only after reaching Workspace Clippy:

```text
error: the Err-variant returned from this function is very large
--> src/core/daemon_turns.rs:1160:10
the Err-variant is at least 1072 bytes
-D clippy::result-large-err implied by -D warnings
```

The failure is not an environmental flake and is not external to M004.

The same head's Desktop E2E `37130587855` is green. Therefore C001 must preserve the product behavior proven there while repairing the Rust internal boundary.

## 6. Target error boundary

Introduce a private narrow error shape owned by the daemon turn-composition module, for example:

```rust
struct PromptCompositionError {
    code: &'static str,
    message: String,
}
```

or a small enum with equivalent wire mapping.

Requirements:

- no `CoreResponse` field inside the internal error;
- no boxed `CoreResponse` as the default fix;
- no generic anyhow/string-only error that loses stable error codes;
- conversion into `CoreResponse::Error { code, message }` happens at the `SessionPromptSubmit` request-handling boundary;
- existing public error codes remain byte-for-byte stable unless an existing test proves a correction is required.

Current codes to preserve include at least:

- `prompt_text_empty`;
- `prompt_text_too_long`;
- `model_unselected`;
- `selection_lookup_failed`;
- `model_unresolved`;
- `session_unbound`;
- `agents_unresolvable`;
- `agents_invalid`.

Search the full function before implementation and freeze every emitted code in tests.

## 7. Required implementation behavior

### Internal helper

`resolve_prompt_submit_composition` returns:

- success tuple unchanged; and
- narrow prompt-composition error on failure.

Do not change the success allocation/layout merely to address Clippy unless measurement or code clarity justifies it.

### Protocol boundary

The `CoreRequest::SessionPromptSubmit` handler maps the narrow error to the existing `CoreResponse::Error` response.

Authorization, audit, request family, at-most-once active-turn guard, and downstream `TurnSubmit` delegation remain unchanged.

### Diagnostics

Do not log prompt body, provider credentials, workspace secrets, or agent configuration contents while mapping errors.

## 8. Required regression tests

Add/adjust focused tests proving:

- empty prompt returns `prompt_text_empty`;
- over-bound prompt returns `prompt_text_too_long`;
- unselected model returns `model_unselected`;
- legacy unresolved model returns `model_unresolved`;
- selection lookup failure maps to `selection_lookup_failed`;
- unbound session maps to `session_unbound`;
- agent resolution/translation failures retain their stable codes;
- successful composition still produces the same model/agents/messages used by the live-turn path;
- `SessionPromptSubmit` still delegates to the identical turn-start path;
- duplicate active submit still fails closed/at-most-once as before.

Prefer tests at both the narrow helper boundary and one request-level mapping boundary where useful.

## 9. Verification requirements

The corrective is not closed by local Clippy alone.

Required local evidence:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test -p codegg --lib
cargo test -p codegg --test single_daemon_lifecycle --locked
scripts/verify.sh quick

cd apps/desktop
npm ci
npm run typecheck
npm test
npm run bindings:check
rustup run 1.90.0 cargo test --manifest-path src-tauri/Cargo.toml --locked --lib
```

Also retain the focused M004 prompt/projection regressions touched by the fix.

Required hosted evidence on one corrected revision containing C001:

- root `CI / verify` — SUCCESS;
- path-gated `Desktop E2E` — SUCCESS if triggered by the changed paths; if the workflow is not triggered because the production edit falls outside its current path filter, explicitly dispatch/run it or update the path filter only if `src/core/daemon_turns.rs` is legitimately part of desktop M004 behavior.

Because `SessionPromptSubmit` lives in `src/core/daemon_turns.rs` and is required by desktop live-turn E2E, the preferred disposition is to include the narrow daemon prompt path in the Desktop E2E workflow trigger set rather than rely on a manual exception.

Do not weaken root Clippy or remove `-D warnings`.

## 10. Workflow trigger audit

Audit `.github/workflows/desktop-e2e.yml` path filters.

The routine should rerun when desktop-critical shared daemon surfaces change, including at minimum the exact modules that own:

- `SessionPromptSubmit`;
- local Core transport/projection lifecycle;
- projection replay/storage semantics;
- `codegg-client`;
- protocol DTO/CoreRequest definitions;
- desktop app/harness.

Do not broadly trigger Desktop E2E on every repository change. Add only current M004 dependency paths that were proven production-critical by the closure work.

## 11. Documentation reconciliation

Do not rewrite `plans/closure/desktop-frontend-ide-foundation/004-status.md`.

Instead:

- this addendum becomes current corrective authority;
- roadmap M004 is recorded as historical closure with active/ready C001 strict-closure corrective;
- registry desktop foundation row reflects M004 conditionally closed/currently controlled by C001;
- registry M003 lifecycle-corrective row becomes `closed` because its addendum and C003 strict closure are closed;
- stale M003/C002 “hosted pending” wording is removed from current-authority roadmap/registry text while historical records remain untouched;
- M005 remains deferred until C001 closes;
- after green hosted evidence, create a C001 closure record and promote current M004 disposition to strict closed.

## 12. M005 unblock rule

Do not start the M005 implementation line while C001 is open.

After C001 closes:

- M004 is strict closed;
- the desktop foundation's M001–M004 closure boundary is satisfied on Linux/macOS;
- M005 becomes eligible for the fresh document/buffer/LSP ownership audit already required by the roadmap;
- M002's Windows operational condition does not block Linux/macOS M005 research unless M005 makes Windows-specific claims.

## 13. Stop conditions

Stop and register a separate corrective if:

- the Clippy fix exposes a wider `CoreResponse` ownership/design problem affecting multiple unrelated request handlers;
- preserving public error codes is impossible without a protocol change;
- Desktop E2E exposes a new M004 product defect after the error-boundary fix;
- the root CI failure moves to another reproducible M004 production defect;
- workflow-trigger correction would require broad permanent CI expansion beyond current desktop dependency paths.

## 14. Completion definition

C001 closes only when:

1. `resolve_prompt_submit_composition` no longer returns `CoreResponse` as its internal error type.
2. No `result_large_err` suppression is added for this helper.
3. Public `SessionPromptSubmit` error codes/messages retain tested compatibility.
4. Root Clippy passes with `-D warnings`.
5. Hosted root `CI / verify` is green on the corrected revision.
6. Hosted Desktop E2E is green on the corrected revision or an explicitly triggered equivalent revision with identical production tree.
7. Desktop E2E path filters cover the shared M004 daemon surfaces actually required by the live-turn path.
8. Current roadmap/registry state is reconciled.
9. M003 corrective is recorded closed in current authority.
10. No unresolved high/medium M004 correctness finding remains.

## 15. Status

| Corrective | Status | Plan | Blocker |
|---|---|---|---|
| C001 M004 prompt-composition error boundary + hosted strict closure | active | `plans/implementation/desktop-frontend-ide-foundation-post-closure-corrective/001-m004-prompt-composition-error-boundary-and-hosted-closure.md` | No code dependency blocker. Hosted root CI is currently red on the new M004 helper; M005 remains deferred until C001 closes. |
