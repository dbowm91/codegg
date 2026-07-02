# Plugin UI Corrective Convergence Pass

## Objective

Close the remaining convergence and safety gaps after the Phase 11–15 plugin UI implementation sweep.

The current repo has the major pieces in place: plugin management models, policy evaluation, examples/SDKs, generic `UiNode` rendering, remote protocol support, and multi-frontend UI limits. The remaining risk is that some user-facing paths are not yet using the canonical service/registry/policy layers, and a few filesystem install helpers still have path-validation bugs inherited from earlier installer code.

This corrective pass should make the implementation internally consistent before additional feature work.

## Primary Findings to Address

1. **TUI plugin management is split from the live registry.**
   - `src/plugin/management.rs` provides `PluginManager` over `PluginService`/`PluginRegistry`.
   - `src/tui/commands/plugin_management.rs` uses `MarketplaceService::list_local_plugins()` and a separate `disabled_plugins.toml` sidecar state.
   - This can make `/plugins`, `/plugin-info`, `/plugin-enable`, `/plugin-disable`, and `/plugin-doctor` disagree with the live runtime/capability registry.

2. **Installer path validation is still incorrect in places.**
   - `copy_dir_all()` validates `src_canonical.starts_with(dst_canonical)`, which is backwards for normal source-to-destination copying.
   - Archive extraction canonicalizes destination paths before entries exist.
   - `validate_install_source()` canonicalizes before checking parent components, making the `ParentDir` check mostly ineffective.

3. **Install/remove policy helpers exist but are not consistently enforced by command paths.**
   - `validate_uninstall_target()`, `validate_wasm_module_path()`, and `validate_install_source()` exist.
   - Command paths should call these before mutating filesystem or registry state.

4. **Multi-frontend/protocol changes need verification coverage rather than just docs.**
   - Commit messages claim tests pass, but no GitHub status checks are attached.
   - Local validation should explicitly include workspace tests, plugin feature tests, TUI tests, examples/SDK tests, and protocol serde/compatibility tests.

## Non-Goals

- Do not add new plugin features.
- Do not add marketplace/network install.
- Do not add PyO3.
- Do not redesign the plugin ABI.
- Do not rewrite the plugin registry.
- Do not introduce a new UI schema.
- Do not make process plugins sandboxed.

## Workstream A: Unify TUI Plugin Management Around `PluginManager`

### Problem

`src/tui/commands/plugin_management.rs` currently drives user-facing management commands from `MarketplaceService` and a local TOML disabled set. This creates a parallel state model next to the live `PluginRegistry` and the canonical `PluginManager`.

### Target Architecture

The TUI command handlers should be thin presentation/async-dispatch wrappers around `PluginManager`.

Preferred flow:

```text
/tui slash command
  -> src/tui/commands/plugin_management.rs
  -> PluginManager over App/Core plugin service
  -> PluginRegistry / PluginService / install policy
  -> management_ui.rs builds UiNode
  -> UiNodeRenderer / UiNodeDialog displays result
```

### Required Changes

#### 1. Add an app/core accessor for plugin management

If `App` has access to `PluginService`, expose it safely:

```rust
impl App {
    pub(crate) fn plugin_manager(&self) -> Option<PluginManager> { ... }
}
```

If the TUI only has remote core access, route management through `CoreRequest`/`CoreResponse` instead of direct registry access. Do not retain the marketplace sidecar path as the primary source of truth.

#### 2. Update `src/tui/commands/plugin_management.rs`

Replace primary use of:

```rust
MarketplaceService::new().list_local_plugins()
load_disabled_set()
save_disabled_set()
```

with:

```rust
PluginManager::list()
PluginManager::info()
PluginManager::enable()
PluginManager::disable()
PluginManager::doctor()
PluginManager::remove()
```

Keep `MarketplaceService` only for filesystem discovery/install staging if still needed. It should not be the authoritative state for `/plugins` once a plugin registry exists.

#### 3. Remove or demote sidecar disabled state

The `disabled_plugins.toml` sidecar has two acceptable futures:

- remove it and rely entirely on registry/config persistence; or
- treat it as a persistence backend loaded into `PluginRegistry` at startup and written by `PluginRegistry::set_enabled()` or `PluginManager`.

