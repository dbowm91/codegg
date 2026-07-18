# Runtime Assets Milestone 004 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/runtime-assets/004-immutable-runtime-pinning-and-closure.md`

Source subsystem roadmap:

- `plans/subsystems/runtime-assets-roadmap.md#milestone-4--immutable-runtime-pinning-and-closure`

Repository baseline reviewed: `972c286` (Runtime Assets Milestone 003
closed)

Implementation commit:

- `2293a11` — `feat(runtime-assets): implement immutable pinning and resource bounds`

## 1. Executive finding

Milestone 004 is closed. Runtime asset identity is now captured at the
daemon turn boundary as an immutable snapshot plus a path-free runtime pin.
The pin retains the published generation and fingerprint while recording
bounded digests for skills actually activated during the turn. Refreshes
after capture publish new snapshots for later turns without changing the
active turn's behavior.

Local skill resources are represented by lazy, bounded handles. Each read
revalidates canonical containment, rejects traversal and symlink escape, and
enforces independent resource-size and returned-byte limits. Remote workspace
manifest types are bounded data-only DTOs with provenance, digest, and
compatibility diagnostics; they contain no local paths, commands, permissions,
or transport authority.

Run metadata gained an additive, path-free asset-provenance field. Existing
manifests remain readable, while the canonical Bash run path records the
captured pin when a run store is available. No watcher, transport, remote
execution, or in-flight snapshot mutation was introduced.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Turn/agent-run boundaries retain captured generation and fingerprint | `RuntimeAssetPin`, `CoreDaemon::TurnSubmit`, `TurnRunInput.asset_pin`, `AgentLoop::set_runtime_asset_pin`, `tests/asset_snapshot.rs`, `src/agent/asset_refresh.rs` pinning tests | pass | Later refreshes cannot replace the captured snapshot or identity. |
| Activated skill digests are bounded and auditable | `RuntimeAssetPin::record_skill_activation`, `SkillTool::with_snapshot`, `RunAssetProvenance`, run-store bound test | pass | Only skills present in the captured snapshot can be recorded; the persisted form is path-free. |
| Run metadata remains compatible and bounded | Optional `RunManifest.asset_provenance`/`RunDraft.asset_provenance`, `manifest_backward_compat_no_provenance_fields`, `asset_provenance_is_bounded_and_path_free` | pass | Missing legacy fields deserialize as `None`; bodies and paths are not stored. |
| Resource handles are lazy and contained | `src/skills/resource.rs`, `EffectiveSkill::resource_handle`, `AssetRegistry::resource_handle` | pass | Discovery inventories names/metadata without reading resource bodies. |
| Traversal, absolute paths, symlink escape, oversize, and malformed reads are rejected | `tests/skills_registry.rs` resource boundary, symlink, size, and malformed UTF-8 tests | pass | Reads re-resolve the canonical path and apply size/read limits. |
| Restart reconstructs equivalent asset identity | `restored_metadata_prevents_generation_reuse` and `PublishedAssetSnapshot::runtime_asset_pin` | pass | Explicit-context rebuilds use the restored generation/fingerprint metadata. |
| Remote manifests are inert bounded compatibility data | `crates/codegg-protocol/src/runtime_assets.rs`, six `runtime_assets` tests, 91-test protocol suite | pass | Validation sorts/deduplicates/caps fields and diagnostics; no execution authority is represented. |
| Discovery and activation remain non-executable | Existing registry tests plus `script_files_inventoried_not_executed`, discovery invariants, execution-ownership guard | pass | Skill activation renders bounded metadata/body only and never runs bundled scripts. |
| Architecture and operational documentation describe the final boundary | `architecture/agent.md`, `architecture/overview.md`, `architecture/skills.md`, `architecture/storage.md` | pass | Pinning, resource bounds, inert manifests, and rollback behavior are documented. |

## 3. Production implementation evidence

### Runtime pinning and audit

`CoreDaemon::TurnSubmit` obtains the published project snapshot and creates a
shared `RuntimeAssetPin`. `TurnRunInput`, `TurnHandle`, and `AgentLoop` retain
that pin for the turn/agent-run lifetime. `SkillTool` resolves names only from
the captured snapshot and records activation digests in the shared pin. The
canonical Bash run-store path converts the current pin into bounded
`RunAssetProvenance` immediately before persistence.

### Bounded resources

`ResourceHandle` stores a canonical package root and relative resource name,
performs metadata-only construction, and revalidates containment and file
limits on every read. `ResourceReadLimits` bounds both the source file and
returned bytes. The registry exposes handles only for inventoried resources,
so arbitrary paths cannot be requested through the skill surface.

