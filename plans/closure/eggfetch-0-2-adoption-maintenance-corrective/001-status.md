# Eggfetch 0.2 Adoption Maintenance Corrective C001 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/eggfetch-0-2-adoption-maintenance-corrective/001-provider-policy-api-and-redirect-fixture-hardening.md`

Source subsystem roadmap:

- `plans/subsystems/eggfetch-0-2-adoption-maintenance-corrective-addendum.md#C001 — Provider policy API and redirect-fixture hardening`

Repository baseline reviewed: `89ed753b` (plan authored against `926a6e5bac8e3b2968679ec9e1fbf6242798ebde`; only planning-registration commits intervened)

Implementation commits or pull requests:

- `7c505d1f` — eggfetch maintenance corrective C001: narrow provider timeout policy, harden redirect fixtures

## 1. Executive finding

C001 is complete. The two low-severity post-M001 maintenance findings are closed without reopening the M001 correctness boundary: the provider non-streaming timeout policy is now crate-internal (`pub(crate)`, no public re-export) and both redirect test fixtures tolerate transient nonblocking `WouldBlock`/interrupted readiness within an explicit local deadline. The pass is production-behavior neutral: no timeout value, redirect policy, retry, TLS, Eggfetch version, or dependency change. All acceptance criteria in §9 of the implementation plan are satisfied with deterministic regression proof plus repeated focused runs and canonical verification green.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Narrow `NON_STREAMING_PROVIDER_TOTAL` to intra-crate visibility | `crates/codegg-providers/src/provider_core.rs`: `pub(crate) const`; `cargo check -p codegg-providers --locked` pass | pass | Value unchanged at 60s. |
| Narrow `non_streaming_timeout()` to intra-crate visibility | `provider_core.rs`: `pub(crate) fn`; intra-crate callers unchanged (`openai_compatible.rs:665`, `opencode_zen.rs:315` via `crate::provider_core::`) | pass | Helper retained as single internal policy point, not inlined. |
| Remove both symbols from public `codegg-providers` facade | `crates/codegg-providers/src/lib.rs` re-export drops both names; post-edit `rg` shows no `lib.rs` match | pass | `create_http_client()` visibility untouched per plan. |
| No external in-repo consumer broken | Pre-edit census: definition + 2 intra-crate consumers + crate-root re-export + tests + docs/history only; post-edit `rg` in `src crates tests`: only intra-crate definition/uses remain | pass | Stop condition (external consumer) never triggered. |
| Both redirect fixtures tolerate transient `WouldBlock`/interrupted readiness | Hardened `read_http_request` in `src/http_client.rs` and `provider_core.rs`: loop until `\r\n\r\n`/EOF/deadline; `WouldBlock` → 1ms sleep + retry; `Interrupted` → retry; terminal error → diagnostic panic | pass | Production socket/transport code untouched; explicit `set_nonblocking(true)` on accepted stream makes the path deterministic cross-platform. |
| Reader remains bounded, never a hang | 2s local fixture deadline with elapsed-time panic; outer server lifetime still 2s | pass | No unbounded busy-spin (1ms sleep on `WouldBlock`). |
| Deterministic induced-`WouldBlock` regression | New `read_http_request_tolerates_transient_wouldblock` in both modules: probe proves `WouldBlock` pre-write (old `expect` would panic), client withholds 50ms, hardened reader consumes headers | pass | Primary proof; repetition is supplemental. Provider 20/20, root 10/10. |
| Existing redirect-follow + ten-hop assertions unchanged and passing | `ordinary_builder_follows_redirects_and_enforces_ten_hop_bound` 20/20; `shared_client_follows_redirects_and_enforces_ten_hop_bound` 20/20 | pass | Response bodies/`TooManyRedirects{max:10}` assertions byte-identical. |
| No production HTTP/dependency change | `git diff --stat`: 3 source files only (`lib.rs`, `provider_core.rs`, `http_client.rs`); no `Cargo.toml`/`Cargo.lock`, no Eggfetch bump, no duration/policy edit | pass | M001 closure records untouched. |
| Canonical verification | `cargo fmt --all -- --check` clean; `cargo clippy --workspace --all-targets --locked -- -D warnings` clean; `scripts/verify.sh quick` pass | pass | See §4. |

## 3. Production implementation evidence

Production delta is API hygiene only:

- `crates/codegg-providers/src/provider_core.rs`: `NON_STREAMING_PROVIDER_TOTAL` `pub` → `pub(crate)`; `non_streaming_timeout()` `pub` → `pub(crate)`; rustdoc reworded to state crate-internal policy (no longer a `[link]` presented as external API). Timeout construction (`total: Some(60s)`, all other phases `None`) byte-identical.
- `crates/codegg-providers/src/lib.rs`: public re-export list drops `non_streaming_timeout` and `NON_STREAMING_PROVIDER_TOTAL` (rustfmt reflow only otherwise).
- Intended call sites still compile and behave identically: `openai_compatible::models()` and `opencode_zen::discover_models()` keep per-request `.timeout(crate::provider_core::non_streaming_timeout())`; `non_streaming_timeout_carries_sixty_second_total` test still asserts 60s total-only.
- Test-only delta: both `read_http_request` fixtures replaced (bounded `WouldBlock`/`Interrupted` handling + 2s diagnostic deadline); both `redirect_server` loops now force `set_nonblocking(true)` on the accepted stream; two new deterministic regression tests (one per owning module, identical logic, no shared production helper per plan non-goal).
- Expressly absent per non-goals: no 60s value change, no Responses/stream-setup/idle change, no redirect-policy change, no downgrade hardening, no client consolidation, no shared production HTTP utility, no dependency upgrade, no broad test refactor.

## 4. Verification executed

### Commands run

```bash
rg -n "NON_STREAMING_PROVIDER_TOTAL|non_streaming_timeout" .
rg -n "fn read_http_request|fn redirect_server" src crates tests
cargo check -p codegg-providers --locked
cargo test -p codegg-providers provider_core::tests::read_http_request_tolerates_transient_wouldblock --locked -- --exact --test-threads=1
cargo test --lib http_client::tests::read_http_request_tolerates_transient_wouldblock --locked -- --exact --test-threads=1
for i in $(seq 1 20); do cargo test -p codegg-providers provider_core::tests::shared_client_follows_redirects_and_enforces_ten_hop_bound --locked -- --exact --test-threads=1 || exit 1; done
for i in $(seq 1 20); do cargo test --lib http_client::tests::ordinary_builder_follows_redirects_and_enforces_ten_hop_bound --locked -- --exact --test-threads=1 || exit 1; done
for i in $(seq 1 20); do cargo test -p codegg-providers provider_core::tests::read_http_request_tolerates_transient_wouldblock --locked -- --exact --test-threads=1 || exit 1; done
for i in $(seq 1 10); do cargo test --lib http_client::tests::read_http_request_tolerates_transient_wouldblock --locked -- --exact --test-threads=1 || exit 1; done
cargo test -p codegg-providers --locked -- --test-threads=1
cargo test --lib http_client --locked -- --test-threads=1
rg -n "NON_STREAMING_PROVIDER_TOTAL|non_streaming_timeout" src crates tests
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
scripts/verify.sh quick
git status --short
```

### Results

- Pre-edit census: definition (`provider_core.rs:34,41`), intra-crate consumers (`openai_compatible.rs:665`, `opencode_zen.rs:315`, provider-core tests), crate-root re-export (`lib.rs:57,62`), no production consumer outside `codegg-providers`, docs/history only (`architecture/provider.md:506`, M001 closure records, plan/addendum). Fixtures: `read_http_request` + `redirect_server` in exactly the two planned files.
- `cargo check -p codegg-providers --locked`: pass.
- New provider regression: pass (single + 20/20 repetition).
- New root regression: pass (single + 10/10 repetition). One pre-existing macOS linker note (`__eh_frame section too large`, `#[warn(linker_messages)]`) observed on the root lib-test link; unrelated to this change, no warning from edited code.
- Provider redirect follow + ten-hop bound: 20/20 pass (~2s each).
- Root redirect follow + ten-hop bound: 20/20 pass (~2s each).
- `cargo test -p codegg-providers --locked`: 164 passed, 0 failed (163 pre-existing + 1 new).
- `cargo test --lib http_client --locked`: 4 passed, 0 failed (3 `http_client::tests` + 1 `lsp_security` name-substring match), 0 failed.
- Post-edit census (`src crates tests`): only crate-internal definition/uses remain; `lib.rs` re-export gone; no source outside `codegg-providers` imports either symbol.
- `cargo fmt --all -- --check`: clean (edits formatted via `cargo fmt --all` before check).
- `cargo clippy --workspace --all-targets --locked -- -D warnings`: clean.
- `scripts/verify.sh quick`: pass (agent schema, core-boundary, sandbox, execution-ownership, tui-authority, http-route-disposition, audit-coverage, scheduler-bypass, workspace check).
- `git status --short` / `git diff --stat`: only the 3 intended source files (+ plan status line); no `Cargo.toml`/`Cargo.lock` change.

## 5. Invariant review

