# Tool-Surface Upstream Compatibility M009 — Closure Status

Status: closed

Source implementation plan: `plans/implementation/tool-surface-upstream-compatibility/009-eggsact-1.2.5-msrv-adoption.md`

Source subsystem roadmap: `plans/subsystems/tool-surface-upstream-compatibility-corrective-addendum.md#milestones`

Predecessor closure: `plans/closure/tool-surface-upstream-compatibility/008-status.md`

Repository baseline reviewed: `b9a020cf`

Implementation commit:

- `dc0f915` — adopt the approved Rust 1.89 MSRV, resolve eggsact 1.2.5,
  modernize Rust-1.89-supported Clippy patterns, refresh active documentation,
  and register M009 for closure.

## 1. Executive finding

M009 is closed. The user-approved Rust 1.89 MSRV decision resolved M008's only
medium finding: the repository now declares Rust 1.89 across the root and all
workspace packages, and the locked production dependency is eggsact 1.2.5.

The existing in-process profile and exposure boundary remains intact. M009 did
not expand the model-visible deterministic palette, import eggsact's MCP
discovery facade, or introduce a second preflight/disclosure path.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Adopt the approved MSRV | Root and all nine workspace package manifests declare `rust-version = "1.89"`; contributor, release, agent, and dependency docs agree | pass | Rust 1.89 is now the repository compatibility floor |
| Resolve eggsact 1.2.5 | `Cargo.toml`, `Cargo.lock`, and `cargo tree -i eggsact --locked` | pass | The only CodeGG dependency path resolves to eggsact 1.2.5 |
| Preserve M008 profile behavior | `eggsact` library tests and `eggsact_adapter` tests | pass | 21 library tests and 17 adapter tests passed |
| Preserve deterministic and audience boundaries | `eggsact_deterministic_tools` and `preflight_integration` | pass | 26 deterministic-tool tests and 72 preflight tests passed |
| Maintain repository quality gates | strict all-feature Clippy, formatting, diff, and `scripts/verify.sh quick` | pass | All required local gates passed |
| Avoid unapproved scope expansion | source and architecture audit | pass | Curated palette, in-process ownership, and typed-data boundaries remain unchanged |
| Assess typed `DependencyPreflight` | call-site audit recorded in `architecture/preflight.md` | deferred by design | There is still no dependency-edit consumer; no parallel unused path was added |

## 3. Production implementation evidence

- The root manifest now declares `rust-version = "1.89"` and
  `eggsact = "1.2.5"`; every workspace package uses the same MSRV.
- The lockfile contains eggsact 1.2.5 and its required dependency graph. No
  eggsact 1.1.4 resolution remains on the CodeGG path.
- Rust 1.89-supported idioms (`is_none_or` and `repeat_n`) replace the
  equivalent older forms flagged by strict Clippy under the upgraded MSRV.
  These changes preserve behavior and do not alter execution authority.
- Active README, contributor, release, dependency-maintenance, deterministic
  tools, native-crates, preflight, roadmap, registry, and agent guidance now
  describe the approved MSRV and dependency baseline.
- M008's profile parser delegation, fail-closed invalid-profile behavior,
  authoritative upstream profile list, audience separation, and curated
  eight-plus-five palette were retained unchanged.

## 4. Verification executed

The local host uses Rust `1.98.0` (a newer compiler than the declared MSRV),
which can compile and lint the Rust 1.89-compatible code. The host's ordinary
test linker selects incompatible `/opt/local` arm64 libraries for the x86_64
target, so the focused test and broad local commands used the existing static
xz workaround shown below.

