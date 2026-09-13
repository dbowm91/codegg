# HTTP Client Consolidation M003 — Remaining HTTP Consumers and Reqwest Retirement

Status: ready for handoff

Repository baseline reviewed: `a848872573c0604eb52a3ae1f46b6edef0c3eb29`

Source subsystem roadmap:

- `plans/subsystems/http-client-consolidation-roadmap.md`

Predecessors:

- `plans/implementation/http-client-consolidation/001-transport-boundary-and-pinned-http-adoption.md`
- `plans/implementation/http-client-consolidation/002-provider-streaming-and-eggpool-adoption.md`
- accepted closure records for M001 and M002.

## 1. Objective

Migrate every remaining CodeGG-owned reqwest consumer to the published `eggfetch-core 0.1.4` transport, including built-in search/research sources, image generation, the remote SDK, update checks and EggLSP downloads; then remove direct reqwest ownership from active manifests/source, update active documentation/source guards, and close the subsystem with truthful dependency/footprint evidence.

This milestone owns final retirement. It must not merely make the main binary compile while leaving a secondary crate, feature, test helper, source guard, or active documentation path semantically tied to reqwest.

## 2. Readiness

Do not begin until M002 closes with the provider crate reqwest-free and broad verification green.

Use the established M001/M002 conventions for:

- Eggfetch dependency feature selection;
- explicit redirects/timeouts;
- mutable/fallible response-body access;
- sanitized error conversion;
- URL parsing through `url::Url` where only URL semantics are required.

## 3. Current evidence

Remaining root-package reqwest ownership at the reviewed baseline spans several categories:

### Search and research

Built-in search clients such as GitHub, Hacker News Algolia, OpenAlex, arXiv, PubMed, Mojeek, Wikipedia, Google News and related providers own reqwest clients and use combinations of query parameters, headers, JSON/text responses and explicit timeouts.

Research adapters such as advisory/docs.rs/crates.io/GitHub and related network sources also own reqwest clients. The security-sensitive direct URL source is handled earlier by M001.

`src/search/types.rs` has a reqwest-specific error conversion into `SearchError::Transport`.

### Root utility clients

- `src/client/sdk.rs` builds a reqwest client with optional default bearer authorization, a 10s connect timeout, and a 10s per-health-request timeout.
- `src/tool/image.rs` uses reqwest client and URL types for image-generation HTTP calls.
- `src/upgrade/mod.rs` uses a 10s reqwest client for release/update checks.
- root URL-only call sites use `reqwest::Url` in Eggpool/search/research/tool paths.

### EggLSP

`crates/egglsp/src/download.rs` uses `reqwest::Client::new()` to download LSP archives/binaries, follows reqwest's ordinary redirects, checks success status, buffers the response bytes, and then applies the existing archive traversal/link protections.

`crates/egglsp/Cargo.toml` directly owns reqwest with `stream,rustls-tls`.

### Active docs and guards

Active architecture/dependency docs name reqwest as the HTTP baseline. `src/tool/lsp_security.rs` also includes reqwest symbols in its forbidden-network/source inspection logic; that guard must be updated so changing the implementation does not accidentally weaken the policy it was intended to enforce.

## 4. Invariants

- M001's security-sensitive pinning behavior remains unchanged.
- M002's provider transport remains unchanged except for shared lockfile resolution.
- Ordinary clients preserve redirect behavior that reqwest supplied by default. A mechanical Eggfetch default no-follow migration is not acceptable.
- Ordinary clients remain bounded by their existing explicit timeout or by an intentionally documented replacement for reqwest defaults.
- Search/research result parsing, provenance, rate limits and source-quality semantics remain unchanged.
- Image-generation request/response semantics remain unchanged.
- Remote SDK bearer credentials remain header-only and redacted from errors/logs.
- Update checking remains best-effort/bounded and does not become a startup blocker.
- EggLSP archive traversal/symlink/hard-link protections remain exactly as strict as before; the HTTP swap does not reopen extraction policy.
- Source guards that prohibit network-capable code continue to prohibit the new Eggfetch path and may retain reqwest strings as a legacy forbidden pattern.
- Historical plans/closure records are not rewritten.

## 5. Scope and non-goals

In scope:

- remaining root-package production reqwest consumers after M001/M002;
- all built-in search and research source HTTP clients;
- `src/client/sdk.rs`;
- `src/tool/image.rs`;
- `src/upgrade/mod.rs`;
- root URL-only reqwest parsing;
- `src/search/types.rs` transport error conversion;
- `crates/egglsp/Cargo.toml` and `crates/egglsp/src/download.rs` plus any other EggLSP reqwest consumers discovered by census;
- active dependency/architecture/user/contributor documentation that describes the current HTTP client;
- network-source static guards;
- final root/EggLSP reqwest manifest removal and lockfile cleanup;
- bounded dependency-tree/artifact observations for closure.

Out of scope:

- introducing new search/research backends;
- changing extraction/ranking/content policies;
- redesigning image generation;
- adding streaming archive extraction to EggLSP;
- automatic update installation;
- dependency upgrades unrelated to removing reqwest;
- DashMap 5→6 migration merely to deduplicate Eggfetch's DashMap 6 dependency;
- permanent binary-size or dependency-count gates.

## 6. Production changes

### 6.1 Apply explicit client policy per ordinary consumer

Reqwest and Eggfetch have different defaults. For each remaining client construction site, classify the old behavior before replacing it:

- explicit request/client timeout;
- implicit reqwest request timeout where `Client::new()` was used;
- redirect behavior;
- headers/user agent/auth;
- query serialization;
- JSON/text/bytes body handling.

Ordinary clients that previously inherited reqwest redirect behavior should use:

```text
follow_redirects(true)
max_redirects(10)
```

unless the existing code explicitly disabled/overrode redirects.

Where code relied on `reqwest::Client::new()` rather than an explicit timeout, choose and document a bounded CodeGG timeout matching the old practical contract instead of silently adopting Eggfetch's disabled timeout default. Prefer the reqwest-era 30s request bound unless the owning subsystem already documents a different value.

Use the same Rustls/WebPKI feature profile selected by earlier milestones. Do not add native roots or additional protocols during retirement.

### 6.2 Migrate search and research adapters

For each source:

- replace `reqwest::Client` with `eggfetch_core::Client`;
- translate query construction to repeated `.query(key, value)` calls or explicit `url::Url` query mutation where dynamic collections make that clearer;
- use native JSON response decoding where it reduces boilerplate, handling mutable responses and decode errors explicitly;
- preserve source-specific user agents, auth headers, result limits and timeout values;
- preserve non-success status handling and any best-effort fallback semantics;
- map Eggfetch transport failures into `SearchError::Transport` / `ResearchError` without leaking secret URLs.

Do not centralize all search/research clients behind one generic transport wrapper. Existing source adapters remain the policy boundary.

### 6.3 Migrate remote SDK

Replace reqwest with Eggfetch while preserving:

- normalized base URL;
- optional bearer authorization as a client default header or request header;
- 10s connect timeout;
- 10s health-call bound;
- success/non-success classification.

Header construction must remain fallible at client creation so an invalid token/header cannot become a delayed runtime surprise.

Explicitly enable ordinary redirects only if preserving the old reqwest client behavior is desired for the SDK endpoint; document the decision in the focused test.

### 6.4 Migrate image and update clients

For image generation:

- switch URL parsing to `url::Url` where parsing only is needed;
- preserve auth/header/JSON payload behavior;
- preserve response JSON/binary handling and existing size/error semantics;
- do not broaden accepted URL schemes or redirect policy.

For update checks:

- preserve the existing 10s bound;
- preserve normal redirect following needed by release-host/CDN endpoints;
- keep failures mapped to `AppError::Upgrade` / existing best-effort UX rather than generic server failure;
- do not add background polling or release automation.

### 6.5 Migrate EggLSP downloads

Replace the EggLSP reqwest dependency with:

```toml
eggfetch-core = { version = "0.1.4", default-features = false, features = ["http1", "tls-rustls"] }
```

Configure the downloader client with an explicit bounded timeout and `follow_redirects(true), max_redirects(10)` because release asset URLs commonly redirect and reqwest previously followed redirects by default.

Adapt response body consumption to Eggfetch's mutable `bytes()` API.

Do not change:

- cache paths;
- archive type detection;
- ZIP/TAR traversal validation;
- symlink/hard-link rejection;
- binary name matching;
- executable permission handling.