| Source-plan invariant (§3) | Evidence it remains true |
|---|---|
| `eggfetch-core 0.2.0` unchanged | No manifest/lockfile delta; `cargo check` resolves the same tree. |
| Provider streaming keeps no client-global absolute total | `create_http_client()` untouched; `absolute_total_deadline_terminates_healthy_stream` still passes within the full 164-test provider run. |
| Bounded non-streaming ops keep current finite values | `non_streaming_timeout_carries_sixty_second_total` passes; 60s literal untouched. |
| Responses API timeout semantics unchanged | No `responses_api` edit; provider suite green. |
| Redirect behavior follow=true, max 10 hops (root + provider) | Both ten-hop overflow tests assert `TooManyRedirects{max:10}` and pass 20/20 each. |
| No production socket/transport change | All fixture edits are inside `#[cfg(test)]` modules; `git diff` confirms. |
| No retry/cancellation/SSE/TLS/trust-root/body-limit/SSRF change | No such files touched; provider suite green. |
| No new dependency | No manifest/lockfile delta. |
| Public API narrowed only for proven implementation-only symbols | Pre/post census proves no external in-repo consumer; sibling intra-crate call sites compile. |
| M001 plans/closure records immutable | No edit under `plans/closure/eggfetch-0-2-adoption/`; this record owns only new evidence. |

`architecture/provider.md:506` still names `non_streaming_timeout()` when explaining per-request timeout ownership. Per implementation-plan §7 this is permitted (internal helper named in ownership explanation, not presented as supported external API); no doc edit made.

## 6. Failure and recovery review

- Provider policy visibility: intentional API-surface contraction. Failure mode (external consumer relying on removed re-export) was gated by the pre-edit census stop condition; census showed none, and `cargo check` + full provider tests confirm no breakage. No replacement wrapper was created per plan.
- Redirect fixture: retryable readiness (`WouldBlock`, `Interrupted`) is now a wait-within-deadline, not a failure. Terminal read errors and 2s deadline expiry still panic with context (`request read failed: ...` / `timed out waiting for HTTP headers after ...`), so the fixture cannot silently ignore real failures or hang (1ms backoff, no busy-spin, outer 2s server lifetime preserved).
- Old-behavior failure proof: each new regression probes a raw nonblocking read with no bytes pending and asserts `WouldBlock`/`Interrupted`; the old unconditional `expect("read HTTP request")` would panic at exactly that probe. The hardened reader then consumes the delayed headers without panic.
- No daemon, scheduler, persistence, lease, or cancellation surface is touched; no recovery semantics apply beyond the test-thread join assertions, all of which pass.

## 7. Migration and compatibility review

Rust API-surface contraction for two symbols proven internal: any out-of-tree external caller importing `codegg_providers::{NON_STREAMING_PROVIDER_TOTAL, non_streaming_timeout}` would fail to compile after this change. Within this repository there is no such caller (census + workspace check + Clippy all-targets green). In-tree behavior is identical: same 60s total-only timeout applied at the same two call sites. No schema, storage, protocol, config, or rollback surface exists for this change. No migration required.

## 8. Security review

No authorization, secret, path, privilege, or audit surface touched. Redirect policy (follow=true, max 10, no downgrade-policy change) is byte-identical; the fixture's explicit `set_nonblocking(true)` and bounded retry affect only test threads and cannot widen any denial-of-service bound (2s local deadline, 1ms sleep, EOF/header-terminator exits preserved). No secret material in test traffic (loopback header fixtures only).

## 9. Documentation and operations

- No user-facing documentation change (none expected by the plan).
- `architecture/provider.md` reviewed; no edit required (see §5).
- Static-guard/verification footprint: existing `verify.sh quick` gates all green; no new guard script required for a two-symbol visibility narrowing plus test-local hardening.
- Operator diagnostics: none affected.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| — | None. | — | — |

No medium-or-higher finding remains. The two plan-owned low-severity findings (accidental public timeout-policy surface; `WouldBlock` fixture race) are both closed above. No new finding was discovered; the macOS linker `__eh_frame` note is pre-existing toolchain noise, not a C001 finding.

## 11. Roadmap disposition

Milestone C001 closed. The `eggfetch-0-2-adoption-maintenance-corrective` workstream completes with this closure: no successor corrective is required and none is registered. M001 remains closed with immutable history. Dependency audit (see §12): no registered `blocked` plan lists C001 as a hard or interface dependency, so no downstream plan is unblocked by this closure.

## 12. Registry updates

- `plans/registry.md`: subsystem row `Eggfetch 0.2 adoption maintenance corrective` active → closed (C001 closed); dependency-ready plan row C001 ready → closed with closure link `plans/closure/eggfetch-0-2-adoption-maintenance-corrective/001-status.md` and implementation `7c505d1f`; control-points row C001 ready → closed; execution-order item 11 reworded to closed; recently-closed table gains the C001 row. Blocked-work section unchanged (audit: dependency-security M005, architecture M009, runtime-safety C002, and advisor M004 variants are all blocked on unrelated prerequisites; none names C001).
- `plans/subsystems/eggfetch-0-2-adoption-maintenance-corrective-addendum.md`: header `active; C001 ready for handoff` → `closed; C001 closed`; C001 status table `ready` → `closed` with closure-record link; implementation commit recorded.
- `plans/implementation/eggfetch-0-2-adoption-maintenance-corrective/001-provider-policy-api-and-redirect-fixture-hardening.md`: `ready for handoff` → `implemented` (in implementation commit `7c505d1f`).
