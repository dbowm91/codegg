# Plugin UI Final Polish Pass

## Objective

Close the last small correctness, ergonomics, and operational gaps after the corrective convergence pass. The plugin platform now has a coherent architecture: runtime abstraction, registry-backed management, hardened installer path handling, portable UI nodes, multi-frontend effect transport, and examples/SDKs.

This polish pass should avoid new architecture. It should make the current implementation reliable in normal user workflows and easier to verify in CI.

## Scope

This plan focuses on four remaining areas:

1. Ensure installed plugins carry source metadata so `/plugin-remove` can actually remove installed files when intended.
2. Decide and implement exact local install path semantics, especially absolute source paths.
3. Make validation externally visible through CI/status checks or a documented manual validation harness.
4. Tighten management UX wording and edge-case behavior around install/remove/runtime-only enable state.

## Non-Goals

- Do not add network marketplace installs.
- Do not add PyO3.
- Do not redesign the registry or runtime traits.
- Do not add persistence for enable/disable state unless the change is very small and clearly bounded.
- Do not add new plugin surfaces.
- Do not change the WASM ABI.

## Workstream A: Preserve Installed Plugin Source Metadata

### Problem

`PluginManager::uninstall()` unregisters the plugin and then attempts filesystem removal only if `PluginManagementView::source_path` is populated. If `PluginManagementView::from_info()` still sets `source_path: None`, a freshly installed plugin may be unregistered but not removed from disk.

### Target Behavior

A plugin installed through `/plugin-install` or `PluginManager::install_from_path()` should record enough source/install metadata to support:

- `/plugin-info` showing where it is installed;
- `/plugin-doctor` checking install path accessibility;
- `/plugin-remove` removing the installed directory safely;
- future persistence/loading work.

### Recommended Data Model

Prefer adding explicit metadata to `PluginInfo` rather than overloading manifest fields.

Suggested struct addition:

```rust
pub struct PluginInfo {
    pub id: String,
    pub manifest: PluginManifest,
    pub enabled: bool,
    pub trust: PluginTrustClass,
    pub diagnostics: Vec<PluginDiagnostic>,
    pub source: Option<PluginSourceMetadata>,
}

pub struct PluginSourceMetadata {
    pub install_path: Option<PathBuf>,
    pub original_source_path: Option<PathBuf>,
    pub installed_by: PluginInstallKind,
}

pub enum PluginInstallKind {
    Builtin,
    LocalPath,
    RegistryLoaded,
    Unknown,
}
```

If changing `PluginInfo` is too much churn, use a smaller field:

```rust
pub source_path: Option<PathBuf>
```

But avoid putting runtime-local paths into `PluginManifest`, because manifests should remain portable plugin declarations.

### Implementation Steps

1. Add source metadata to `PluginInfo` or an equivalent side table keyed by plugin id.
2. Update `PluginManagementView::from_info()` so `source_path` is populated from source metadata.
3. In `PluginManager::install_from_path()`:
   - after `install::install_from_path()` returns `dest`, store `dest` as install path in the `PluginInfo` registered into the registry;
   - optionally store the canonical original source path as original source metadata.
4. Builtins should set `installed_by = Builtin` or `source_path = None` and should never attempt filesystem deletion.
5. Registry-loaded plugins should populate install path if loading from the plugins directory.
6. Update `PluginManager::uninstall()`:
   - resolve plugin;
   - reject filesystem removal for builtins;
   - if install path exists, validate via `validate_uninstall_target()`;
   - remove directory;
   - unregister only after filesystem validation succeeds, or unregister first but preserve path metadata before unregistering.
7. Make removal ordering explicit:
   - preferred: validate target first, unregister second, delete third;
   - if delete fails, either re-register or report “unregistered but failed to remove files.”

### Tests

Add or update tests:

- installed plugin view contains `source_path` equal to installed destination.
- `/plugin-info` for installed plugin shows install path.
- `/plugin-doctor` install path check passes for installed plugin.
- `/plugin-remove` removes installed directory from disk.
- `/plugin-remove` refuses to remove builtin plugin files.
- `/plugin-remove` reports accurately if unregister succeeds but file removal fails.
- registry-only plugins without source metadata are unregistered but message says files were not removed.

