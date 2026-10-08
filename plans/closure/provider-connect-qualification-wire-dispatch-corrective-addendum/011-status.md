# Provider Connect Qualification Corrective — Milestone 011 — Closure Status

Status: closed

Source implementation plan:
`plans/implementation/provider-connect-qualification-wire-dispatch-corrective-addendum/011-shared-provider-profile-and-opencode-multi-surface-dispatch.md`

Source subsystem roadmap:
`plans/subsystems/provider-connect-qualification-wire-dispatch-corrective-addendum.md#milestone-011--shared-provider-profile-consumption-and-opencode-go-multi-surface-dispatch`

Repository baseline reviewed: `ddfc6ce1311fdf82525e744a37c9a3a0cd13a5c8`

Implementation commits: see §3.

## 1. Executive finding

M011 is complete. OpenCode Go is no longer a one-surface OpenAI-compatible
wrapper: a requested model resolves through the shared EggPool provider-profile
contract to exactly one wire surface, and CodeGG builds the request for that
surface's own path, credential header, and grammar.

The milestone's real obstacle was not writing a multi-surface provider. It was
that the plan's own pinning instruction — "following the existing
`eggpool-wire`/`eggpool-model-routing` pinning precedent" — is **insufficient as
written**, and following it literally would have produced a workspace with two
`WireSurface` types while satisfying the letter of the instruction.
`eggpool-provider-profile` declares `eggpool-wire` as a *path* dependency, and
Cargo resolves a git dependency's path dependencies against the **parent's**
revision. CodeGG pinned `eggpool-wire` at `f05b18b`, so adding the profile at
`9ac6a131` alone links two copies of `eggpool-wire` 0.1.0. WP-A therefore
realigned CodeGG's `eggpool-wire` pin to the same revision. The plan header and
the registry now record this constraint, and
`scripts/check_provider_multi_surface_dispatch.py` enforces it.

The dependency was verified against the real crate before any CodeGG code was
written: the immutable revision, the surface assignments, the per-surface auth
shapes, and the single-vocabulary property were all confirmed empirically
rather than read off the closure record.

## 2. Requirement-to-evidence matrix

