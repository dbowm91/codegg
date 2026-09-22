# Eggfetch 0.2 Adoption Maintenance Corrective C001 — Provider Policy API and Redirect-Fixture Hardening

Status: ready for handoff

Repository baseline reviewed: `926a6e5bac8e3b2968679ec9e1fbf6242798ebde`

Source corrective addendum:

- `plans/subsystems/eggfetch-0-2-adoption-maintenance-corrective-addendum.md`

Predecessor closure:

- `plans/closure/eggfetch-0-2-adoption/001-status.md`

Primary class: polish.

## 1. Objective

Close the two low-severity maintenance findings left after Eggfetch 0.2 adoption without reopening the closed M001 correctness boundary:

1. narrow `NON_STREAMING_PROVIDER_TOTAL` and `non_streaming_timeout()` to crate-internal provider policy instead of exporting them from `codegg-providers`;
2. harden the root/provider redirect test fixtures so transient nonblocking `WouldBlock` cannot cause a spurious server-thread panic and client `IncompleteMessage`.

The pass must be production-behavior neutral. It changes API hygiene inside the workspace and deterministic test mechanics only.

## 2. Current evidence

### 2.1 Accidental provider API surface

`crates/codegg-providers/src/provider_core.rs` currently declares:

```rust
pub const NON_STREAMING_PROVIDER_TOTAL: Duration = Duration::from_secs(60);

pub fn non_streaming_timeout() -> eggfetch_core::Timeout {
    ...
}
```

`crates/codegg-providers/src/lib.rs` publicly re-exports both symbols.

Known current production consumers are intra-crate:

- `openai_compatible::models()`;
- `opencode_zen::discover_models()`;
- provider-core tests.

The timeout value is an internal construction policy. External callers should not be encouraged to couple to it as part of the provider crate's supported API.

### 2.2 Redirect-fixture race

Both:

- `src/http_client.rs`;
- `crates/codegg-providers/src/provider_core.rs`

contain near-identical test helpers:

```rust
fn read_http_request(stream: &mut TcpStream) {
    ...
    let count = stream.read(&mut chunk).expect("read HTTP request");
    ...
}
```

Their redirect server sets its `TcpListener` nonblocking and loops on `accept()`. M001 closure recorded a transient `WouldBlock` from the accepted request-read path; the client then observed an incomplete response. An immediate rerun passed unchanged.

The defect is in test determinism, not Eggfetch redirect behavior.

## 3. Required invariants

- `eggfetch-core 0.2.0` remains unchanged.
- Provider streaming keeps no client-global absolute total timeout.
- Bounded non-streaming provider operations keep their current finite timeout values.
- Responses API timeout semantics remain unchanged.
- Root ordinary-client and provider-client redirect behavior remains follow=true, max 10 hops.
- No production network socket mode or transport code changes.
- No provider retry, cancellation, SSE, TLS, trust-root, body-limit or SSRF behavior changes.
- No new dependency is introduced.
- Public API is only narrowed for symbols proven to be implementation-only and unused by in-repo external consumers.
- Historical M001 plans/closure records remain immutable.

## 4. Scope and non-goals

In scope:

- repository-wide symbol census for `NON_STREAMING_PROVIDER_TOTAL` and `non_streaming_timeout`;
- visibility narrowing in `provider_core.rs`;
- removal of unnecessary public re-exports from `crates/codegg-providers/src/lib.rs`;
- compile/test evidence that intended provider call sites still work;
- deterministic hardening of both redirect fixtures;
- a focused regression that intentionally exercises the previous transient-read condition;
- targeted test repetition to demonstrate the flake is removed;
- normal formatting/Clippy/quick verification;
- closure record and planning reconciliation.

Out of scope:

- changing the 60-second metadata timeout value;
- changing the Responses default timeout;
- changing stream setup/idle timeouts;
- changing production redirect policy;
- HTTPS→HTTP downgrade hardening;
- consolidating root/provider production clients;
- introducing a shared production HTTP utility;
- dependency upgrades;
- broad test utility refactors;
- unrelated flaky-test cleanup.

## 5. Ordered work packages

### WP1 — Confirm symbol and fixture ownership