### Acceptance Criteria

- Freshly installed plugins have source/install metadata.
- `/plugin-remove` can remove installed plugin files when safe.
- Builtins and registry-only plugins are not accidentally deleted from disk.
- User-facing messages distinguish unregister-only from uninstall-with-files.

## Workstream B: Decide Absolute Local Install Path Semantics

### Problem

`validate_install_source()` now rejects `RootDir` and `Prefix` when strict traversal policy is enabled. This is safe, but it may conflict with normal user behavior: `/plugin-install /Users/me/dev/my-plugin` or `/plugin-install /tmp/my-plugin` should likely be valid for a local install command.

Current `PluginManager::install_from_path()` calls `install_from_path()` directly, so this strict helper may not currently block absolute local installs. That ambiguity should be resolved.

### Recommended Policy

Distinguish user-supplied local source paths from archive member paths:

- **Local install source path**: may be absolute or relative, including `..`, if canonicalization resolves to an existing directory and the manifest validates.
- **Archive member path / copy relative path**: must be strictly relative and must reject parent traversal, root, prefix, symlink, and hardlink components.
- **Uninstall target/plugin name**: must be a plugin id/name, not a path; reject separators, root, prefix, and parent components.

This is ergonomic and safe because local source paths are explicitly chosen by the user, while archive entries and uninstall names are attacker-controlled or mutation-target paths.

### Implementation Steps

1. Split helpers:

```rust
pub fn validate_local_install_source(source: &Path, policy: &PluginInstallPolicy) -> Result<PathBuf, InstallError>;
fn validate_relative_install_path(rel: &Path) -> Result<(), String>;
fn validate_plugin_name_for_uninstall(name: &str) -> Result<(), InstallError>;
```

2. `validate_local_install_source()` should:
   - canonicalize the source;
   - require it exists;
   - require it is a directory for local path installs;
   - require `manifest.toml` exists;
   - optionally reject symlink source roots if desired;
   - not reject absolute paths or `..` if canonicalization succeeds.

3. `validate_relative_install_path()` remains strict and is used only for:
   - archive entry paths;
   - source-relative copy paths produced after `strip_prefix(src_root)`;
   - any future unpacked file member paths.

4. Update docs and comments to avoid implying absolute local source paths are traversal.
5. Add tests for absolute and relative source installs.

### Tests

- absolute local path install succeeds.
- relative local path install succeeds.
- local path with `..` succeeds if canonical target exists and has a valid manifest.
- archive `../escape` still fails.
- archive absolute path still fails.
- uninstall absolute path still fails.
- uninstall `../name` still fails.

### Acceptance Criteria

- Local path install semantics are explicit and documented.
- Absolute local source paths are either supported or intentionally rejected with a clear error; prefer support.
- Archive and uninstall path safety remains strict.

## Workstream C: Tighten Install/Remove UX Wording

### Problem

Current messages can imply that files were removed even when only unregister occurred or when no source path was available.

### Required Changes

1. Return a structured result from uninstall:

```rust
pub struct PluginUninstallResult {
    pub view: PluginManagementView,
    pub unregistered: bool,
    pub removed_files: bool,
    pub install_path: Option<PathBuf>,
    pub warning: Option<String>,
}
```

2. Update TUI completion variant if needed:

```rust
PluginRemoveFinished {
    plugin_id: String,
    removed_files: bool,
    warning: Option<String>,
    error: Option<String>,
}
```

3. User-facing success messages:

- files removed: `Plugin '<id>' unregistered and removed from <path>.`
- no source path: `Plugin '<id>' unregistered. No install path was recorded, so no files were removed.`
- builtin: `Builtin plugin '<id>' cannot be removed from disk; it was disabled/unregistered only if allowed.`
- delete failed: `Plugin '<id>' unregistered, but failed to remove files: <error>.`

4. `/plugin-install` should say whether plugin is enabled immediately and whether enable state is persistent.
5. `/plugins` note should remain: enable/disable is runtime-only until persistence lands.

