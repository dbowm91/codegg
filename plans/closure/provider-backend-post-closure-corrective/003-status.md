# Provider Backend Post-Closure Corrective C003 — Closure Status

Status: closed

Source implementation plan: `plans/implementation/provider-backend-post-closure-corrective/003-provider-resilience-ownership-and-public-api-disposition.md`

Source subsystem roadmap: `plans/subsystems/provider-backend-post-closure-corrective-addendum.md#c003--provider-resilience-ownership-and-public-api-disposition`

Repository baseline reviewed: `62653a85b09ae10efe5e3c379cc68c96a62050f6`

Implementation commits:

- `7963db4461dea738eb5d72ddcb10b7490f3ebf47` — feat: provider backend post-closure correctives C001-C003 implementation (contains C003 ownership docs, stale-comment correction, and production-construction guard)

## 1. Executive finding

C003 is complete with disposition **retain as library-only** (preferred outcome
1). Production provider-turn retry/failover has one truthful owner:
`src/agent/provider_turn.rs` (same selected provider/session, canonical
`ProviderError::retry_disposition` taxonomy, unified `RetryContext` budget).
`FallbackProvider` and provider-level `CircuitBreaker` are retained as clearly
labeled library/test compatibility primitives with a static guard proving no
production provider/session path constructs the dormant fallback owner.
No production retry behavior changed. No unresolved medium-or-higher finding
remains.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result |
|---|---|---|
| One truthful production retry/failover owner | `src/agent/provider_turn.rs` (retry loop, no provider switching) + `architecture/provider.md` unified-chain note + corrected module comment | Pass |
| No active doc claims Fallback participates in production | `architecture/provider.md` Fallback section now "library-only, NOT production"; `architecture/resilience.md` integration now "library-only (C003)"; `provider_turn.rs` stale comment corrected; `fallback.rs`/`circuit.rs` module docs state NOT production owner | Pass |
| Public fallback/circuit API has explicit retain/deprecate/remove disposition with compatibility evidence | §3 disposition table: retain both (see compatibility audit); no removal, no deprecation needed | Pass |
| Static guard prevents second production failover owner | `scripts/check_provider_resilience_ownership.py` (new, wired into quick) forbids `FallbackProvider::new` outside `fallback.rs`/`tests/`; passes | Pass |
| Production retry behavior/tests unchanged | `cargo test -p codegg-providers --lib` 196 passed (fallback/circuit suites green); `cargo test -p codegg-core --lib` 813 passed; no retry-budget/taxonomy diff | Pass |

## 3. Production implementation evidence

### Internal call-site audit (repository-wide, `*.rs`)

| Surface | Construction/reference | Disposition |
|---|---|---|
| `FallbackProvider::new` | only `crates/codegg-providers/src/fallback.rs` (definition + 4 unit tests) | retain; no production construction |
| `FallbackProvider` type/docs | `src/agent/provider_turn.rs:22` historical comment (now corrected); `architecture/provider.md`, `architecture/resilience.md` (now corrected to library-only) | docs fixed; no code consumer |
| `CircuitBreaker::new` | `fallback.rs` (per-provider breakers for library composition) + `circuit.rs` unit tests + `tests/reliability_qualification_m008.rs:430` (test-only qualification) | retain; test + library use only |
| `codegg-core::resilience` re-export | `crates/codegg-core/src/resilience.rs:6` re-exports `Circuit{Breaker,Error,State}`; no production `src/` or `codegg-core` consumer constructs it | retain re-export (harmless, test-visible) |
| Production registry/session/turn paths | `register_builtin*`, `build_durable_provider`, `ProviderConnectionFactory`, `src/core/eggpool.rs`, `src/agent/provider_turn.rs` — zero `FallbackProvider` constructions | guard-enforced |

### Package/publication/API compatibility audit

- `codegg-providers` / `codegg-core`: workspace-internal crates (`version = "=0.1.0"`,
  `path = ...` in root `Cargo.toml`); no `publish` to crates.io, no external
  downstream evidence in-repo. Only in-workspace consumers are the crate's own
  tests plus one integration test (`reliability_qualification_m008` uses
  `CircuitBreaker`, not `FallbackProvider`).
- `codegg-providers::fallback` (`FallbackProvider`): zero internal consumers
  outside its own tests; safe to remove technically, but retained per plan's
  "truthful ownership over code deletion" guidance to avoid any unknown
  downstream break.
- `codegg_providers::circuit` + root `CircuitBreaker` re-export +
  `codegg_core::resilience`: consumed by `fallback.rs` and the reliability
  qualification test; generic admission primitive with library purpose beyond
  providers (single-probe HalfOpen, timeout seeding). Retained.