Before editing, census:

```bash
rg -n "NON_STREAMING_PROVIDER_TOTAL|non_streaming_timeout" .
rg -n "fn read_http_request|fn redirect_server" src crates tests
```

Classify every match as:

- provider-core definition;
- intra-crate consumer;
- crate-root re-export;
- external workspace consumer;
- docs/planning/history.

If any production consumer outside `codegg-providers` imports the timeout symbols through the public crate API, stop and evaluate whether that coupling is intentional before narrowing visibility.

Expected result at plan creation: no such external production consumer exists.

### WP2 — Narrow provider timeout-policy visibility

Preferred implementation:

- change `NON_STREAMING_PROVIDER_TOTAL` from `pub` to `pub(crate)`;
- change `non_streaming_timeout()` from `pub` to `pub(crate)`;
- remove both symbols from `crates/codegg-providers/src/lib.rs` public re-exports.

Keep the function in `provider_core`; sibling provider modules may continue using `crate::provider_core::non_streaming_timeout()`.

Do not inline the timeout into every caller merely to remove the symbol. The helper remains useful as an internal single policy point.

Do not change `create_http_client()` visibility as part of this corrective; it has broader provider-crate utility and was not identified as residual debt.

Update rustdoc wording only where necessary to stop presenting the timeout helper as supported external API.

### WP3 — Design a bounded redirect request reader

Replace the current "any read error panics" behavior in both test fixtures with an explicitly bounded strategy.

The helper must:

- read until `\r\n\r\n`, EOF, or a local fixture deadline;
- retry `std::io::ErrorKind::WouldBlock`;
- treat `Interrupted` as retryable;
- avoid an unbounded busy-spin;
- fail with a diagnostic if the bounded deadline expires or a non-retryable read error occurs;
- preserve the existing redirect server's overall bounded lifetime.

Acceptable narrow implementations include:

1. keep the accepted stream nonblocking and make the reader `WouldBlock`-aware with a short sleep/yield and deadline; or
2. explicitly put the accepted stream into blocking mode and give it a finite read timeout, with timeout handling that remains bounded.

Prefer the smallest cross-platform behavior whose regression can be deterministically induced.

Do not change production Eggfetch I/O or socket settings.

### WP4 — Add deterministic regression coverage

Add a focused test for the request-reader behavior that guarantees at least one transient not-ready read before request bytes arrive.

The regression should coordinate client/server timing rather than depend on chance. A suitable shape is:

1. bind a loopback listener;
2. connect the client but deliberately withhold request bytes;
3. accept the connection and force/use nonblocking mode on the accepted stream;
4. invoke the hardened reader;
5. coordinate a later client write through a channel/barrier or bounded delay;
6. assert the complete header terminator is eventually consumed without panic.

The test must fail against the old unconditional `expect("read HTTP request")` behavior under the induced condition.

If the two fixtures use identical hardened logic, either:

- retain two small test-local copies and cover each owning redirect test; or
- factor a test-only helper at an already-natural shared test boundary.

Do not create a production module or public utility solely to deduplicate a few test lines.

### WP5 — Re-run redirect behavior tests

Required focused coverage:

- root ordinary-client redirect success;
- root ten-hop overflow rejection;
- provider shared-client redirect success;
- provider ten-hop overflow rejection;
- new induced-`WouldBlock` regression.

Run the affected redirect tests repeatedly enough to catch obvious residual nondeterminism, for example:

```bash
for i in $(seq 1 20); do
  cargo test --lib http_client::tests::ordinary_builder_follows_redirects_and_enforces_ten_hop_bound --locked -- --exact --test-threads=1 || exit 1
done

for i in $(seq 1 20); do
  cargo test -p codegg-providers provider_core::tests::shared_client_follows_redirects_and_enforces_ten_hop_bound --locked -- --exact --test-threads=1 || exit 1
done
```

Use the actual test selectors accepted by the current harness; the intent is bounded repetition, not these literal commands if names differ.

A repeated pass is supplemental evidence. The deterministic induced-condition test is the primary regression proof.

### WP6 — API-surface and behavior verification

Run at minimum:

```bash
cargo check -p codegg-providers --locked
cargo test -p codegg-providers --locked -- --test-threads=1
cargo test --lib http_client --locked -- --test-threads=1
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
scripts/verify.sh quick
```

Follow `AGENTS.md`: do not use workspace `--all-features`, because `lsp-real-server-tests` requires installed servers.

Also repeat the symbol census after editing and confirm:

- `non_streaming_timeout` has only crate-internal definition/uses plus documentation/history;
- `NON_STREAMING_PROVIDER_TOTAL` is not publicly re-exported;
- no source outside `codegg-providers` imports either symbol.

No Cargo dependency/lockfile change is expected. If `Cargo.toml` or `Cargo.lock` changes, stop and justify it rather than treating it as incidental.

## 6. Failure and compatibility semantics

### Provider policy visibility

This is an intentional Rust API-surface contraction for symbols that are not intended as supported external API.

If current repository consumers reveal an actual cross-crate dependency, do not silently break it. Either:

- retain the minimal visibility needed and document the owner; or
- move the policy behind an already-supported provider API if that is clearly the existing architecture.

Do not create a replacement public wrapper solely to preserve accidental exposure.

### Redirect fixture

Retryable readiness errors are fixture conditions, not redirect failures. The test helper should keep waiting within its local deadline.

Non-retryable read errors and deadline expiry must still fail loudly with useful context. Do not turn the fixture into "ignore all read errors."

## 7. Documentation effects

No user-facing documentation change is expected.

Review `architecture/provider.md` only for wording that implies `non_streaming_timeout()` is public API. It may continue naming the internal helper when explaining timeout ownership.

Do not rewrite:

- `plans/closure/eggfetch-0-2-adoption/001-status.md`;
- the M001 implementation plan;
- the closed Eggfetch 0.2 roadmap history.

The new corrective closure owns the new evidence.

## 8. Closure evidence required

Create:

- `plans/closure/eggfetch-0-2-adoption-maintenance-corrective/001-status.md`

It must record:

- implementation commit(s);
- pre/post symbol census;
- exact visibility/re-export changes;
- confirmation of no external in-repo consumers;
- redirect-helper implementation choice and why it remains bounded;
- deterministic induced-`WouldBlock` regression result;
- repeated root/provider redirect test results;
- provider/root focused test results;
- format, Clippy and `verify.sh quick` results;
- confirmation that Cargo manifests/lockfile and production HTTP behavior were unchanged;
- unresolved findings by severity;
- final disposition.

## 9. Acceptance criteria

- `NON_STREAMING_PROVIDER_TOTAL` and `non_streaming_timeout()` are no more public than current intra-crate consumers require.
- Neither symbol is publicly re-exported from `codegg-providers`.
- All intended non-streaming provider operations retain their existing finite timeout behavior.
- Both redirect fixtures tolerate transient `WouldBlock`/interrupted readiness without panic.
- The redirect request reader remains deadline-bounded and still fails on terminal I/O errors.
- A deterministic regression induces the old failure condition and passes with the fix.
- Existing redirect-follow and ten-hop-limit assertions remain unchanged and pass.
- No production HTTP code, Eggfetch configuration, timeout duration, retry policy, trust policy, dependency manifest or lockfile changes.
- Canonical verification passes.
- No unresolved medium-or-higher C001 finding remains.

## 10. Stop conditions

Stop and report rather than widen scope if:

- an external workspace consumer relies on the timeout symbols as public API;
- narrowing the symbols requires provider API redesign;
- the redirect flake is traced to production Eggfetch behavior rather than the test helper;
- deterministic fixture hardening requires production socket changes;
- fixing the helper requires a new dependency or broad test framework;
- Cargo dependency/lockfile changes appear unexpectedly;
- focused tests reveal a production redirect or timeout regression;
- unrelated verification failures cannot be resolved within this bounded corrective.

## 11. Handoff notes

- Preserve unrelated user changes.
- Keep M001 closed and its evidence immutable.
- Treat this as polish, not a new Eggfetch adoption milestone.
- Prefer a deterministic induced-readiness regression over large repetition counts.
- Keep the helper bounded; never replace a flake with a possible hang.
- Do not broaden into redirect downgrade/security policy work.
