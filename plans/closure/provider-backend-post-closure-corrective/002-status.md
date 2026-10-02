# Provider Backend Post-Closure Corrective C002 — Closure Status

Status: closed

Source implementation plan: `plans/implementation/provider-backend-post-closure-corrective/002-provider-catalog-and-compatible-discovery-reconciliation.md`

Source subsystem roadmap: `plans/subsystems/provider-backend-post-closure-corrective-addendum.md#c002--provider-catalog-and-compatible-discovery-authority-reconciliation`

Repository baseline reviewed: `62653a85b09ae10efe5e3c379cc68c96a62050f6`

Implementation commits:

- `7963db4461dea738eb5d72ddcb10b7490f3ebf47` — feat: provider backend post-closure correctives C001-C003 implementation (contains C002 catalog correction, generic/preset probe split, shared bounded discovery core, constructor convergence, docs, and static guard)

## 1. Executive finding

C002 is complete. CodeGG has one truthful pre-credential endpoint authority
(`setup_catalog.rs`), confirmed drift is corrected (OpenCode Go
`https://opencode.ai/go/v1` → `https://opencode.ai/zen/go/v1`), Together is
confirmed-retained at `https://api.together.xyz/v1`, generic compatible
validation no longer inherits the Eggpool `:11300` preset, and strict
provisioning vs best-effort runtime discovery share one bounded
`/models` fetch/parser core while preserving distinct failure policies.
No new retry/catalog authority, no provider-ID/storage migration, no live
credentials in tests. No unresolved medium-or-higher finding remains.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result |
|---|---|---|
| OpenCode Go uses current first-party `/zen/go/v1` prefix | `OPENCODE_GO_BASE_URL` corrected; `opencode_go_uses_current_first_party_zen_prefix` + `additional::tests::opencode_go_uses_current_zen_prefix`; first-party `https://opencode.ai/docs/go/` (reviewed 2026-10-02) serves chat/responses/messages/models under that prefix | Pass |
| Together remains on documented `.xyz` | `together_endpoint_remains_on_documented_xyz_prefix`; `.xyz` retained (legacy documented) while `.ai` is the current canonical alias serving the same `/v1` API — no silent repoint | Pass |
| Every built-in endpoint fact has a first-party-source disposition | §3 matrix below (OpenCode Go corrected, Together retained, all other fixed endpoints retained with no contradictory evidence; no other drift corrected per "correct confirmed drift only") | Pass |
| Generic endpoints never inherit `:11300` | `generic_compatible_preserves_port_path_and_never_injects_default_port` + `compatible_probe_uses_generic_semantics_while_eggpool_keeps_preset`; provisioning uses generic `CompatibleProbe` | Pass |
| Strict vs best-effort share one bounded core, policies distinct | `parse_compatible_models_response()` shared by `EggpoolProbe`/`CompatibleProbe` (strict, fail-closed) and `OpenAiCompatibleProvider::models()` (best-effort, fallback to seeds); `shared_parser_enforces_bounds_with_deterministic_dedup` + `best_effort_models_merges_live_ids_and_keeps_seeds_on_failure` | Pass |
| No new retry/catalog authority | No new scheduler/router/retry code; `src/core/eggpool.rs` probe change is normalization-only (Eggpool→Compatible), timeouts/cancellation unchanged | Pass |
| Constructors use catalog authority | `Anthropic`/`Zen`/`MiniMax`/`OpenRouter` route through catalog constants; `every_fixed_definition_has_a_non_empty_http_prefix`; `fixed_endpoints_cannot_be_silently_repointed` | Pass |
| Cross-repo comparison (non-authoritative) | §3 EggPool comparison: CodeGG correct for Together, EggPool stale; EggPool correct for OpenCode Go, CodeGG was stale (now fixed). Neither repo is authority for the other. | Pass |

## 3. Production implementation evidence

### Provider endpoint/source matrix (bounded; first-party review 2026-10-02)