### Remote compatibility seam

`codegg_protocol::runtime_assets` defines versioned manifest, identity, scope,
entry, provenance, and diagnostic DTOs. `validate_and_normalize` is the only
normalization boundary and enforces schema, field, asset-count, total-size,
diagnostic-count, and serialized-size limits. The module is data-only and does
not implement transport, synchronization, activation, or execution.

## 4. Verification executed

### Commands run

```bash
rtk cargo fmt --all -- --check
rtk cargo check --workspace --all-targets --all-features
rtk cargo clippy --workspace --all-targets --all-features -- -D warnings
rtk cargo test --test asset_snapshot
rtk cargo test --test skills_registry
rtk cargo test -p codegg-protocol runtime_assets
rtk cargo test -p codegg-core run_store
rtk env CARGO_BUILD_JOBS=1 cargo test --workspace --all-features -- --test-threads=14
rtk bash scripts/check-core-boundary.sh
rtk python3 scripts/check_daemon_cwd_usage.py
rtk python3 scripts/check_project_agent_pwd_inference.py
rtk python3 scripts/check_discovery_invariants.py
rtk python3 scripts/check_project_catalog_invariants.py
rtk python3 scripts/check_scheduler_bypass.py
rtk python3 scripts/check_execution_ownership.py
rtk python3 scripts/check_git_forbidden_patterns.py
rtk bash scripts/check_provider_connections_m4_coverage.sh
rtk bash scripts/check_provider_connections_tombstone_compat.sh
rtk python3 scripts/check_builtin_agents.py
rtk python3 scripts/generate_builtin_agents.py --check
rtk python3 scripts/check-tokio-test-flavors.py
rtk cargo fmt --all -- --check
rtk git diff --check
```

### Results

- Formatting, diff hygiene, and the all-features workspace check passed.
- Clippy passed with `-D warnings` after the final runtime-context wiring.
- Focused results: snapshot 8 passed; resource registry 24 passed; protocol
  runtime-asset tests 6 passed; run-store tests 18 passed.