- Decision: **retain, clearly labeled; no deprecation, no removal.** Removal
  would delete working unit tests and risk an unknown downstream break for no
  runtime benefit; deprecation would imply a replacement where none is needed
  (production failover is session/router-owned, intentionally without a
  drop-in `FallbackProvider` substitute).

### Code/docs changes (no behavior diff)

- `crates/codegg-providers/src/fallback.rs`: module + struct docs state
  library-only, point to `provider_turn.rs`/taxonomy/`RetryContext`, cite the
  guard.
- `crates/codegg-providers/src/circuit.rs`: module docs state generic admission
  only, not a turn owner.
- `src/agent/provider_turn.rs`: stale "fallback internals are owned by
  `FallbackProvider`" corrected to library-only + never constructed here (C003).
- `architecture/provider.md`: Fallback section + unified retry chain + circuit
  note state single production owner + guard.
- `architecture/resilience.md`: integration section states library-only +
  guard; stale `is_available()` claim corrected to atomic `try_admit()` +
  terminal-outcome accounting.
- `scripts/check_provider_resilience_ownership.py` (new, wired into quick).

## 4. Verification executed

```bash
cargo fmt --all -- --check
cargo test -p codegg-providers --lib
cargo test -p codegg-core --lib
cargo clippy -p codegg-providers --all-targets -- -D warnings
cargo clippy --workspace --all-targets -- -D warnings (via workspace check)
cargo check --workspace --all-targets --locked
./scripts/check-core-boundary.sh
python3 scripts/check_provider_resilience_ownership.py
cargo test --test provider_transcripts
```

- `cargo test -p codegg-providers --lib`: 196 passed (fallback 5 + circuit 2 +
  others; dormant-primitive tests pass as retained).
- `cargo test -p codegg-core --lib`: 813 passed.
- Clippy/checks/fmt/guards: pass.
- `provider_transcripts`: same environmental link block as C001/C002
  (unrelated `/opt/local` link); provider/core lib suites green.

Focused reliability suites (taxonomy, retry budget, visible-output
no-retry, uncertain-side-effect) are covered by the lib suites and the
existing `reliability_qualification_m008` harness (which consumes
`CircuitBreaker` as a library primitive, consistent with retain).

## 5. Invariant review

- Production turn retry stays in the agent/runtime chain: no diff in
  `provider_turn.rs` logic (comment-only change).
- Selected durable connection/session never silently replaced: no
  `FallbackProvider` in production (guard).
- Retry budgets/uncertain-side-effect behavior unchanged: no taxonomy/budget diff.
- No new routing authority: no new provider/account routing code.
- No nesting behind EggPool retries: no EggPool integration change.
- No public API break: retain (no removal/deprecation).
- Historical tests/closures unchanged: no closure rewrite.

## 6. Failure and recovery review

Unchanged by design. The corrective adds no admission/retry/circuit state to
the production turn path. `FallbackProvider` stream-health accounting and
`CircuitBreaker` state machine are untouched (docs-only + guard).

## 7. Migration and compatibility review

No deprecation, no removal, no migration. If a future plan ever authorizes
production fallback routing, it must register an ADR/roadmap per plan §14
(stop condition) and update the guard explicitly — silent adoption is now a
verification failure.

## 8. Security review

No auth, secret, or privilege change. Guard is a source-pattern check (no
secrets). Docs remove the misleading failover claim, reducing the risk of a
future double-retry/failover integration.

## 9. Documentation and operations

- `architecture/provider.md`, `architecture/resilience.md`,
  `src/agent/provider_turn.rs`, `fallback.rs`, `circuit.rs` docs (see §3).
- `scripts/check_provider_resilience_ownership.py` + `verify.sh quick` wiring.
- Closure record (this file).

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| low | `provider_transcripts` link block (same as C001/C002) | No C003 signal from that binary here | Run in CI/hosted Linux |
| low | `CircuitBreaker::is_available()` remains as deprecated compat (touched only in docs) | No production use; deprecation retained | None; remove only with a separate API disposition |

No medium-or-higher findings.

## 11. Roadmap disposition

C003 closed (retain). C001 and C002 closed on the same branch. The provider
backend post-closure corrective addendum (C001–C003) is fully closed; historical
wire-kernel M001–M003 remain closed and immutable. No new corrective required.

## 12. Registry updates

- `plans/registry.md`: move C003 row from ready to closed with this closure link;
  promote the corrective workstream to closed (C001–C003 all closed).
- `plans/implementation/.../003-...md`: Status → `closed` with closure link.
- Unblock audit: audited `plans/registry.md` blocked work (search/eggsearch
  M001–M002 blocked on eggsearch parity; tool-advisor M004/M005 blocked on
  model experiments; desktop M004 blocked on its own C001; causal M003–M005
  gated on its M002). None lists provider-backend C001–C003 as a hard/interface
  dependency. **Nothing unblocked by C003** (or by C001–C003 as a group);
  blocked rows left unchanged.