| Provider ID | Policy | Canonical default/fixed prefix | Wire family | Models behavior | Auth | First-party source + date | CodeGG disposition | EggPool comparison |
|---|---|---|---|---|---|---|---|---|
| anthropic | OptionalOverride | `https://api.anthropic.com` | Anthropic Messages (shared codec) | DirectModels | ApiKeyOnly | retained (no contradictory evidence; stable long-lived host) | retained; constructor now uses catalog constant | n/a (not shared) |
| openai | OptionalOverride | `https://api.openai.com/v1` | OpenAI Chat (shared codec, C001 composer) | DirectModels | ApiKeyOnly | retained | retained; consistency test pins catalog==native default | n/a |
| google | Fixed | `https://generativelanguage.googleapis.com` | Gemini (shared codec) | DirectModels | ApiKeyOnly | retained | retained | n/a |
| openrouter | Fixed | `https://openrouter.ai/api/v1` | OpenAI Chat (shared codec) | DirectModels | ApiKeyOnly | retained | retained; stream URL now derived from catalog constant | n/a |
| opencode_zen | Fixed | `https://opencode.ai/zen/v1` | OpenAI Chat (shared codec) | DirectModels | ApiKeyOnly | `https://opencode.ai/docs/zen/` 2026-10-02 (`/zen/v1/chat`, `/models` under prefix) | retained; constructor now uses catalog constant | n/a |
| mistral/groq/deepinfra/cerebras/cohere/perplexity/xai/venice/generalcompute | Fixed | per-catalog `*_BASE_URL` | OpenAI-compatible | DirectModels | ApiKeyOrBearer | retained (no contradictory evidence at implementation time; static seeds, live discovery preferred) | retained; factories already used constants | n/a |
| together | Fixed | `https://api.together.xyz/v1` | OpenAI-compatible | DirectModels | ApiKeyOrBearer | `https://docs.together.ai/docs/inference/openai-compatibility` 2026-10-02 (canonical `https://api.together.ai/v1`; legacy `https://api.together.xyz/v1` still documented) | retained at `.xyz` (no silent repoint; same `/v1` API) | EggPool stale (inverse drift); CodeGG correct |
| minimax | Fixed | `https://api.minimax.io/anthropic` | Anthropic-compatible | DirectModels | ApiKeyOnly | retained | retained; `create_minimax` now uses `MINIMAX_BASE_URL` | n/a |
| opencode_go | Fixed | `https://opencode.ai/zen/go/v1` | OpenAI-compatible + `x-opencode-session` affinity | DirectModels | ApiKeyOrBearer | `https://opencode.ai/docs/go/` 2026-10-02 (`/zen/go/v1/chat|responses|messages`, `/zen/go/v1/models`) | **corrected** from `/go/v1` | EggPool correct (CodeGG was stale; now converged, not copied) |
| eggpool | ProxyPreset `:11300` | preset host + port + TLS | OpenAI-compatible | CompatibleProbe (strict, generic validation) | ApiKeyOrBearer | CodeGG-owned preset (no external source) | retained; probe now generic (no re-injection) | EggPool daemon/runtime is optional; no dependency |
| custom | RequiredEndpoint | caller-supplied | OpenAI-compatible | CompatibleProbe (strict, generic) | ApiKeyOrBearer | caller-owned | retained; generic validation preserves port/path | n/a |
| azure | RequiredEndpoint | caller deployment endpoint | Azure OpenAI | DirectModels | ApiKeyOnly | caller-owned | retained | n/a |

Correct-confirmed-drift-only: only OpenCode Go changed. All other fixed
endpoints retained.

### Generic vs preset normalization

- `normalize_compatible_base_url()` (new): explicit `http(s)`, preserved
  port/path, never injects `:11300`, rejects userinfo/query/fragment/control/
  traversal/port-0/host-only.
- `normalize_eggpool_base_url()` (retained preset): host shorthand + default
  `:11300` + TLS policy.
- `CompatibleProbe` (new, generic) backs `CompatibleModelsProbe`; `EggpoolProbe`
  remains the preset wrapper. Both share `parse_summary` bounds/digest.
- `src/core/eggpool.rs::probe_with_options` now constructs
  `CompatibleProbe` so custom upstreams without a port never gain `:11300`
  (preset host/port handling already happened in `NormalizedSpec`).

### Shared bounded discovery core

- `parse_compatible_models_response()` (new, public): 1 MiB / 256-count /
  256-char bounds, `{"data":[{"id","name"?}]}` normalization, deterministic
  duplicate-ID handling (lexicographically smallest name wins).
- Strict callers (`EggpoolProbe`, `CompatibleProbe`): fail closed with redacted
  reason codes (existing tests + new shared-parser tests).
- Best-effort caller (`OpenAiCompatibleProvider::models()`): same byte/count/
  string bounds via `max_decoded_body_size` + shared parser; any failure
  returns configured seeds unchanged (new `best_effort_...` tests). Redirects
  stay policy-distinct (strict disallows; ordinary client follows) without a
  second parser. No new retry loop, cache, or background task.

### Static fallback review

Static model lists are conservative fallback seeds, not exhaustive availability
claims. Live discovery is preferred where supported. No static ID was
removed/updated in C002 because first-party evidence did not prove any entry
invalid at implementation time; stale-but-valid IDs are documented, not
churned (see `architecture/provider.md` provisioning note).

## 4. Verification executed

