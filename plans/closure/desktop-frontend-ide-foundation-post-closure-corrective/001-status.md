# Desktop Frontend and IDE Foundation Post-Closure Corrective C001 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/desktop-frontend-ide-foundation-post-closure-corrective/001-m004-prompt-composition-error-boundary-and-hosted-closure.md`

Source subsystem roadmap:

- `plans/subsystems/desktop-frontend-ide-foundation-roadmap.md#milestone-004--desktop-sessioncontrol-plane-vertical-slice`
- Current corrective authority: `plans/subsystems/desktop-frontend-ide-foundation-post-closure-corrective-addendum.md`

Repository baseline reviewed: `995d5cf8dd74009ea12cbcb9220594b49998853f`

Implementation commits:

- `33fce56f` — narrow prompt-composition error boundary, stable request error mapping, workflow path coverage, and selection-store failure regression.
- `738142d8` — closure-review status transition; hosted evidence is on this exact implementation tree.

## 1. Executive finding

C001 is complete and M004 is strict-closed. `resolve_prompt_submit_composition` now returns a private, narrow `PromptCompositionError`; the request boundary constructs the same public `CoreResponse::Error` codes/messages. Hosted root CI and the built-app Desktop E2E workflow both passed on corrected head `738142d8`. No M004 high- or medium-severity finding remains.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Narrow internal prompt-composition error | `src/core/daemon_turns.rs`; workspace Clippy | pass | No protocol enum is retained in the helper error. |
| Stable validation and selection error codes | `session_prompt_submit_rejects_blank_and_overlong_text`, `session_prompt_submit_fails_closed_without_selection`, `session_prompt_submit_legacy_reference_fails_closed`, `session_prompt_submit_maps_selection_store_failure` | pass | Covers empty, over-bound, unselected, legacy unresolved, and storage lookup failure mapping. |
| Unbound session maps to `session_unbound` | `session_prompt_submit_maps_unbound_session_failure` | pass | Forces the authoritative context resolver boundary to fail after a valid durable selection resolves. |
| Request boundary mapping and existing turn path | `session_prompt_submit_maps_selection_store_failure`, `session_prompt_submit_resolves_selection_and_submits`, duplicate-active-submit regression | pass | Request authorization and shared `TurnSubmit` path remain in use. |
| Agent resolution/conversion error codes remain explicit | helper mappings in `resolve_prompt_submit_composition` | pass with fixture limitation | `agents_unresolvable` covers resolution failure/empty agents; `agents_invalid` covers DTO conversion failure. The strongly typed resolved-Agent conversion failure has no current natural fixture, and no production hook was added solely to force an impossible state. |
| Desktop E2E workflow watches shared prompt/replay/storage owners | `.github/workflows/desktop-e2e.yml` path filters | pass | Includes daemon turns, projection replay, and storage paths used by the desktop trajectory. |
| Hosted strict closure | root CI `37134452335`; Desktop E2E `37134452312` | pass | Both successful runs are on `738142d82a78e7cb345e9230fa6372b10e4edf35`. |

## 3. Production implementation evidence

The daemon turn-composition module owns a private error with a stable code and message. The `SessionPromptSubmit` request handler converts it to the protocol response at the boundary. Success still returns the same model, agent DTOs, and user-message tuple. The request-level selection-store regression injects a failed selection persistence pool while retaining the healthy daemon pool for request authorization.

The Desktop E2E workflow now triggers for the shared daemon prompt handler and projection replay/storage modules exercised by the desktop session/control-plane slice. No user-facing protocol or prompt semantics changed.

## 4. Verification executed

### Commands run locally

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test -p codegg --lib
cargo test -p codegg --test single_daemon_lifecycle --locked
scripts/verify.sh quick
cd apps/desktop && npm ci && npm run typecheck && npm test && npm run bindings:check
rustup run 1.90.0 cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml --locked --lib
```

The Rust workspace commands were run using the installed Rust 1.89 ARM toolchain. The initial unqualified Rust attempt selected an x86_64 Homebrew linker/toolchain and failed before compilation due to architecture mismatch; rerunning with the repository's Rust 1.89 ARM toolchain succeeded. Root library tests passed (5,087 passed, 6 ignored), the focused selection-store regression passed, lifecycle integration tests passed (8/8), desktop renderer tests passed (18/18), and Tauri host library tests passed (73 passed, 1 ignored). Formatting, Clippy, quick verification, typecheck, and binding checks passed.

### Hosted results

- `CI / verify` run `37134452335`: success on `738142d82a78e7cb345e9230fa6372b10e4edf35`.
- `Desktop E2E` run `37134452312`: success on the same SHA after rerunning its failed job. The initial attempt failed when the E2E immediately queried `message-list` after observing `permission-list`; the failed job passed in full on same-SHA retry. This is recorded as a timing-sensitive assertion; no product defect was reproduced.

## 5. Invariant review

- Protocol response construction remains at the request boundary; helper failures use a narrow private type.
- Stable public error codes/messages are preserved.
- Authorization, audit, request family, active-turn guard, and shared `TurnSubmit` delegation are unchanged.
- Prompt content, credentials, workspace secrets, and agent configuration contents are not logged by the error mapping.
- Desktop E2E remains path-gated and includes the exact shared dependencies established by the M004 trajectory.

## 6. Failure and recovery review

Empty and over-bound prompt input fail before selection/composition. Missing selection, unresolved legacy selection, selection persistence failure, and an unresolvable session fail closed with stable codes. Duplicate active submits remain rejected by the shared turn-start path. Successful requests preserve the existing submit path. The Desktop E2E first-attempt timing failure passed on same-SHA retry; no production failure was observed. Restart/reconnect ownership and recovery behavior remain covered by the M004 trajectory and prior M003 lifecycle closure.

## 7. Migration and compatibility review

No schema, storage layout, protocol DTO, or configuration migration was introduced. The request/response contract and existing error codes remain compatible. Rollback is a code revert; there is no durable state to migrate.

## 8. Security review

Authorization and audit remain ahead of prompt submission. Error mapping does not expose prompt bodies or credentials. Selection lookup error detail remains in the existing response message contract; tests verify the stable prefix/code. The text bound remains unchanged.

## 9. Documentation and operations

- Corrective addendum marked closed and linked to this evidence.
- Desktop foundation roadmap and registry now agree on strict M004 closure.
- Desktop E2E workflow path filters cover shared prompt, projection replay, and storage dependencies.
- No new operator procedure or static guard was needed.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| low | No natural fixture currently reaches `agents_invalid` after strongly typed agent resolution. | That defensive DTO-conversion branch is verified by code mapping, not an injected request-level failure. | Revisit if the DTO boundary becomes fallible through a realistic fixture or production behavior. |

No high- or medium-severity finding remains. The initial E2E timing assertion passed on same-SHA retry and is not an unresolved product finding.

## 11. Roadmap disposition

M004 is strict-closed. M005 is eligible for a fresh document/buffer/LSP ownership audit and planning; M002's Windows-specific evidence condition does not block this Linux/macOS audit. No M005 implementation plan is registered or ready. M006 remains deferred on M005. M003 remains closed; historical closure records are preserved.

## 12. Registry updates

- Mark C001 and the post-closure corrective addendum closed; remove C001 from active closure work.
- Mark foundation M004 strict-closed, with this corrective closure linked beside the immutable historical M004 closure.
- Remove M005 from `Blocked work`: its C001 prerequisite is satisfied. Record M005 as eligible for fresh audit/planning, without registering an implementation handoff as ready.
- Keep M006 deferred on M005. No other registered implementation plan names C001/M004 as a newly satisfied dependency.
