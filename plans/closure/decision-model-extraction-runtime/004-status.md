# Decision-Model Extraction and Runtime Milestone 004 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/decision-model-extraction-runtime/004-system-one-backend.md`

Source subsystem roadmap:

- `plans/subsystems/decision-model-extraction-runtime-roadmap.md#m004--system-one-compatible-backend`

Repository baseline reviewed: `2d0410b`

Implementation commits:

- `2a0ea64` — add the opt-in config, System One backend, and profile/security bounds.
- `85210a5` — add redacted privacy diagnostics and close the connection-failure evidence gap.
- `7869bdc` — verify redirect refusal with a destination spy and reject invalid URL schemes before DNS.
- `5828a60` — cover wrong answer families and oversized responses in the fake-server matrix.

## 1. Executive finding

M004 is complete. CodeGG now has an explicitly selected System One `DecisionEngine` implementation with bounded Binary, Choice, and integer-range Score mappings. Rank and unsupported score ranges return `Unsupported`; endpoint, transport, timeout, protocol, and response validation failures return `Unavailable` for caller-owned deterministic fallback. The engine has no production agent-loop caller yet; migration and live policy integration remain M005 work.

The default configuration is off. Constructing a missing or disabled engine returns `NoopDecisionEngine` without credential resolution, endpoint DNS, or HTTP. No database migration or new dependency was added.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Explicit opt-in and zero default network attempts | `DecisionEngineConfig::default`; `engine_from_config`; `absent_and_default_config_build_noop_engine`; `rejects_unknown_answers_and_off_mode_never_connects` | pass | A local listener spy observed no connection in off mode. Disabled config returns Noop before auth or target resolution. |
| Binary mapping | fake-server `binary_and_score_round_trip_preserve_supported_values` | pass | Binary maps to `noul`; returned probability remains the CodeGG probability. Provider confidence is not projected. |
| Choice mapping and identity | fake-server `choice_round_trip_uses_exact_question_and_option_ids` | pass | Named options map to criteria; all returned probability names must exactly equal requested option IDs, and every value must be finite and in `[0,1]`. The selected ID must be requested. |
| Score mapping | fake-server `binary_and_score_round_trip_preserve_supported_values` | pass | Integral bounds spanning 1–9 intervals map to ordered rubric levels; upstream score is shifted by the configured minimum. Fractional bounds, reversed bounds, and wider spans return Unsupported. |
| Rank semantics preserved | `rank_is_unsupported_and_discovery_is_explicit` | pass | Rank never becomes exclusive Choice or repeated Choice calls. |
| Bounded failure fallback | `http_and_protocol_failures_become_unavailable`; `deadline_timeout_returns_bounded_unavailable` | pass | Covers connection refusal, 401, 403, 404, 422, 429, 500, 503, truncated/malformed JSON, unknown answer/question IDs, invalid probability, oversized body, redirect response, and timeout. No response error body is logged or returned as a reason. |
| Redirect and host policy | `profiles_are_explicit_and_endpoint_policy_is_bounded`; redirect-destination listener spy; Eggfetch builder config | pass | Reference requires HTTPS and rejects internal resolved addresses; Ollama permits only loopback HTTP without credentials. Redirect following is disabled and the redirect destination received no connection. Resolved addresses are pinned for Eggfetch. |
| Deadline includes endpoint resolution | `bounded_target`; deadline timeout behavior | pass | DNS resolution runs on a blocking task awaited only until the caller's Tokio deadline; HTTP uses the remaining caller budget capped by configured timeout. |
| Credential and state privacy | `AuthResolver` in `SystemOneConfig::from_schema`; external/OAuth modes rejected; `privacy_diagnostics_include_only_field_names_and_sizes` | pass | Uses CodeGG API-key/stored credential resolution. Credential Debug is already redacted by `codegg-providers`; this adapter adds no secret/state logging. Privacy diagnostics expose only validated field names and byte counts. |
| Response cannot widen IDs or semantics | exact one-question envelope, exact generated question ID, option-set validation, `DecisionResponse::validate_for` | pass | Unknown question IDs, answer families, options, and malformed distributions fail closed. The adapter has no tool registry, permission, or execution handle. |
| Explicit model discovery | `rank_is_unsupported_and_discovery_is_explicit` | pass | Only runs by direct operator call when `discover_models` is enabled. No background polling or model installation exists. |
| Configuration and docs | `DecisionEngineConfig`, simple-override merge, `architecture/config.md`, `architecture/tool-advisor.md`, `architecture/overview.md` | pass | Added config is optional, additive, and disabled by default. |

## 3. Supported profile/schema table

| Profile | Endpoint policy | Supported wire semantics | Model discovery |
|---|---|---|---|
| `reference` | HTTPS only; all resolved addresses must pass the existing internal-address policy. Credentials, when configured, use the existing AuthResolver and Bearer header. | `/v1/systemone`, one named question, shared bounded state, Binary=`noul`, Choice=`choice` with named criteria, integer-range Score=`score` with ordered levels. | Explicit `/v1/models` call only when configured and invoked. |
| `ollama` | HTTP only to loopback (`localhost`, loopback IP); credentials are rejected. CodeGG never starts Ollama. | Same bounded single-question common subset. Rank and unsupported score ranges are rejected locally. | Explicit call only when configured and invoked. |

