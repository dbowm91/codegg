# HTTP Client Consolidation M003 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/http-client-consolidation/003-remaining-http-consumers-and-reqwest-retirement.md`

Source subsystem roadmap:

- `plans/subsystems/http-client-consolidation-roadmap.md`

Repository baseline reviewed: `ff448fc`

Implementation commits or pull requests:

- `ff448fc` — migrate remaining CodeGG-owned HTTP consumers to published Eggfetch 0.1.4 and retire direct reqwest ownership

## 1. Executive finding

M003 is complete and strictly closed. All remaining CodeGG-owned production
HTTP consumers in search, research, MCP OAuth, plugin installation, the
remote SDK, image generation, update checks, and EggLSP downloads now use the
published crates.io `eggfetch-core 0.1.4` transport. Root and EggLSP direct
reqwest ownership is retired, ordinary redirect and timeout policy is
explicit, and existing protocol, parsing, authentication, and archive-safety
boundaries remain in place.

This is an ownership and control consolidation. No binary-size win is claimed.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Approved published Eggfetch profile | `Cargo.toml`, `crates/egglsp/Cargo.toml`, `Cargo.lock`, `cargo tree -i eggfetch-core --locked` | pass | Root uses `http1,tls-rustls,json`; EggLSP uses `http1,tls-rustls`; all resolve `eggfetch-core v0.1.4`. |
| Every remaining production consumer migrated | Search/research sources, SDK, image, upgrade, MCP OAuth, plugin install, and EggLSP downloader | pass | Final source and manifest census found no active CodeGG-owned reqwest consumer. |
| Ordinary redirect and timeout semantics preserved | Explicit `follow_redirects(true)`, `max_redirects(10)`, and owner-specific bounded `Timeout` policies | pass | No consumer silently inherited Eggfetch's no-follow/unbounded defaults. |
| Search/research behavior preserved | Root search/research focused suites | pass | Query construction, headers, parsing, limits, provenance, and source-local fallback behavior passed. |
| SDK/image/update behavior preserved | Server-feature SDK tests and existing root test suites | pass | Auth/header construction, response handling, best-effort update mapping, and image payload paths compile and pass. |
| EggLSP redirect/download and extraction safety | `cargo test -p egglsp --all-features -- --test-threads=1` | pass | 1,000 unit tests and all integration suites passed, including loopback redirect/raw-byte coverage and archive/link protections. |
| Network-source guard remains effective | `scanner_catches_eggfetch_and_legacy_http_clients` | pass | Eggfetch client/request symbols are rejected and legacy reqwest markers remain covered. |
| Direct reqwest retirement | `rg` census and `cargo tree -i reqwest --locked` | pass | No package matched `reqwest`; remaining text hits are intentional legacy guard/history references. |
| Active documentation is truthful | Active-doc census and updates to dependency, architecture, search/backend, and upgrade text | pass | Current docs describe Eggfetch ownership and do not claim an unmeasured footprint win. |
| No medium-or-higher finding remains open | Final invariant, security, and compatibility review | pass | No migration finding is medium or higher. |

## 3. Production implementation evidence

The root search and research adapters, SDK, image client, update checker,
MCP OAuth exchange/refresh/revocation paths, plugin installer, and EggLSP
downloader use Eggfetch 0.1.4 directly at their existing ownership
boundaries. URL-only uses were changed to `url::Url`; no generic CodeGG-wide
HTTP wrapper or service was introduced.

Request construction handles Eggfetch's fallible builder/JSON APIs and
mutable response-body APIs explicitly. Ordinary clients use bounded redirects
and explicit timeouts, while existing security-sensitive static-routing paths
remain owned by their earlier migration. EggLSP keeps its existing extraction
ordering, archive traversal checks, symlink/hard-link rejection, binary
matching, and executable handling.

The root error layer no longer converts a reqwest error, and Cargo.lock was
resolved normally after the direct dependency removals.

## 4. Verification executed

The macOS host required the existing x86_64-compatible pkg-config selection
for native compression libraries during link-heavy tests:
`PKG_CONFIG_PATH=/usr/local/lib/pkgconfig PKG_CONFIG_LIBDIR=/usr/local/lib/pkgconfig`.
This affected test-environment library selection only.

### Commands and results