Do not have TUI commands write sidecar state without updating the live registry.

#### 4. Make `/plugins` show live registered plugins

`/plugins` should report:

- builtins;
- installed process/WASM plugins loaded into the registry;
- enabled state from registry;
- runtime kind from manifest;
- trust class;
- capabilities from manifest/registry indexes;
- diagnostics from registry/service.

If a filesystem plugin exists but failed to load into the registry, show it under `/plugin-doctor` or a separate “discovered but not loaded” section, not as a normal enabled plugin.

#### 5. Make enable/disable use registry semantics

`/plugin-enable` and `/plugin-disable` should call `PluginManager::enable()` / `disable()` and surface `PluginRegistryError` messages, especially duplicate command/panel/status conflicts.

Persist only through the canonical persistence layer. If persistence is not done yet, show a clear note: “enabled state is runtime-only until persistence lands.”

#### 6. Make `/plugin-doctor` use the service-backed doctor

The TUI doctor command should call `PluginManager::doctor()` so it can report policy and live registry consistency. Avoid duplicating doctor checks in TUI.

### Tests

Add TUI management tests:

- `/plugins` includes builtin registered plugins.
- `/plugins` uses registry enabled state.
- `/plugin-disable <id>` updates registry state.
- `/plugin-enable <id>` rejects duplicate command conflicts from registry.
- `/plugin-info <id>` shows runtime/trust/capabilities from `PluginManagementView`.
- `/plugin-doctor <id>` includes policy diagnostics from `PluginManager`.
- filesystem-discovered but unregistered plugin is not shown as active/loaded.

Add service tests if missing:

- `PluginManager::list()` returns builtin/process/WASM plugins.
- `PluginManager::enable()` and `disable()` round-trip through `PluginRegistry`.
- `PluginManager::doctor()` reports process lifecycle hooks denied by default.

### Acceptance Criteria

- TUI management commands use `PluginManager` as the authoritative path.
- No user-facing management command relies only on `MarketplaceService` plus sidecar state for live plugin status.
- Registry and TUI state cannot disagree after enable/disable.
- Doctor output reflects actual registry/service/policy state.

## Workstream B: Fix Installer Path Validation and Copy Semantics

### Problem

`install.rs` still contains dangerous or broken path checks:

- `copy_dir_all()` checks that source files start with the destination directory, which is backwards and can reject valid installs.
- `extract_plugin_archive()` canonicalizes destination entry paths before unpacking, which fails for new files and can mask traversal handling.
- `validate_install_source()` canonicalizes before checking for `ParentDir`, so the traversal check does not inspect the original user-supplied path.

### Target Semantics

Installer safety should be based on destination containment and archive entry normalization.

Rules:

- never follow symlinks from plugin source or archive;
- never write outside the canonical plugin install directory;
- never allow archive entries with absolute paths, parent traversal, Windows prefixes, or symlinks/hardlinks;
- destination path validation should be lexical before write and canonical after parent creation where needed;
- file removal should validate target containment under plugin install dir.

### Required Changes

#### 1. Fix `copy_dir_all()`

Use destination containment, not source containment.

Recommended pattern:

```rust
fn copy_dir_all(src: &Path, dst: &Path) -> std::io::Result<()> {
    let src = src.canonicalize()?;
    std::fs::create_dir_all(dst)?;
    let dst_root = dst.canonicalize()?;

    fn copy_inner(src_root: &Path, current_src: &Path, dst_root: &Path) -> std::io::Result<()> {
        for entry in std::fs::read_dir(current_src)? {
            let entry = entry?;
            let ty = entry.file_type()?;
            if ty.is_symlink() {
                return Err(std::io::Error::other("symlinks are not allowed"));
            }

            let src_path = entry.path();
            let rel = src_path.strip_prefix(src_root)
                .map_err(|_| std::io::Error::other("source escaped root"))?;
            let dst_path = dst_root.join(rel);

            validate_relative_install_path(rel)?;
            ensure_destination_under_root(&dst_path, dst_root)?;

            if ty.is_dir() {
                std::fs::create_dir_all(&dst_path)?;
                copy_inner(src_root, &src_path, dst_root)?;
            } else if ty.is_file() {
                if let Some(parent) = dst_path.parent() {
                    std::fs::create_dir_all(parent)?;
                }
                std::fs::copy(&src_path, &dst_path)?;
            }
        }
        Ok(())
    }

    copy_inner(&src, &src, &dst_root)
}
```