A future streamed-to-disk/archive-size hardening pass is separate work unless implementation uncovers a concrete current regression.

### 6.6 Retire direct reqwest ownership

After all production consumers compile and focused tests pass:

- remove reqwest from root `Cargo.toml`;
- ensure it is already absent from `codegg-core` and `codegg-providers` from M001/M002;
- remove reqwest from `crates/egglsp/Cargo.toml`;
- resolve `Cargo.lock` normally;
- run a source/manifest census for production references.

Use `cargo tree -i reqwest` to determine whether any unrelated transitive dependency still resolves reqwest. The success condition is **no CodeGG-owned direct transport dependency/use**. If an unrelated third party still carries reqwest transitively, record that precisely; do not add patches or forks solely to make the package name disappear.

### 6.7 Update active network-source guards

Audit `src/tool/lsp_security.rs` and any similar guard that identifies forbidden network-capable APIs by source text or namespace.

Required policy:

- add Eggfetch client/request symbols necessary to catch the new network path;
- retain reqwest patterns where they remain useful as a forbidden legacy/network marker;
- update tests so the guard still rejects network access attempts and does not become implementation-name blind.

Do not introduce a new repository-wide source scanner framework.

### 6.8 Refresh active documentation truthfully

Update active docs such as:

- `docs/dependency-maintenance.md`;
- `architecture/provider.md`;
- `architecture/error.md`;
- `architecture/core.md` / `architecture/codegg_core.md` where dependency lists are current-state descriptions;
- contributor/upgrade skill text that gives reqwest-specific dependency instructions;
- any current README/architecture section found by census.

Historical plans, closure records and review artifacts retain their historical reqwest references.

Documentation must state that the migration is an ownership/consolidation decision, not a demonstrated size win.

## 7. Ordered work packages

### WP1 — Built-in search/research adapters

Migrate source clients in bounded groups and run their deterministic parsing/request tests after each group.

### WP2 — SDK/image/update utilities

Migrate the remaining root utility clients and URL-only types, preserving explicit auth/timeout/redirect behavior.

### WP3 — EggLSP downloader

Replace EggLSP transport, add redirect/body-focused loopback tests if current coverage does not exercise release-asset redirects, and preserve archive security tests.

### WP4 — Error and source-guard cleanup

Remove the final reqwest-specific search/root conversions and update network-source guards for Eggfetch.

### WP5 — Manifest/lockfile retirement

Delete root/EggLSP direct reqwest dependencies, resolve the lockfile, and run direct/transitive dependency census.

### WP6 — Active documentation and closure audit

Refresh current-state docs, run repository-wide active-source census, run broad verification, record bounded dependency/size observations, and create M003 closure evidence.

## 8. Failure, cancellation, restart, and contention semantics

- Each client retains its existing owner/lifetime; do not create a new global HTTP singleton.
- Search/research failures remain source-local and respect existing fallback/partial-result behavior.
- SDK/update/image calls remain directly cancellable by dropping their futures/owning tasks.
- EggLSP download failure must not leave a falsely valid executable artifact; preserve existing write/extraction ordering.
- No new retry loops are added to compensate for transport differences.
- Ordinary redirects are bounded at 10 and do not inherit the security-sensitive static-routing policy from M001 unless the call site already validates untrusted destinations.

## 9. Compatibility and migration behavior

The intended user-visible behavior is unchanged. The key migration trap is Eggfetch's no-follow/no-timeout-by-default posture; this plan makes ordinary redirect and timeout policy explicit so release downloads, update endpoints and public APIs do not silently regress.

The final direct dependency set may not be smaller in bytes. Eggfetch upstream evidence shows aligned minimal artifacts are larger than reqwest despite fewer resolved packages. Any CodeGG-specific artifact observation is informational only.

## 10. Required tests

At minimum cover or retain:

- representative search source query/header/JSON fixture;
- representative research source JSON/text fixture;
- `SearchError` mapping from Eggfetch transport failure;
- SDK valid/invalid authorization header and health success/non-success/timeout;
- image request payload/auth and response handling;
- update-check redirect and timeout behavior with loopback fixture;
- EggLSP HTTP redirect to archive bytes followed by existing secure extraction;
- EggLSP non-success status and body-read failure;
- network-source guard rejects Eggfetch network APIs and still catches legacy reqwest markers;
- root/server error tests after final reqwest conversion removal;
- final active-source/manifests census.