| Plan requirement | Evidence | Result |
|---|---|---|
| Pin `eggpool-provider-profile` at the EggPool M001 closure revision | `crates/codegg-providers/Cargo.toml` pins `9ac6a1318e8db3c034b5ab54987317752d5ffea6`; `Cargo.lock` shows both eggpool crates at that exact rev | pass |
| Exactly one `eggpool-wire` copy links | `Cargo.lock` contains a single `eggpool-wire` entry at rev `9ac6a131`; guard check 1 | pass |
| CodeGG adapter from shared profile IDs to setup/catalog presentation | `provider_profile.rs`: `SHARED_IDS` map (`opencode_go` → `opencode-go`), `shared_provider_id`, `adapted_provider_ids`; durable CodeGG spelling never rewritten (`adapted_provider_ids_are_explicit`) | pass |
| Remove or guard duplicated base URL facts | `OPENCODE_GO_BASE_URL` is now documented as a durable-connection mirror and pinned to the shared profile by `opencode_go_base_url_mirrors_the_shared_profile`; the runtime provider resolves from the profile | pass |
| Dedicated provider resolving model → surface → path/auth | `opencode_go.rs`: `resolve_route` → `encode_body` → `apply_auth` → `request_builder` | pass |
| No second wire-surface vocabulary | `profile_resolution_uses_the_single_shared_wire_vocabulary` assigns the profile's `WireSurface` into an `eggpool_wire::profile::WireSurface` binding | pass |
| Direct stateless Responses through `eggpool-wire` | `wire::encode_openai_responses` / `wire::openai_responses_stream`; 9 `responses_surface_tests` | pass |
| Not routed through the hosted/stateful Responses program subsystem | `responses_body_is_stateless_and_never_uses_the_hosted_program_path` asserts `store:false` and absence of `previous_response_id`/`conversation`/`background`; guard check 6 rejects `responses_api::` in `wire.rs` | pass |
| Anthropic Messages without reusing MiniMax/Anthropic provider identity | `wire::encode_anthropic_messages` is a shared codec helper; `anthropic.rs` now delegates to it; `qwen_messages_model_uses_the_messages_surface` and `messages_model_uses_messages_path_and_api_key_auth` prove OpenCode Go serves Messages without the Anthropic provider's base-URL/`anthropic-version` | pass |
| Model catalog qualification for wire-resolved models | `opencode_go::qualify_models` / `unresolved_models`; `models()` filters discovery; 3 `catalog_tests` | pass |
| Unknown models unresolved, never defaulted to Chat | `unknown_model_is_unresolved_and_never_defaults_to_chat`, `unknown_model_is_not_retried_against_a_neighbouring_family`, `unresolved_model_sends_no_request_and_never_defaults_to_chat`; guard checks 2 and 4 | pass |
| Per-surface auth correct and non-colliding | `per_surface_auth_is_profile_owned`; capture tests assert Messages sends `x-api-key` and no `authorization`, Chat/Responses send Bearer and no `x-api-key`; reserved-header check in `request_builder` | pass |
| Stable `x-opencode-session` on all three surfaces | Three capture tests each assert the header; `session_identity_is_stable_and_isolated_per_request` also asserts it never leaks into the body | pass |
| No speculative cross-surface retry | `stream()` resolves exactly once before I/O; `one_logical_request_uses_exactly_one_surface`; guard check 4 rejects a second resolve | pass |
| M010 credential semantics consume real inference outcomes | `ProviderError::from_http_status` preserves 401/403 → class `auth`; `unauthorized_response_reaches_the_typed_auth_classification`, `forbidden_response_also_reaches_auth_classification` | pass |
| Wrong/unresolved surface is local, not auth | `ProfileError::WireUnresolved`/`SurfaceUnavailable` → `ProviderError::ModelNotFound` (class `model_not_found`, permanent); asserted `!= "auth"` | pass |
| Wrong/missing session is a local zero-network failure | `missing_session_context_fails_locally_before_any_network_io` (capture base points at an unbound port, so any outbound attempt would surface as transport failure) | pass |
| Secrets absent from errors/logs | `secrets_are_absent_from_error_renderings`; `Credential`'s masking `Debug` is unchanged; the profile cannot express a credential value at all | pass |
| Existing non-OpenCode providers do not regress | Full `codegg-providers` suite 242 passed; `anthropic` 2, `openai_compatible` 11, `wire::` 21 all green after the codec refactor | pass |

## 3. Production implementation evidence

**New: `crates/codegg-providers/src/provider_profile.rs`** — the narrow adapter.
Pure resolution over the parsed embedded registry: no I/O, no environment reads,
no state. `SurfaceRoute` carries surface, absolute URL, auth *shape*, static
headers, and the advisory `fixed_hint`. `ProfileError` has four variants, two of
which are hard local failures (`WireUnresolved`, `SurfaceUnavailable`).

**New: `crates/codegg-providers/src/opencode_go.rs`** — the multi-surface
provider. Resolves once, encodes per surface, applies per-surface auth, attaches
the session header, sends one request, decodes with the matching kernel adapter.
Includes a `#[cfg(test)]`-only `with_capture_base` that redirects the origin
while leaving the **profile-owned path** intact, so capture tests observe exactly
the endpoint production would request.

**Changed: `crates/codegg-providers/src/wire.rs`** — added
`encode_openai_responses`, `openai_responses_stream`, and
`encode_anthropic_messages`. The latter is now shared: `anthropic.rs`'s
`try_build_body` delegates to it, removing the duplicated Messages encoding the
plan called out.

**Changed: `crates/codegg-providers/src/additional.rs`** — `create_opencode_go`
now returns `OpenCodeGoProvider`. Its three former tests constructed a throwaway
`OpenAiCompatibleProvider` and therefore tested nothing in production; they were
removed and replaced with capture tests against the real provider.

**Changed: `crates/codegg-providers/Cargo.toml`** — added
`eggpool-provider-profile` and realigned `eggpool-wire`, both at
`9ac6a1318e8db3c034b5ab54987317752d5ffea6`.

**Changed: `scripts/check_provider_wire_boundary.py`** — the pre-existing guard
rejected every `eggpool*` package except `eggpool-wire`. Adding the neutral
profile crate would have failed it. Rather than delete the boundary, the
allowlist became an explicit `PERMITTED` set of neutral contract crates, each
carrying its admitting evidence, and the check now *requires* both
`eggpool-wire` and `eggpool-provider-profile` to be present. Every other
`eggpool*` package is still rejected.

