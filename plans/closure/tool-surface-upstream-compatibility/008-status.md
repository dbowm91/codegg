# Tool-Surface Upstream Compatibility M008 — Closure Status

Status: conditionally closed

Source implementation plan: `plans/implementation/tool-surface-upstream-compatibility/008-eggsact-1.2.5-inprocess-compatibility.md`

Source subsystem roadmap: `plans/subsystems/tool-surface-upstream-compatibility-corrective-addendum.md#milestones`

Repository baseline reviewed: `2798501b61edcd43acb215924500635dc705bbb2`

Implementation commits:

- `2055696` — delegate eggsact profile validation to the upstream parser,
  reject invalid profiles without fallback, update exposure/config tests, and
  reconcile deterministic-tool documentation;
- this closure commit — accept M008 evidence, record the MSRV condition, and
  reconcile the planning registry.

Audited upstream baseline: eggsact `1.2.5` (crates.io source, released
2026-09-11).

## 1. Executive finding

M008's in-process compatibility correction is complete and safe to retain,
but strict adoption of eggsact 1.2.5 is conditionally closed. The linked
CodeGG dependency remains eggsact `1.1.4` because eggsact `1.2.5` declares
Rust `1.89.0`, while CodeGG's authoritative package and documentation
baseline is Rust `1.81`. The upgrade was compiled successfully on the local
Rust 1.98 toolchain for API audit, then deliberately reverted so this change
does not silently raise CodeGG's MSRV.

The unsafe behavior is fixed: profile selection now uses eggsact's parser and
authoritative profile metadata, invalid names retain their identity through
config resolution, and runtime construction fails with the invalid name,
accepted profiles, and an explicit no-fallback statement.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Resolve against 1.2.5 or document the blocker | `cargo tree -i eggsact`; eggsact 1.2.5 manifest audit; CodeGG `rust-version = "1.81"` | conditional | 1.2.5 requires Rust 1.89; lockfile intentionally remains 1.1.4 |
| Remove duplicated profile allowlists | `src/eggsact/adapter.rs`, `src/tool/integrated_config.rs`, `crates/codegg-config/src/schema.rs`; repository search finds no `KNOWN_EGGSACT_PROFILES`/`KNOWN_PROFILES` | pass | config crate checks only non-empty shape; linked runtime owns known-name validation |
| Reject invalid profiles without changing policy | `tests/eggsact_deterministic_tools.rs::unknown_profile_fails_without_fallback` | pass | Error includes name, accepted profiles, and `no fallback was applied` |
| Accept current upstream profile names | `every_upstream_profile_is_accepted_by_the_runtime`; config integration profile loop | pass | Tests iterate eggsact `mcp::registry::available_profiles()` |
| Preserve model/harness audience boundaries | `model_audience_blocks_harness_only_tools`; `harness_audience_can_access_harness_tools`; upstream exposure audit | pass | CodeGG continues to use Model for model calls and Harness for preflight |
| Assess typed `DependencyPreflight` | `architecture/preflight.md`; current `src/preflight/service.rs` call-site audit | deferred | No dependency-edit preflight consumer exists; adoption would add an unused parallel path |
| Review new utility/discovery surface | curated wrapper inventory and `architecture/deterministic_tools.md` | deferred by design | No new utilities or eggsact MCP discovery facade entered CodeGG |
| Preserve in-process ownership | `EggsactRuntime` still wraps `eggsact::agent::ToolRegistry` directly | pass | No subprocess, MCP hop, or second disclosure system |

## 3. Production implementation evidence

- `EggsactRuntime::new()` calls `Profile::from_str_opt()` and obtains the
  accepted-name diagnostic list from `eggsact::mcp::registry::available_profiles()`.
  It returns a `ToolError` for unknown names rather than constructing
  `Profile::Default`.
- Integrated config resolution preserves the configured profile verbatim.
  `ToolRegistry::with_options()` logs the actionable initialization failure,
  and the CLI deterministic-tools report prints it for operator diagnosis.
- The config crate no longer duplicates upstream profile membership; it only
  rejects an empty profile before the linked runtime performs authoritative
  validation.
- Model-facing CodeGG wrappers remain the existing curated eight
  always-visible plus five deferred entries. Upstream's larger utility set and
  `tool_search`/`tool_invoke` facade remain outside CodeGG's palette.
- Existing structured eggsact response fields and machine-code handling were
  not changed. No manual dependency-preflight JSON consumer was found to
  replace with `DependencyPreflight`.

## 4. Verification executed

### Commands and outcomes