The request uses a stable generated `codegg_decision_v1` question ID, not a model-visible label. State is serialized from validated M001 `StateField`s only. Request and decoded response bodies are capped at 64 KiB. The caller deadline bounds DNS resolution and the single Eggfetch request; redirects and retries are disabled for this backend.

The reference profile follows the [System One API reference](https://docs.system-one.dev/en/docs/api); Ollama's separately identified profile follows the [Ollama System One endpoint](https://docs.ollama.com/api/systemone). The implementation intentionally uses their documented common subset and does not send optional criteria extensions whose compatibility varies by deployment.

## 4. Fake-server result matrix

The local fake HTTP server is part of the `src/decision.rs` unit test target:

- Success: Binary, Choice, Score, model discovery.
- Semantic rejection: Rank, unknown option, unknown/extra answer ID, wrong answer family, invalid probabilities, oversized response behavior.
- Transport/protocol fallback: refused connection; 401/403/404/422/429/500/503; malformed/truncated JSON; timeout.
- Redirect protection: 302 response with a live local destination spy; zero follow-up connection.
- Off mode: ephemeral listener spy; zero connection.
- Privacy: a sentinel state value never appears in the diagnostic rendering; credential Debug uses the provider type's masked representation.

No hosted or local inference service credential was configured in this environment, so no live profile smoke was performed. This is recorded rather than inferred. The plan allows fake-server plus reference-schema evidence to close without available live credentials.

## 5. Verification executed

```bash
rtk cargo fmt --all
rtk cargo check --locked -p codegg --tests
rtk env RUSTFLAGS='-C link-arg=/Library/Developer/CommandLineTools/SDKs/MacOSX.sdk/usr/lib/libz.tbd -C link-arg=/usr/local/opt/libiconv/lib/libiconv.dylib -C link-arg=/usr/local/opt/xz/lib/liblzma.dylib' cargo test --locked -p codegg --lib decision -- --nocapture
rtk cargo test --locked -p codegg-config
rtk cargo clippy --workspace --all-targets --locked -- -D warnings
rtk scripts/verify.sh quick
rtk git diff --check
```

Results:

- Root decision filter: 32 passed, 0 failed (9 System One tests plus other decision-named tests).
- `codegg-config`: 88 passed across 2 suites.
- Root test binary linked with the same SDK/Homebrew overrides required earlier on this x86_64 target; the linker emitted warnings that unrelated arm64 MacPorts libraries were ignored.
- Workspace Clippy: passed with `-D warnings`.
- `scripts/verify.sh quick`: passed, including workspace all-target check and boundary/security guards.
- `git diff --check`: passed.

`--all-features` was deliberately omitted: repository guidance prohibits that workspace sweep because it enables installed real-server tests. No feature-specific test feature is needed for the System One backend.

## 6. Invariant, migration, and security review

- No config or missing/disabled config selects Noop without auth resolution or network access.
- A request is validated before endpoint resolution and transport. State values never appear in logs or diagnostics.
- API keys and stored credentials use `AuthResolver`; unsupported ExternalCommand and OAuth modes fail closed. The transport never follows a redirect and transmits only to the DNS-pinned configured target.
- The adapter has no tool authority. CodeGG's tool surface, candidate filtering, permission checks, fallback policy, causal-frontier behavior, and actuation remain host-owned and unchanged.
- No database, provider registry, training capture, Cargo dependency, or lockfile change.
- No live behavior is wired to this engine until M005 migration.

## 7. Unresolved findings

| Severity | Finding | Impact | Disposition |
|---|---|---|---|
| Informational | Live reference/Ollama smoke unavailable because no service credential or running Ollama instance was configured. | Provider-side operational compatibility was not exercised against a live service. | Allowed by M004 plan; fake-server and current reference-schema evidence are recorded above. |

## 8. Dependency audit and roadmap disposition

M004's only hard dependency (M001) was closed before activation. The M005 interface dependency is now satisfied by the generic DecisionEngine plus a second independent implementation; M005 remains blocked solely on positive M003 closure. M003 remains blocked on M002. M002 remains blocked on a maintainer-approved external repository and explicit destination license/release policy. M006 remains downstream of M005 and its required source/dependency inventory.

No other milestone in this plan line can complete while M002's external destination and release policy remain unresolved. M003 requires positive M002 evidence, M005 requires M003, and M006 requires M005. The subsystem stays active with M004 closed and M002/M003/M005/M006 accurately blocked.

## 9. Registry updates

- Mark M004 closed and link this closure from the subsystem roadmap and registry.
- Remove M004 from dependency-ready work; record it in recently closed work.
- Keep M002 blocked on external repository/license/release policy; keep M003 blocked on positive M002.
- Keep M005 blocked on M003 only; M004 is closed and its contract is stable.
- Keep M006 blocked on M005 and source/dependency inventory.