Do not canonicalize the destination file before it exists.

#### 2. Add relative path validation helper

```rust
fn validate_relative_install_path(rel: &Path) -> Result<(), InstallErrorOrIo> {
    for component in rel.components() {
        match component {
            Component::Normal(_) => {}
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => reject,
        }
    }
}
```

Use for copied directories and archive entries.

#### 3. Fix `extract_plugin_archive()`

Use archive entry path components before unpacking:

```rust
let entry_path = entry.path()?;
validate_relative_install_path(&entry_path)?;
if entry.header().entry_type().is_symlink() || entry.header().entry_type().is_hard_link() { reject }
let dst_path = dest_root.join(&entry_path);
ensure_destination_under_root_lexical(&dst_path, &dest_root)?;
entry.unpack(&dst_path)?;
```

Create parent directories before unpacking. Do not call `canonicalize(&dst_path)` before it exists.

#### 4. Fix `validate_install_source()`

Validate the original path components before canonicalizing if the goal is to reject parent traversal in user input. Then canonicalize to confirm the path exists.

However, parent components in a local source path are not inherently unsafe if the final canonical path is a legitimate user-provided directory. Decide policy explicitly:

- if strict mode: reject any `..` in the submitted source path before canonicalization;
- if normal mode: allow `..` but require canonical path exists and manifest is valid.

Document the chosen behavior and test it.

#### 5. Harden `uninstall()`

Before `remove_dir_all`, call `validate_uninstall_target(&plugin_path, policy)` with default `PluginInstallPolicy` or service policy.

Reject plugin names containing separators, parent components, absolute paths, or Windows prefixes.

### Tests

Add installer tests:

- valid directory with manifest and normal file installs successfully.
- nested directories install successfully.
- source symlink rejected.
- archive symlink rejected.
- archive hardlink rejected.
- archive `../escape` rejected.
- archive absolute path rejected.
- archive Windows prefix path rejected where representable.
- copy does not reject normal source files because they are outside destination root.
- uninstall rejects `../../../etc/passwd` before touching filesystem.
- uninstall rejects absolute path.
- uninstall accepts plugin id under plugin dir.
- `validate_wasm_module_path()` rejects module outside plugin install dir.

### Acceptance Criteria

- Valid local plugin install works.
- Directory copy validation checks destination containment, not source containment.
- Archive extraction rejects traversal without pre-canonicalizing nonexistent target paths.
- Uninstall validates target containment before deletion.
- Install/remove tests cover traversal, symlink, hardlink, nested valid paths, and normal valid install.

## Workstream C: Enforce Install/Remove Policy in Command Paths

### Problem

Policy helpers exist, but command paths may not consistently call them.

### Required Changes

1. `PluginManager::remove()` should either:
   - only unregister, with no filesystem deletion; or
   - accept a policy and perform safe filesystem removal through `install.rs`.

   Be explicit in naming:

   - `unregister()` for registry-only removal;
   - `uninstall()` for filesystem deletion.

2. TUI `/plugin-remove` should clarify behavior:

   - if registry-only: “unregistered plugin; files left in place.”
   - if filesystem deletion: show exact path and require it to be under plugins dir.

3. TUI `/plugin-install` should call `validate_install_source()` with policy before install.

4. URL install should be disabled in management UX unless hardened archive/fetch validation is complete. If kept, it must be explicit and should not run from `/plugin-install` by default.

5. Doctor should report:

   - install path outside expected dir;
   - WASM module outside plugin dir;
   - process plugin executable missing;
   - policy warnings for high-risk permissions.

### Tests

- `/plugin-remove` cannot delete outside plugin dir.
- `/plugin-remove` behavior matches its message: unregister-only vs filesystem uninstall.
- `/plugin-install` rejects traversal source in strict mode.
- `/plugin-install` rejects invalid manifest before copying.
- `/plugin-install` installs disabled by default if no confirmation UX exists.
- URL install path is rejected or explicitly unsupported from TUI.