```bash
cargo fmt --all -- --check
cargo test -p codegg-providers --lib
cargo clippy -p codegg-providers --all-targets -- -D warnings
cargo check --workspace --all-targets --locked
./scripts/check-core-boundary.sh
python3 scripts/check_provider_wire_cutover.py
python3 scripts/check_provider_catalog_consistency.py
cargo test --test provider_transcripts
```

- `cargo test -p codegg-providers --lib`: 196 passed (includes new
  `eggpool::tests` generic/preset/parser, `openai_compatible::tests`
  best-effort, `setup_catalog::tests` Go/Together/fixed-override,
  `additional::tests` Go exact chat/models paths).
- Clippy/checks/fmt: pass (see C001 for workspace check).
- `check_provider_catalog_consistency.py`: pass (new, wired into quick).
- `provider_transcripts`: same environmental link block as C001 (unrelated
  `/opt/local` arm64 vs x86_64 `libgit2` link); provider-lib suites green.

OpenCode Go exact-path evidence: `opencode_go_chat_uses_exact_path_with_session_affinity`
(`POST /zen/go/v1/chat/completions` + `x-opencode-session: S1`) and
`opencode_go_models_uses_exact_models_path` (`GET /zen/go/v1/models`).

## 5. Invariant review

- `setup_catalog.rs` remains the pre-credential authority: capability-coverage
  test + new endpoint-policy tests.
- Fixed endpoints cannot be silently repointed: `fixed_endpoints_cannot_be_silently_repointed`
  (Fixed ignores caller URL; `src/core/eggpool.rs` still rejects endpoint/port/TLS
  overrides for Fixed).
- Explicit user endpoints supported for optional/required policies: unchanged
  `endpoint_from_user_url`/`normalize_endpoint` paths.
- Eggpool shorthand/default port only for preset: generic tests prove no `:11300`
  injection; preset test proves shorthand still maps to `:11300`.
- Provisioning strict/fail-closed; runtime best-effort/non-destructive:
  strict tests + best-effort fallback tests.
- Discovery bounded (bytes/count/string): shared parser + byte-limit tests.
- No secrets in logs: redacted reason codes; tests assert no key leakage.
- IDs/storage stable: no ID/schema change.
- No EggPool runtime dependency: `eggpool-wire` pin unchanged; EggPool
  daemon remains optional.
- No live provider access in CI: all new tests use local capture/fake servers.

## 6. Failure and recovery review

Provisioning remains operation-cancellable and bounded by workflow timeout;
runtime discovery uses the existing non-streaming timeout. No new background
task, retry loop, or global cache. Discovery failure never erases a usable
catalog (strict fails the operation; best-effort retains seeds). Cancellation
during body collection and overall timeout map to stable redacted reasons
(existing eggpool tests).

## 7. Migration and compatibility review

No database migration, no provider-ID change. Changing the fixed OpenCode Go
default affects future direct use of that definition only; explicit durable
custom endpoints follow current endpoint-policy rules (unchanged). Stored
fixed descriptors cannot silently repoint (Fixed wins at construction); no
persisted-data rewrite performed. Together `.xyz` retained to avoid breaking
stored fixed endpoints; `.ai` alias documented but not cut over.

## 8. Security review

Userinfo/query/fragment/control/traversal rejected in both normalizers;
port-0 rejected; auth header failures map to redacted `auth`; oversized/
invalid JSON map to redacted `oversized`/`invalid_json` (strict) or seeds
(best-effort) without response-body leakage. No credentials in diagnostics.

## 9. Documentation and operations

- `architecture/provider.md` provisioning/endpoint/discovery sections updated
  (authority, split, shared core, Go/Together dispositions, seeds).
- `scripts/check_provider_catalog_consistency.py` + `verify.sh quick` wiring.
- Closure record (this file).

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| low | `provider_transcripts` link block (same as C001) | No C002 signal from that binary here | Run in CI/hosted Linux |
| low | Non-Go/Together built-in endpoints not re-audited beyond "no contradictory evidence" | Retained values could drift later | Future corrective only on fresh first-party contradiction; no automatic freshness checks authorized |

No medium-or-higher findings.

## 11. Roadmap disposition

C002 closed. C001 and C003 closed on the same branch (`7963db44`). The
corrective addendum closes fully once all three closures land. No downstream
plan was gated on C002 alone.

## 12. Registry updates

- `plans/registry.md`: move C002 row from ready to closed with this closure link.
- `plans/implementation/.../002-...md`: Status → `closed` with closure link.
- Unblock audit: no registered blocked plan lists C002 as a hard/interface
  dependency (EggPool metadata M001 is an operational sibling, explicitly not a
  hard dependency per plan §16); nothing unblocked.