```text
cargo test --lib search -- --test-threads=1                         264 passed
cargo test --lib research -- --test-threads=1                       123 passed
cargo test --lib client -- --test-threads=1                          41 passed
cargo test --features server --lib client -- --test-threads=1       42 passed
cargo test --lib upgrade -- --test-threads=1                          0 passed
cargo test -p egglsp --all-features -- --test-threads=1             1,000 unit + all integration suites passed
cargo check --workspace --all-targets --locked                         passed
cargo fmt --all -- --check                                             passed
cargo clippy --workspace --all-targets --all-features -- -D warnings  passed
scripts/verify.sh quick                                                 passed
```

The non-server `client` command is recorded because it is the plan's exact
command; the server-feature run additionally exercised the SDK module and its
invalid-authorization-header test.

## 5. Invariant review

- M001's validated-destination/static-routing security behavior is unchanged.
- M002's provider transport and streaming behavior is unchanged.
- Ordinary clients explicitly preserve reqwest-era redirect behavior with a
  ten-hop cap and retain bounded owner-specific timeouts.
- Search/research parsing, provenance, rate limits, and fallback semantics
  remain source-local.
- SDK bearer credentials remain header-only; invalid header construction fails
  at client creation.
- Image and update calls retain their existing payload, response, and
  best-effort/error semantics.
- EggLSP archive and link protections remain strict and pass the full suite.
- No automatic retries, global HTTP singleton, protocol redesign, or new
  cross-workspace abstraction was added.

## 6. Failure and recovery review

Each client retains its existing owner and lifetime. Transport, request-build,
JSON, response-body, and status failures terminate through existing
source-local error/fallback channels. Dropping the owning future still
cancels direct SDK, image, update, OAuth, plugin, and download operations.

The EggLSP downloader preserves write/extraction ordering so a failed download
cannot become a falsely valid executable artifact. No new retry loop or
restart/persistence behavior was introduced.

## 7. Migration and compatibility review

No database, session, wire-protocol, provider-contract, or user configuration
schema changed. The Cargo.lock changes are the normal dependency graph result
of retiring direct reqwest declarations. `codegg-core` remains transport
neutral, and historical plans/closure records retain their historical
reqwest references.

Eggfetch's explicit fallible request and mutable response APIs are adapted at
each existing source boundary. The approved transport profile remains
HTTP/1 + Rustls/WebPKI without native roots or additional protocol features.

## 8. Security review

The network-source scanner now recognizes Eggfetch client/request symbols and
continues to reject legacy reqwest markers. OAuth Basic authorization and
bearer/token headers are constructed through Eggfetch's validated header/auth
surface. Existing error mappings remain sanitized and do not add secret URLs
to CodeGG-owned error data. EggLSP archive traversal, symlink, and hard-link
tests remain green.

## 9. Documentation and operations

Updated active documentation and contributor guidance include:

- `docs/dependency-maintenance.md`;
- `architecture/client.md`, `architecture/core.md`, `architecture/error.md`,
  and `architecture/provider.md`;
- `.opencode/skills/upgrade/SKILL.md`;
- current search/backend provenance and network-guard comments.

No permanent dependency scanner, binary-size gate, new CI lane, or release
automation was added.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| low | `cargo tree -d` still reports the pre-existing DashMap 5/6 major-version split (Eggfetch uses DashMap 6) | Duplicate dependency remains, but DashMap migration is explicitly outside M003 scope | Revisit only as separately authorized dependency work. |

No critical, high, or medium findings remain. No informational artifact-size
measurement was used to support closure because the plan makes footprint
observations optional and non-gating.

## 11. Roadmap disposition

M003 is closed with accepted evidence. The HTTP Client Consolidation and
Eggfetch Adoption roadmap is now closed with M001-M003 closure records.

The registry was audited for dependency-ready and blocked work. No registered
future plan lists M003 as a prerequisite, so no future plan became unblocked
or required a status transition. The unrelated Architecture Convergence M009
and Runtime Safety C002 conditional blockers remain unchanged:

- compatible-host root runtime / strict all-feature Clippy evidence;
- supported-Linux Landlock fixture evidence.

## 12. Registry updates

- Marked the M003 implementation plan `implemented`.
- Marked the subsystem roadmap and M003 milestone `closed`.
- Added this closure record and implementation commit `ff448fc` to
  `plans/registry.md`.
- Removed M003 from dependency-ready work and recorded that no future plan was
  unblocked by this closure.
- Preserved the unrelated architecture and runtime-safety blockers.