**New: `scripts/check_provider_multi_surface_dispatch.py`** — 6 checks, wired
into `verify.sh quick`, with a 6-case `--self-test`.

## 4. Verification executed

All commands ran locally in this workspace. Nothing below is quoted from CI.

| Command | Result |
|---|---|
| `cargo fmt --all --check` | clean |
| `CARGO_BUILD_JOBS=2 cargo clippy --workspace --all-targets -- -D warnings` | clean, no allowlist |
| `cargo test -p codegg-providers --lib` | **242 passed**, 0 failed |
| `cargo test -p codegg-providers --lib provider_profile` | 12 passed |
| `cargo test -p codegg-providers --lib opencode_go` | 20 passed (17 capture + 3 catalog) |
| `cargo test -p codegg-providers --lib setup_catalog` | 16 passed |
| `cargo test -p codegg-providers --lib qualification` | 9 passed |
| `cargo test -p codegg-providers --lib anthropic` | 2 passed |
| `cargo test -p codegg-providers --lib openai_compatible` | 11 passed |
| `cargo test -p codegg-providers --lib wire::` | 21 passed |
| `cargo test -p codegg --lib core::eggpool::tests` | 28 passed (M010 regression) |
| `cargo test -p codegg --lib provider_qualification` | 3 passed (M010 inference-classification regression) |
| `python3 scripts/check_provider_multi_surface_dispatch.py --verbose` | 6/6 |
| `python3 scripts/check_provider_multi_surface_dispatch.py --self-test` | 6/6 regressions caught, tree restored |
| `python3 scripts/check_provider_wire_boundary.py` | exit 0 |
| `bash scripts/verify.sh quick` | **exit 0** — "Quick verification passed" |
| `git diff --check` | clean |

## 5. Invariant review

| Plan invariant | How it holds |
|---|---|
| CodeGG runs direct providers with no EggPool daemon | No daemon, IPC, or runtime client introduced; the profile crate is sans-I/O and compile-time embedded |
| CodeGG retains credentials, secret refs, lifecycle, transport, cancellation, retry | Provider keeps `Credential`, builds its own request, owns status handling and deadlines; the profile carries no credential field |
| Shared profile data contains no secrets | The crate's own contract cannot express a credential value; CodeGG only ever projects shape |
| `eggpool-wire` remains grammar/stream authority | All three surfaces encode and decode through the kernel; the aligned pin keeps one copy |
| One surface per request, no speculative retry | `stream()` calls `self.resolve` exactly once before send; guard check 4 |
| Unknown models unresolved, not defaulted | Exact map lookup with an explicit `WireUnresolved`; no prefix matching exists |
| Per-surface auth owns its headers; static extras cannot collide | `request_builder` reserves credential/session/content-type and rejects collisions |
| Model selection/storage IDs stable | No ID renamed; `opencode_go` connection identity unchanged; no migration |

## 6. Failure, cancellation, restart, and contention

- **Unresolved model** — local failure before any socket is opened; permanent
  disposition, so it cannot enter a retry storm.
- **Wrong surface** — impossible to reach by construction: the URL comes from
  the profile entry for the resolved surface.
- **Upstream failure after send** — surfaced with its status, never re-routed to
  another surface.
- **401/403** — mapped to the typed `auth` class, feeding M010's credential axis.
- **Cancellation/deadline** — a single 30 s chunk timeout applies identically to
  all three surfaces, so behavior cannot drift between them.
- **Restart** — no new durable state. Profile metadata is embedded at compile
  time; connections and selections are existing CodeGG data.
- **Previously-selectable model stops resolving** — fails locally with an
  actionable diagnostic naming the model, instead of switching surfaces.

## 7. Migration and compatibility review

- **No database migration.** No model id, provider id, or connection record is
  rewritten.
- **Connection identity unchanged.** Existing `opencode_go` connections keep the
  same provider id and secret reference; only the runtime implementation behind
  that identity changed.
- **`eggpool-wire` pin moved** `f05b18b` → `9ac6a131`. The diff between those
  revisions is additive (`+881/-75` across `codecs.rs`, `decode.rs`, `lib.rs`,
  `stream.rs`); the only new public items are `terminal_evidence()` and
  `flush()`, and no public item was removed. `codegg-providers` compiles and its
  full suite passes unchanged against the new rev.