```text
rtk cargo check -p codegg --lib                                      pass
rtk cargo tree -i eggsact                                             pass — eggsact v1.1.4
rtk cargo test --lib eggsact -- --test-threads=1                     pass — 21 passed
rtk proxy env LZMA_API_STATIC=1 RUSTFLAGS='-C link-arg=-Wl,-force_load,/usr/local/opt/xz/lib/liblzma.a' CARGO_BUILD_JOBS=1 cargo test --test eggsact_adapter -- --test-threads=1
                                                                      pass — 17 passed
rtk proxy env LZMA_API_STATIC=1 RUSTFLAGS='-C link-arg=-Wl,-force_load,/usr/local/opt/xz/lib/liblzma.a' CARGO_BUILD_JOBS=1 cargo test --test eggsact_deterministic_tools -- --test-threads=1
                                                                      pass — 26 passed
rtk proxy env LZMA_API_STATIC=1 RUSTFLAGS='-C link-arg=-Wl,-force_load,/usr/local/opt/xz/lib/liblzma.a' CARGO_BUILD_JOBS=1 cargo test --test preflight_integration -- --test-threads=1
                                                                      pass — 72 passed
rtk proxy env LZMA_API_STATIC=1 RUSTFLAGS='-C link-arg=-Wl,-force_load,/usr/local/opt/xz/lib/liblzma.a' CARGO_BUILD_JOBS=1 cargo clippy --workspace --all-targets --all-features -- -D warnings
                                                                      pass
rtk proxy env LZMA_API_STATIC=1 RUSTFLAGS='-C link-arg=-Wl,-force_load,/usr/local/opt/xz/lib/liblzma.a' CARGO_BUILD_JOBS=1 scripts/verify.sh quick
                                                                      pass
rtk cargo fmt --all -- --check                                    pass
rtk git diff --check                                             pass
```

The initial ordinary test-link attempt failed on this macOS host because
`/opt/local` supplied arm64 liblzma/libiconv while the target was x86_64.
The documented static-xz workaround made all focused tests pass. This is a
local linker condition, not an M008 production finding.

## 5. Invariant review

- Eggsact remains in-process and CodeGG still owns the model-facing palette
  and progressive disclosure.
- Model audience calls cannot execute harness-only tools; preflight retains a
  separate Harness audience. No Debug audience is used by production config.
- Invalid profile configuration fails visibly and deterministically. Neither
  config resolution nor runtime construction substitutes `default` or
  `codegg_core`.
- Structured result, findings, warnings, and machine-code fields remain typed
  data at the CodeGG boundary.
- The immediate model-facing palette did not grow, and no MCP discovery facade
  or utility parity layer was introduced.

## 6. Failure and recovery review

Profile initialization is fail-closed at the runtime boundary. Registry
construction logs the error and omits deterministic wrappers rather than
silently selecting another policy. The diagnostics path reports the same
failure for operator action. Preflight's established fail-open behavior on
tool execution failures is unchanged; this does not apply to invalid runtime
configuration.

No durable state, storage migration, subprocess, or new concurrency path was
introduced. Reverting `2055696` restores the old fallback defect but requires
no data migration; the intentional dependency decision remains independently
reversible.

## 7. Migration and compatibility review

Valid existing profiles remain valid, and all profile names exposed by the
linked upstream registry are accepted. Unknown names now require correction;
they no longer receive an implicit policy. The config schema's profile field
is intentionally a non-empty string because `codegg-config` does not mirror
eggsact's registry. No user-visible config migration is needed for valid
configurations.

The 1.2.5 upgrade is not included because its declared MSRV would break the
repository's Rust 1.81 compatibility contract. A future MSRV decision can
upgrade `Cargo.toml`/`Cargo.lock` and then enable the already-audited typed
API without reopening the profile-safety design.

## 8. Security review

The removed fallback prevented an invalid profile from silently widening or
narrowing execution policy. Accepted profile names and exposure semantics are
read from eggsact rather than copied into CodeGG. Model-facing registrations
remain explicitly curated and harness-only tools remain unavailable to the
model audience. No external network, credential, MCP, or provider boundary
changed.

## 9. Documentation and operations

- `architecture/deterministic_tools.md` records the 1.1.4 manifest/lock/test
  baseline, 1.2.5 MSRV condition, upstream profile validation, and curated
  eight-plus-five palette.
- `architecture/preflight.md` records the typed `DependencyPreflight`
  deferral and existing harness ownership.
- `architecture/native_crates.md`, `architecture/config.md`, and
  `architecture/tool.md` now describe the dependency, config validation, and
  profile ownership accurately.
- No CI workflow, compatibility matrix, network smoke, dependency bot, or
  release automation was added.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| medium | eggsact 1.2.5 declares Rust 1.89.0, above CodeGG's Rust 1.81 MSRV | The audited 1.2.5 API cannot become the locked production dependency without an MSRV decision | Make an explicit MSRV/compatibility decision, then update manifest and lockfile in a follow-up plan if approved |
| low | Default macOS linker selects incompatible `/opt/local` arm64 libraries for x86_64 test links | Unworkarounded local test links fail on this host | Use the recorded static-xz workaround or a compatible host/toolchain |

No critical or high-severity defect remains. The medium MSRV item is the
named condition for this conditional closure, not an unrecorded assumption.

## 11. Roadmap disposition

M008 is conditionally closed. Its production correctness boundary is complete;
the direct 1.2.5 dependency upgrade is deferred until CodeGG explicitly
revises its Rust MSRV policy. The parent corrective roadmap is therefore also
conditionally closed with this condition recorded.

## 12. Registry updates

- M008 was moved from active implementation to conditionally closed and
  recorded under recently closed work with implementation commit `2055696`.
- The dependency-ready table is empty; no future plan lists M008 as a hard or
  interface dependency.
- The registry's blocked-work audit found only Architecture M009's compatible
  host/all-feature evidence blocker and Runtime Safety C002's supported-Linux
  Landlock evidence blocker. M008 resolves neither, so neither plan was
  unblocked or had its status changed.
- The deferred typed dependency-preflight and 1.2.5/MSRV follow-ups remain
  unregistered until a concrete consumer or explicit MSRV decision makes a
  bounded plan dependency-ready.