### Acceptance Criteria

- Install/remove command paths enforce `PluginInstallPolicy`.
- Command names/messages make registry-only vs filesystem deletion clear.
- No TUI command can delete arbitrary paths.
- No default TUI command performs network install.

## Workstream D: Align Management UI With Portable `UiNode`

### Problem

TUI management handlers convert `UiNode` into lines early. That is acceptable for existing dialogs, but it should not prevent future frontend reuse.

### Required Changes

1. Keep `src/plugin/management_ui.rs` as canonical conversion from management data to `UiNode`.
2. TUI may lower `UiNode` to lines only at the final render boundary.
3. If completion variants currently carry `Vec<String>`, consider adding `UiNode` variants or a generic `OpenUiNodeDialog` command for first-party management views.
4. Avoid duplicate formatting helpers in `plugin_management.rs` that diverge from `management_ui.rs`.

### Tests

- `plugins_table()` returns stable table with runtime/trust/enabled/capabilities.
- `plugin_info_node()` includes permissions and diagnostics.
- `doctor_report_node()` marks failed checks visibly.
- TUI rendering uses `UiNodeRenderer::node_to_lines()` at the final boundary.

### Acceptance Criteria

- Management UI data flows through `management_ui.rs` builders.
- Duplicate old marketplace formatting helpers are removed or used only in tests for legacy compatibility.
- Future remote/GUI clients can reuse management UI nodes.

## Workstream E: Verification and CI Signal

### Required Local Validation

Run and record exact results for:

```bash
cargo fmt --check
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo test --features plugins
cargo check --features plugins
cargo test -p codegg-protocol
cargo test --test tui_render
```

If any command is not valid for the repo, document the exact replacement.

### Examples/SDK Validation

Run or document:

```bash
python3 -m pytest examples/plugins/sdk-python/tests
python3 examples/plugins/process-quota-json/scripts/quota_json.py < examples/plugins/process-quota-json/sample_invocation.json
cargo test --manifest-path examples/plugins/sdk-rust/Cargo.toml
cargo check --manifest-path examples/plugins/wasm-command-table/Cargo.toml --target wasm32-unknown-unknown
cargo check --manifest-path examples/plugins/wasm-hook-message-transform/Cargo.toml --target wasm32-unknown-unknown
cargo check --manifest-path examples/plugins/wasm-status-widget/Cargo.toml --target wasm32-unknown-unknown
```

If `wasm32-unknown-unknown` is not installed in the environment, document that clearly and ensure docs instruct users to run:

```bash
rustup target add wasm32-unknown-unknown
```

### CI/Status Follow-Up

If GitHub Actions are available, add or update CI to include:

- workspace test;
- clippy;
- feature-gated plugin check/test;
- codegg-protocol tests;
- Python SDK tests if Python is available;
- example Rust SDK test;
- optional WASM example checks.

If CI is intentionally absent, add a `plans/` or docs note listing manual validation commands.

### Acceptance Criteria

- Validation commands are run and documented in the final implementation commit message or plan closure note.
- No unverified “tests pass” claim without exact command list.
- Examples are either in CI or have explicit manual validation instructions.

## Implementation Order

1. Fix installer path validation first; it is the most correctness-sensitive.
2. Unify TUI plugin management with `PluginManager`/registry.
3. Wire install/remove policy into TUI command paths.
4. Clean management UI line/node boundary.
5. Run validation and update docs.

## Final Definition of Done

This corrective convergence pass is complete when:

- `/plugins` and related management commands reflect live registry/service state.
- Enable/disable cannot diverge between TUI sidecar state and registry state.
- `PluginManager` is the canonical management API used by TUI or remote management paths.
- Local plugin install works for normal plugin directories and rejects traversal/symlink/hardlink cases.
- Archive extraction validates paths before write and does not canonicalize nonexistent destination files.
- `/plugin-remove` cannot delete outside Codegg’s plugin install directory.
- Management UI remains portable through `UiNode` builders.
- Exact validation commands and outcomes are recorded.
