# Runtime Safety, Resource Control, and Footprint Milestone 007 — Closure Status

Status: closed

Source implementation plan: `plans/implementation/runtime-safety-resource-footprint/007-binary-topology-and-footprint-reduction.md`

Source subsystem roadmap: `plans/subsystems/runtime-safety-resource-footprint-roadmap.md`

Repository baseline reviewed: `4d540ce315c9ef2a1c07544cd42df0efc43708e1`

Implementation commits:

- `d1cf4db` — reduce default dependency and feature footprint

## 1. Executive finding

M007 is implemented and strictly closed with a measured no-split decision.
The production change removes three unused direct dependencies and makes two
server-only dependencies feature-owned. It removes the unused `wasmtime-wasi`
graph, the stale Ratatui textarea/crossterm branch, and the unused
`serde_path_to_error` package without changing application code, user features,
daemon authority, protocol, or storage paths.

The pre-change and post-change default stripped release executables are both
54,463,680 bytes on the reviewed Darwin arm64 target. The accepted reductions
therefore reduce dependency/lockfile footprint but do not materially alter the
ordinary executable. A daemon/TUI split was not implemented: the measured
result cannot meet the plan's 5 MiB and 10 percent role-size threshold, and the
current default binary already excludes the large optional server, plugin, and
image graphs. No prototype scaffolding or follow-up split plan is warranted.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result |
|---|---|---|
| Verify prior dependency work | M002–M006 dispositions in the runtime-safety registry and roadmap; current lockfile and feature tree | pass |
| Remove only proven safe dependency/feature weight | Source search found no uses of `serde_path_to_error`, `ratatui-textarea`, or `wasmtime-wasi`; server-only `tokio-stream` and `socket2` are now enabled by `server` | pass |
| Preserve supported feature gates | `cargo check --workspace --all-targets --all-features --locked`; release builds for `server`, `plugins`, and `image` | pass |
| Record clean release measurements | Fresh isolated target with Rust 1.97.1, `aarch64-apple-darwin`, release LTO/strip/codegen-units=1, locked builds | pass |
| Identify dominant topology contributors | Feature-tree comparison and per-feature release measurements; optional server and plugin graphs add 3,622,416 and 7,997,504 bytes respectively, while default/no-default differ by 112 bytes | pass |
| Apply split materiality rule | Default before/after are equal; no measured role-specific reduction reaches 5 MiB and 10 percent; no split implemented | pass |
| Preserve compatibility and authority | No Rust source, protocol, storage, endpoint, CLI, service, or package invocation changes; only manifest/lockfile ownership changes | pass |
| Avoid forbidden automation and feature deletion | No size gate, workflow, artifact, release automation, UPX, or feature removal added | pass |

## 3. Production implementation evidence

- `serde_path_to_error` was a direct manifest dependency with no repository
  source use and was removed.
- `ratatui-textarea` was a direct manifest dependency with no repository source
  use. Removing it also removed its stale Ratatui 0.24/crossterm 0.27 branch.
- `wasmtime-wasi` was declared and enabled by `plugins`, but the plugin runtime
  uses the core `wasmtime` API and contains no `wasmtime-wasi` source use. The
  unused WASI graph was removed while WASM plugin support remains enabled.
- `tokio-stream` and `socket2` are used only by feature-gated server modules.
  Both are now optional and activated by the `server` feature.
- `Cargo.lock` records the resulting removal of the unused dependency graph;
  the default application feature set remains `arboard` plus the existing
  always-on product dependencies.

## 4. Verification executed

All commands below were run locally. Release measurements were made in fresh
isolated target directories and are not claims about hosted CI.

```text
cargo check --workspace --all-targets --locked                    pass
cargo check --workspace --all-targets --all-features --locked     pass
cargo build --release --locked                                    pass
cargo build --release --locked --no-default-features             pass
cargo build --release --locked --no-default-features --features server  pass
cargo build --release --locked --no-default-features --features plugins pass
cargo build --release --locked --no-default-features --features image    pass
cargo tree --no-default-features -e features --locked             pass
cargo tree --features server -e features --locked                 pass
```

Final fresh release measurements:

| Variant | Stripped `codegg` bytes |
|---|---:|
| baseline default, commit `687efcdf` | 54,463,680 |
| final default, `arboard` | 54,463,680 |
| final `--no-default-features` | 54,463,568 |
| final `server` | 58,086,096 |
| final `plugins` | 62,461,184 |
| final `image` | 54,480,112 |

The measurements used `rustc 1.97.1 (8bab26f4f 2026-07-14)`, host/target
`aarch64-apple-darwin`, `--release --locked`, and the repository profile
`lto = true`, `strip = true`, `codegen-units = 1`. The baseline was built in
an isolated worktree at `687efcdf`; final variants used one fresh isolated
target directory.

The normal hosted verification trigger was not available on the direct
planning branch; no hosted run is claimed here. The repository quick check is
run again after this closure documentation is staged.

## 5. Invariant review

- The single active daemon remains the owner of scheduling, durable execution,
  projects, sessions, and shared resources.
- No daemon/client protocol, endpoint discovery, state path, project identity,
  authentication, or CLI invocation changed.
- All documented features remain buildable, including server, plugins, image,
  and LSP test-support combinations covered by the all-feature check.
- Optional large features remain optional; `wasmtime` remains behind
  `plugins`, and server/image dependencies remain behind their feature gates.
- Business logic and composition boundaries were not duplicated or moved.
- The accepted release profile remains stripped and optimized.

## 6. Failure and recovery review

No execution, cancellation, restart, scheduler, storage, process, or recovery
code changed. Manifest ownership changes cannot create a second daemon or alter
runtime failure semantics. Cargo validation rejected any missing feature-owned
dependency before the change was committed.

## 7. Migration and compatibility review

No database, configuration, protocol, package, service, or invocation migration
is required. The package still exposes the same `codegg` executable and the
same feature names. The removed dependencies were unused direct declarations;
their transitive lockfile removal is not user data or runtime state migration.

## 8. Security review

No security boundary, sandbox, auth, process, plugin authority, or network
policy changed. Removing unused WASI support does not remove the existing WASM
plugin runtime; it removes only an unreferenced dependency graph. No critical,
high, or medium M007 finding remains.

## 9. Downstream unblock audit

M008 is the only registered downstream runtime-safety plan. M007 is now an
accepted disposition, so it is no longer a blocker. M008 remains `blocked`
because it still requires the supported-Linux Landlock result and reconciliation
of the conditional M001/M002/C001 records. No other registered plan names M007
as an unresolved dependency, so no additional plan became `ready` in this
closure.

## 10. Final disposition

M007 is closed with the existing single-binary topology retained. The measured
no-split result is a completed plan outcome, not an architecture deferral.
Future topology work should require a new concrete deployment constraint or a
material change in the measured dependency grouping.