- **Durable base URL.** `SetupEndpointPolicy::Fixed` stores a `&'static str`, so
  `OPENCODE_GO_BASE_URL` is retained as a mirror of the shared profile and pinned
  to it by test. The shared profile is authoritative at runtime.

## 8. Security review

- **Credential ownership never moves.** Only the credential's *header shape*
  comes from the profile; the value is always CodeGG's, applied at request time.
- **No secret can enter profile data** — the contract has no field for one, and
  the crate rejects literal values on credential-named headers at validation.
- **No credential in errors.** 401/403 render status plus upstream body; the test
  asserts the credential never appears in the rendered error.
- **Reserved-header enforcement.** Profile-supplied static headers are checked
  against credential, session, and content-type before being attached, preserving
  the M008 rule.
- **Session binding.** The session header is read only from request context, so a
  caller cannot inject it and a missing context cannot silently send an
  unaffiliated request.

## 9. Documentation and operations

- `architecture/provider.md` — new "Shared Provider Profile and Multi-Surface
  Dispatch (M011)" section covering ownership, the pinning constraint, the
  adapter, per-surface auth, one-surface-per-request, direct Responses, and
  catalog qualification; the `openai_compatible` and `create_opencode_go`
  entries corrected.
- `docs/providers.md` — user-facing "OpenCode Go uses three endpoint families"
  table with the per-surface auth header and examples, plus the unresolved-model
  behavior.
- `.opencode/skills/provider-auth/SKILL.md` — backend table now names
  `opencode_go.rs` and `provider_profile.rs`; hard rules 9 and 10 added; both new
  guards listed under Static Guards.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| low | The M001 closure record's §10 low finding notes `eggpool-wire`'s own asset still carries 7 stale OpenCode Go `[[hints]]` rows (`minimax-m2.5` among them) that no runtime path reads. | Documentation-level drift inside the sibling crate. CodeGG resolves hints only from the canonical profile, so no dispatch decision is affected. | Owned by the EggPool repository; optional future reconciliation there. |
| low | `ProviderProfile::runtime_capabilities` is an opaque `toml` passthrough in the shared contract. | CodeGG does not read it, so no semantic confusion is currently possible. | Owned by the sibling contract; no action here. |
| low | Hosted CI has not been observed for these commits. | The local sweep is green, but hosted results are not claimed. | Observe the next hosted run; no PR was opened for this work. |

No critical, high, or medium findings remain.

## 11. Roadmap disposition

Milestone 011 is **closed**. With M010 already closed, this corrective roadmap
meets its completion definition: `/connect` no longer equates model enumeration
with key validity, and every selectable OpenCode Go model has an explicit
supported wire mapping using the correct endpoint/auth/codec path or is
explicitly unresolved rather than misrouted. No successor milestone is
registered for this roadmap; new provider-profile or dispatch work requires a
new bounded plan.

## 12. Registry updates

- `plans/registry.md` — the roadmap row moves to "M010 closed; M011 active"
  during implementation and "M010 closed; M011 closed" at closure; M011 is
  registered in the dependency-ready table with its implementing revision; the
  Blocked-work row for M011 is **removed** (blocker resolved); the
  execution-order gate records the discharge with dated evidence and the pinning
  constraint; the recently-closed table records M011.
- `plans/subsystems/provider-connect-qualification-wire-dispatch-corrective-addendum.md`
  — M011 row → closed with the closure record path.

## 13. Unblock audit

Searched `plans/registry.md` (dependency-ready, active, blocked, closing,
recently-closed tables) and the dependency graphs of active subsystem roadmaps
for work gated on M011.

- **Downstream CodeGG plans** — none. M011 is the terminal milestone of this
  roadmap; no registered plan lists it as a hard, interface, or operational
  dependency.
- **Roadmap successors** — none registered.
- **Sibling EggPool plans** — out of this repository's planning scope. The
  dependency direction is reversed (CodeGG consumes EggPool M001), and no EggPool
  milestone depends on M011.

This closure unblocked **no other registered plan**. The unblock that did occur
was inbound and is recorded here for traceability: M011's own EggPool M001
blocker was discharged on 2026-10-07 before implementation began, which is what
moved it from `blocked` to `active`.