- The capped all-features workspace suite completed with exit 0. The changed
  root unit suite reported 3,828 passed and 0 failed; all subsequent workspace
  unit, integration, protocol, native-crate, and doc-test suites also passed
  (with only the repository's pre-existing ignored tests).
- Core boundary, daemon-CWD, project-agent-PWD, discovery, project-catalog,
  scheduler-bypass, execution-ownership, Git-forbidden-pattern, provider
  lifecycle/tombstone, and diff guards passed.
- `check_builtin_agents.py` and `generate_builtin_agents.py --check` report
  one pre-existing `general` built-in prompt mismatch between TOML and the
  checked-in generated Rust.
- `check-tokio-test-flavors.py` reports 758 pre-existing bare
  `#[tokio::test]` annotations. The new asset test uses an explicit
  `current_thread` flavor and adds no finding.

## 5. Invariant review

- **Immutable in-flight behavior:** refresh swaps the published snapshot for
  future turns; the `Arc<ProjectAssetSnapshot>` and captured generation do
  not change for an active turn.
- **Explicit scope:** pin creation occurs from the daemon's published,
  project/workspace-scoped snapshot; no new path or process-CWD authority was
  added.
- **Deterministic identity:** generation and fingerprint come from the
  published snapshot, while skill digests are copied from the same snapshot
  and activation is limited to known effective skills.
- **Transactional refresh compatibility:** the prior refresh coordinator's
  invalid/cancelled/failed-candidate retention remains unchanged; pinning does
  not add a second publication path.
- **Bounded resource access:** handle construction and reads are metadata-first,
  canonicalized, size-limited, and UTF-8 checked.
- **Inert remote data:** remote DTO validation cannot create a local handle,
  grant a permission, or start a process.
- **No bundled execution:** discovery, rendering, and activation are read and
  formatting operations only.

## 6. Failure and recovery review

- **Refresh race:** a refresh after turn capture affects only subsequent
  publications; the active pin remains unchanged.
- **Restart:** restored generation/fingerprint metadata prevents generation
  reuse, while explicit-context reconstruction supplies the current body.
- **Resource TOCTOU:** every handle read re-resolves and rechecks the target,
  so a later symlink or replacement cannot bypass package-root containment or
  limits.
- **Malformed input:** invalid resource paths, non-files, oversized files,
  oversized reads, invalid UTF-8, malformed manifest identity, duplicate
  entries, and oversized manifest fields are rejected or diagnosed within
  bounded output.
- **Persistence failure:** run-store asset provenance is additive and
  best-effort like the existing run metadata; it cannot change command
  execution or grant authority.
- **Contention and release:** pin state is shared only for the bounded turn
  lifetime; the resource handle has no global lock or eager body cache.

## 7. Migration and compatibility review

- `RunManifest.asset_provenance` and `RunDraft.asset_provenance` are optional
  serde fields. Existing manifests and drafts without the field retain their
  prior behavior.
- Run provenance is bounded at the storage boundary and contains no local
  paths, bodies, commands, or permission records.
- The remote manifest DTOs use schema version 1 with safe defaults for missing
  optional fields, bounded normalization, and explicit compatibility
  diagnostics. No transport or protocol version bump was required.
- Existing legacy `SkillTool` construction remains available; production
  session construction uses the snapshot/pin path.
- No database migration is required for the additive JSON run-manifest field;
  rollback remains safe because older readers ignore the omitted field and
  newer readers default it when absent.

## 8. Security review

- Relative resource names reject absolute paths, parent traversal, root/prefix
  components, backslash separators, and symlink escapes outside the canonical
  package root.
- Resource and manifest bounds prevent unbounded local reads, returned output,
  diagnostic growth, asset counts, total asset size, and serialized payloads.
- Remote provenance is descriptive only; it cannot authorize tools, commands,
  permissions, local filesystem access, or remote execution.
- Runtime and run-store asset metadata is path-free. Skill bodies/resources are
  not copied into manifests or remote DTOs.
- Discovery/activation does not invoke scripts, shell commands, providers,
  plugins, LSP, or scheduler paths.
- Static path, execution-ownership, core-boundary, and Git secret-boundary
  guards passed. The two failing baseline hygiene guards are listed as low
  findings below and are outside this milestone's runtime-assets scope.

## 9. Documentation and operations

Updated:

- `architecture/agent.md` — runtime pin lifetime, audit identity, restart, and
  inert-manifest boundaries.
- `architecture/overview.md` — Runtime Assets M4 module map.
- `architecture/skills.md` — lazy handles, resource limits, and safe reads.
- `architecture/storage.md` — path-free run provenance.
- `plans/implementation/runtime-assets/004-immutable-runtime-pinning-and-closure.md`
  — implemented status.
- `plans/subsystems/runtime-assets-roadmap.md` — closed milestone and link.
- `plans/registry.md` — closed subsystem, recently closed row, and dependency
  review.

Operator/recovery behavior is unchanged for refresh failures: the last valid
published snapshot remains authoritative, while active turns retain their
captured pin. Resource and manifest rejection returns bounded diagnostics.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| low | `check_builtin_agents.py` and `generate_builtin_agents.py --check` report a pre-existing mismatch in the built-in `general` system prompt. | Agent-asset CI hygiene remains red independently of Runtime Assets M4; no runtime-asset behavior is affected. | Regenerate/reconcile built-in agent assets in a separate agent-assets maintenance change. |
| low | `check-tokio-test-flavors.py` reports 758 pre-existing bare Tokio test annotations. | The repository-wide flavor guard remains red independently; all new tests use explicit flavors. | Migrate or allowlist the existing test inventory in a separate Tokio test-hygiene change. |

No medium, high, or critical finding remains in this milestone's implemented
scope.

## 11. Roadmap disposition

The Runtime Assets subsystem roadmap is closed at Milestone 004. Project
Catalog Milestone 003 remains `ready` and can proceed using the already-closed
Runtime Assets Milestone 003 activation-refresh seam; its status was already
ready and requires no transition from this closure.

No other future plan became unblocked by this milestone. Multi-Project TUI
Milestone 001 and Session Projections Milestone 001 remain blocked on Project
Catalog Milestone 004 and the project-aware TUI foundation. No new Runtime
Assets follow-up plan is required; watchers, distributed manifests, and
transport remain explicitly deferred rather than correctness blockers.

## 12. Registry updates

- Mark the source implementation plan implemented and link this closure.
- Mark the Runtime Assets roadmap and Milestone 004 closed.
- Remove Runtime Assets 004 from dependency-ready plans and add it to
  recently closed work.
- Mark the Runtime Assets subsystem closed in the active registry.
- Retain Project Catalog 003 as ready; no newly unblocked plan needs a status
  update.
- Retain the existing Multi-Project TUI and Session Projections blocked rows.