```text
rtk cargo fmt --all -- --check
                                                                  pass
rtk git diff --check
                                                                  pass
rtk cargo tree -i eggsact --locked
                                                                  pass — eggsact v1.2.5 -> codegg v0.1.0
rtk proxy env LZMA_API_STATIC=1 RUSTFLAGS='-C link-arg=-Wl,-force_load,/usr/local/opt/xz/lib/liblzma.a' CARGO_BUILD_JOBS=1 cargo test --locked --lib eggsact -- --test-threads=1
                                                                  pass — 21 passed, 4474 filtered
rtk proxy env LZMA_API_STATIC=1 RUSTFLAGS='-C link-arg=-Wl,-force_load,/usr/local/opt/xz/lib/liblzma.a' CARGO_BUILD_JOBS=1 cargo test --locked --test eggsact_adapter -- --test-threads=1
                                                                  pass — 17 passed
rtk proxy env LZMA_API_STATIC=1 RUSTFLAGS='-C link-arg=-Wl,-force_load,/usr/local/opt/xz/lib/liblzma.a' CARGO_BUILD_JOBS=1 cargo test --locked --test eggsact_deterministic_tools -- --test-threads=1
                                                                  pass — 26 passed
rtk proxy env LZMA_API_STATIC=1 RUSTFLAGS='-C link-arg=-Wl,-force_load,/usr/local/opt/xz/lib/liblzma.a' CARGO_BUILD_JOBS=1 cargo test --locked --test preflight_integration -- --test-threads=1
                                                                  pass — 72 passed
rtk proxy env LZMA_API_STATIC=1 RUSTFLAGS='-C link-arg=-Wl,-force_load,/usr/local/opt/xz/lib/liblzma.a' CARGO_BUILD_JOBS=1 cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
                                                                  pass
rtk proxy env LZMA_API_STATIC=1 RUSTFLAGS='-C link-arg=-Wl,-force_load,/usr/local/opt/xz/lib/liblzma.a' CARGO_BUILD_JOBS=1 scripts/verify.sh quick
                                                                  pass
```

Quick verification included the existing agent-schema, core-boundary,
sandbox-contract, execution-ownership, TUI-authority, formatting, and locked
workspace-check guards.

## 5. Invariant and security review

- Eggsact remains an in-process dependency behind `ToolRegistry`; no subprocess,
  MCP hop, network call, or new concurrency path was introduced.
- CodeGG still owns the model-facing palette and progressive-disclosure policy.
  The upstream utility/discovery surface remains intentionally outside the
  palette.
- Invalid profiles continue to fail visibly without substitution of
  `Profile::Default`; model and harness audience boundaries remain unchanged.
- The Clippy modernization is mechanical and semantics-preserving. No new
  execution authority, credential flow, persistence, or provider boundary was
  added.

## 6. Migration and recovery

Users building CodeGG must use Rust 1.89 or newer. Valid existing profile
configurations need no migration; unknown profiles continue to require
correction rather than receiving an implicit fallback. No storage or schema
migration is required.

The dependency adoption is reversible by reverting the implementation commit
and restoring the prior manifest/lockfile baseline. Reverting does not require
data repair. The historical M008 closure remains unchanged so its former MSRV
condition remains auditable.

## 7. Unresolved findings

| Severity | Finding | Disposition |
|---|---|---|
| — | No medium-or-higher M009 finding remains | M009 acceptance criterion satisfied |
| low / environmental | This macOS checkout needs the recorded static-xz linker workaround for x86_64 test links | Does not block repository closure; use a compatible host/toolchain or the documented workaround |

## 8. Roadmap and dependency disposition

M009 closes the deferred dependency baseline. The parent tool-surface
compatibility corrective roadmap is closed, with M008 retained as its historical
conditional closure and M009 recorded as the corrective MSRV/dependency pass.

The registry audit found no future plan whose complete dependency graph became
satisfied through this work. Architecture convergence M009 remains blocked on
compatible-host root-runtime/all-feature operational evidence, and Runtime
Safety C002 remains blocked on supported-Linux Landlock fixture evidence. This
M009 dependency change satisfies neither blocker, so neither future plan was
unblocked or reclassified.

No new dependency-ready plan was registered.

## 9. Control-surface updates

- M009 source plan is marked `implemented`.
- The parent roadmap is marked `closed` and links this closure record.
- `plans/registry.md` marks the subsystem and M009 closed, removes the active
  execution gate, and records the implementation and closure evidence.
- The historical M008 closure record was not rewritten.