### Tests

- remove installed plugin reports files removed.
- remove source-less plugin reports unregister-only.
- remove builtin reports non-removable or appropriate error.
- install message reports registered/enabled/runtime-only state.
- failed delete surfaces warning.

### Acceptance Criteria

- Management UX does not overclaim filesystem deletion.
- Install/remove messages correspond to actual behavior.
- Tests assert message variants or result flags.

## Workstream D: External Validation Signal

### Problem

Commit messages report robust validation, but GitHub commit statuses are empty. For handoff and regression prevention, the repo should expose at least one durable validation signal.

### Preferred Option: GitHub Actions CI

Add or update `.github/workflows/ci.yml`.

Recommended jobs:

#### Rust workspace

```yaml
cargo fmt --check
cargo check --workspace --all-features
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
cargo test -p codegg-protocol --all-features
cargo test --test tui_render
cargo test --test tui --all-features
```

#### Plugin focused

```yaml
cargo test -p codegg --lib plugin::install::
cargo test -p codegg --lib plugin::management::
cargo test -p codegg --lib tui::commands::plugin_management::
```

#### Examples/SDKs

```yaml
cargo test --manifest-path examples/plugins/sdk-rust/Cargo.toml
python3 -m unittest discover examples/plugins/sdk-python/tests
rustup target add wasm32-unknown-unknown
cargo check --manifest-path examples/plugins/wasm-command-table/Cargo.toml --target wasm32-unknown-unknown
cargo check --manifest-path examples/plugins/wasm-hook-message-transform/Cargo.toml --target wasm32-unknown-unknown
cargo check --manifest-path examples/plugins/wasm-status-widget/Cargo.toml --target wasm32-unknown-unknown
```

If total CI time is too high, split into:

- default PR CI: fmt, check, clippy, workspace tests;
- nightly/manual plugin examples workflow: examples and WASM checks.

### Alternative: Manual Validation Script

If GitHub Actions is not desired, add:

```text
scripts/validate_plugin_ui.sh
```

The script should run the same commands and fail fast. Document it in `AGENTS.md` and `docs/PLUGINS.md`.

### Acceptance Criteria

- Either GitHub Actions or a validation script exists.
- The validation command list is discoverable from docs.
- Future commits can produce visible status or reproducible validation output.

## Workstream E: Small Test and Doc Cleanup

### Tests to Add or Confirm

- `PluginManager::install_from_path()` populates source metadata.
- `PluginManager::uninstall()` removes installed directory and reports exact outcome.
- Absolute local install path behavior is covered.
- Relative local install path behavior is covered.
- Archive traversal and uninstall traversal regressions remain covered.
- `/plugins` shows builtins and runtime-only note.
- `/plugin-doctor` reports install path for installed plugin.
- `PluginManagementView` source path behavior is tested for builtin, installed, and registry-only plugins.

### Docs to Update

- `docs/PLUGINS.md`:
  - local install accepts absolute/relative paths, if implemented;
  - enable/disable runtime-only until persistence;
  - remove semantics: unregister-only vs filesystem removal.
- `architecture/plugin.md`:
  - plugin source metadata model;
  - management source of truth;
  - validation/CI workflow.
- `.opencode/skills/plugin/SKILL.md`:
  - current validation command;
  - do not reintroduce sidecar disabled state in TUI management.

## Implementation Order

1. Add source/install metadata to `PluginInfo` or an equivalent registry-backed source table.
2. Update install/uninstall manager behavior and result type.
3. Decide and implement local absolute-path policy.
4. Fix TUI install/remove messages and tests.
5. Add CI workflow or validation script.
6. Update docs.

## Final Definition of Done

This polish pass is complete when:

- Installed plugins have visible source/install metadata.
- `/plugin-remove` removes files only when safe and reports exactly what happened.
- Local install path semantics are explicit, tested, and documented.
- Builtins cannot be removed from disk.
- Source-less registry plugins do not produce misleading remove messages.
- A CI workflow or validation script exists for the plugin UI stack.
- Documentation and `.opencode` skills reflect current management and validation behavior.