No test should require GitHub, crates.io, search engines or provider APIs over the public Internet.

## 11. Verification commands

Focused commands may use the repository's real target names. Final closure must include the equivalent of:

```bash
cargo test --lib search -- --test-threads=1
cargo test --lib research -- --test-threads=1
cargo test --lib client -- --test-threads=1
cargo test --lib upgrade -- --test-threads=1
cargo test -p egglsp --all-features -- --test-threads=1
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
scripts/verify.sh quick
```

Temporary closure census:

```bash
rg -n '\breqwest\b' Cargo.toml crates/*/Cargo.toml src crates \
  --glob '!plans/**'
cargo tree -i reqwest
cargo tree -i eggfetch-core
cargo tree -d
```

Run an active-document census separately and classify each hit as current text requiring update or historical evidence that must remain unchanged.

If practical on the same host/toolchain, record the final CodeGG release artifact size and dependency-tree shape relative to the pre-migration baseline commit. This is descriptive evidence only; no pass/fail threshold is authorized.

## 12. Documentation updates

This milestone owns the broad current-state refresh. At minimum:

- replace active statements that CodeGG's HTTP clients are reqwest-based;
- describe the explicit Eggfetch feature profile and WebPKI trust choice;
- document provider pool/timeout behavior after M002;
- update error architecture to CodeGG-owned HTTP error data;
- update EggLSP/download architecture if it names reqwest;
- update dependency-maintenance checkpoints;
- remove `generalized HTTP/provider-client unification` from deferred work because this roadmap now owns and completes the bounded migration.

Do not erase historical reqwest references from implementation/closure history.

## 13. Acceptance criteria

- Root, `codegg-providers`, `egglsp` resolve crates.io `eggfetch-core 0.1.4` under their minimal approved features.
- `codegg-core` remains transport-neutral and directly owns `url` where required.
- Every production root/EggLSP HTTP call site is migrated from reqwest.
- Ordinary consumers explicitly preserve redirect and timeout behavior rather than inheriting incompatible Eggfetch defaults.
- EggLSP redirected release downloads work and all existing archive security protections remain green.
- Network-source guards recognize Eggfetch and retain their original policy intent.
- No active CodeGG manifest or production source directly references reqwest.
- Any remaining transitive reqwest package is attributable to an unrelated dependency and documented; otherwise `cargo tree -i reqwest` is empty.
- Active architecture/dependency docs describe Eggfetch and do not claim an unmeasured footprint win.
- Focused tests and broad local verification pass.
- M003 closure contains no unresolved medium-or-higher finding.

## 14. Stop conditions

Stop and record a blocker if:

- an ordinary consumer requires a reqwest feature not represented by the approved Eggfetch profile;
- preserving redirect/timeout behavior requires enabling out-of-scope proxy/H2/H3/native-root capabilities;
- EggLSP redirected downloads cannot be represented without weakening archive/download safety;
- a final reqwest reference belongs to an active behavior not covered by M001/M002/M003 rather than dead/history text;
- source-guard migration would weaken the no-network policy;
- final broad verification exposes a cross-subsystem regression that cannot be fixed narrowly.

## 15. Closure evidence requirements

Create `plans/closure/http-client-consolidation/003-status.md` after implementation. Record:

- implementation commit and resolved Eggfetch version;
- focused root/EggLSP test results;
- broad verification results;
- production source/manifest reqwest census;
- `cargo tree -i reqwest`, `cargo tree -i eggfetch-core`, and duplicate-tree disposition;
- active documentation census;
- informational artifact/dependency observations if collected;
- explicit final statement that the migration is complete on ownership/control grounds, without unsupported size claims.

After accepting M003, update `plans/subsystems/http-client-consolidation-roadmap.md` and `plans/registry.md` to closed status.

## 16. Handoff summary

Treat M003 as retirement and truth-pass work, not an opportunity for broad HTTP abstraction. Migrate the remaining simple clients and EggLSP with explicit redirect/timeout policy, update network guards, remove direct reqwest ownership, refresh active docs, and close only after the production/source/dependency census confirms there is no hidden CodeGG reqwest consumer left.
