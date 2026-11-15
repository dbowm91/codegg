# Configuration

CodeGG configuration uses JSON with JSONC support, so `//` and `/* */`
comments are allowed. The repository-root `codegg.example.jsonc` file is a
fully commented starting point that you can edit down.

Check a config before relying on it:

```bash
codegg validate                        # auto-detected config
codegg validate --config ./my.jsonc    # a specific file
```

To change one in your editor instead:

```bash
codegg edit                            # $VISUAL/$EDITOR, creating it if missing
codegg edit --config ./my.jsonc        # a specific file
codegg edit --project                  # the project-local config
codegg edit --editor hx                # override $VISUAL/$EDITOR
```

`edit` resolves the editor from `--editor`, then `$VISUAL`, then `$EDITOR`,
then the first of `hx`, `vim`, `vi`, or `nano` on `PATH`. It re-checks the file
when the editor exits. See `docs/cli.md` for details.

`validate` exits 0 when the config is valid and exits 1 with an error message
otherwise. With `--config`, it names the file; without it, CodeGG validates the
auto-detected config.

Build jobs are limited to one per workspace by default. An optional
repository-wide limit is available through
`scheduler.quota.max_concurrent_builds_per_repository`. It applies only when a
workspace has a repository binding and does not classify arbitrary Bash, tests,
or other processes as builds. See `architecture/build_admission.md`.

## Where configuration is read from

All of these layers are loaded and **merged**, rather than using only the first
matching file. They are collected in this order, and a later layer overrides an
earlier one:

| Order | Location | Notes |
|---|---|---|
| 1 | `$CODEGG_TUI_CONFIG` | An explicit file path, if it exists. Merged first, so it is the **lowest** precedence. |
| 2 | System | macOS `/Library/Application Support/codegg/codegg.json`; other Unix `/etc/codegg/codegg.json`; Windows `%ProgramData%\codegg\codegg.json`. |
| 3 | Global | `<config_dir>/codegg/codegg.jsonc`, then `.json`, then `config.json`. Falls back to `<config_dir>/codegg/codegg.jsonc`. |
| 4 | Project | Walked upward from the working directory. Merged last, so it is the **highest** precedence. |

So effective precedence, highest first, is: **project → global → system →
`$CODEGG_TUI_CONFIG`**.

How the layers combine depends on the field (the full table is under
"Merge Strategies" in `architecture/config.md`): most sections combine
key-by-key, maps replace per key, and `instructions` concatenates. A section
whose fields are all non-`Option` (`security`, `provider_connections`,
`deterministic_tools`) is replaced wholesale rather than merged. Within a
key-by-key section, a nested block you specify replaces that whole block.

`<config_dir>` is the platform config directory. It is `~/.config` on Linux
and `~/Library/Application Support` on macOS.

Project discovery checks these four paths in each ancestor directory,
starting at the working directory and walking up:

```text
<dir>/.codegg/codegg.jsonc
<dir>/.codegg/codegg.json
<dir>/codegg/codegg.jsonc
<dir>/codegg/codegg.json
```

Because the walk goes upward, a config at the repository root applies to every
subdirectory beneath it.

## Three config mistakes to avoid

### 1. Unknown keys are ignored, not rejected

The top-level schema does not use `deny_unknown_fields` (the single use in
the schema is on a nested record, `ReasoningCapabilityRecord`). A misspelled
key parses cleanly and does nothing. If a setting appears to be ignored, check the
spelling against `architecture/config.md`.

### 2. Some settings accept only specific shapes

These accept specific shapes. A wrong value is a parse error and can prevent
the file from loading. Validate changes before relying on them.

| Key | Accepted shapes |
|---|---|
| `autoupdate` | `true` / `false`, or a string. **Not** an object. This setting is inert; run `codegg upgrade` explicitly. |
| `provider.<id>.timeout` | An integer number of **milliseconds**, or a boolean. `"60s"` is rejected. |
| `lsp` | `false`, or a map keyed by **server id**. |
| `formatter` | `false`, or a map keyed by **file extension**. |
| `plugin` | An array of path strings or `[path, options]` entries. |
| `permission.<tool>` | An action string (`"allow"` / `"ask"` / `"deny"`), or an object whose values are all strings. |

### 3. Some maps are keyed directly, with no wrapper object

A nested wrapper can silently register the wrong thing instead of failing:

- `mcp` servers are keyed directly. `"mcp": {"servers": {...}}` registers a
  server literally named `servers`.
- `lsp` is keyed directly by server id, such as `rust-analyzer` or `pyright`,
  not by language name.
- `formatter` is keyed directly by file extension. Do not add a `rules` wrapper.

## A minimal working config

```jsonc
{
  "model": "anthropic/claude-sonnet-4-20250514",
  "provider": {
    "anthropic": {
      "auth": { "type": "api_key", "env": "ANTHROPIC_API_KEY" }
    }
  }
}
```

That is enough to run a turn once `ANTHROPIC_API_KEY` is set. See
`docs/providers.md` for the credential options, including the encrypted
store.

## Windows model shell

On Windows, raw model shell commands use Windows PowerShell by default. To
select `cmd.exe` explicitly, add:

```jsonc
{
  "shell": { "windows_shell": "cmd" }
}
```

`"powershell"` is also accepted, along with `"git-bash"` and `"wsl"` when
those executors are installed. The setting is ignored on Unix and applies only
to raw shell text; structured executable-and-argument calls bypass shell
parsing. It does not enable Windows filesystem or network containment, so
constrained Windows requests remain unavailable until native enforcement is
qualified.

## Semantic model routing

Semantic routing is opt-in and only reachable through an exact `virtual:<name>`
alias configured under `model_routers`. A concrete model selection bypasses
routing entirely.

```jsonc
{
  "model_routers": {
    "virtual:code": {
      "selector_model": "openai/gpt-4o-mini",
      "default_model": "openai/gpt-4o-mini",
      "routes": {
        "fast":    { "model": "openai/gpt-4o-mini", "description": "short implementation task" },
        "deep":    { "model": "anthropic/claude-opus-4-20250514", "description": "complex reasoning" }
      }
    }
  }
}
```

Then select it with `codegg -m virtual:code` or `/model virtual:code`.

CodeGG chooses among configured routes for each turn. It keeps the provider
connection already selected for that turn and will not switch providers. If
selection fails or returns an invalid route, CodeGG uses the compatible
`default_model`. Routing does not pin a session to one route. See
`architecture/config.md` for the full contract.

## Environment variable interpolation

Any `${VAR}` sequence in a config file is replaced with that environment
variable's value before parsing, anywhere in the file:

```jsonc
{
  "provider": {
    "openai": {
      "base_url": "${OPENAI_BASE_URL}",
      "auth": { "type": "api_key", "value": "${OPENAI_API_KEY}" }
    }
  }
}
```

An unterminated `${` is emitted literally rather than swallowing the rest of
the file, and a variable that is not set expands to an empty string. Note that
the config file is read fresh on each load, so this is the supported way to
keep secrets out of the file itself. Prefer a provider's `auth.env` field,
which makes the lookup explicit and avoids writing the expanded secret to disk.


## What else configuration controls

`codegg.example.jsonc` annotates each block. The generated config catalog at
`architecture/config-catalog.md` lists every section and its merge behavior.
This page provides the narrative guide. The main groups are:

| Group | Key | Guide |
|---|---|---|
| Model selection | `model`, `small_model`, `model_routers`, `model_profile` | above, and `architecture/config.md` |
| Providers | `provider`, `disabled_providers`, `enabled_providers`, `provider_connections` | `docs/providers.md` |
| MCP servers | `mcp` | `docs/MCP.md` |
| Language servers | `lsp`, `experimental.lsp_tool` | `docs/LSP.md` |
| Permissions | `permission`, `approval_reviewer` | `docs/tui.md`, `architecture/permission.md` |
| Compaction | `compaction` | `architecture/compaction.md` |
| Skills | `skills` | `docs/agents-skills.md` |
| Custom commands | `commands` | `docs/agents-skills.md` |
| Tools | `tools`, `formatter` | `docs/tools.md` |
| Plugins | `plugin`, `keybinds`, `theme`, `watcher`, `instructions` | `docs/PLUGINS.md`, `docs/themes.md` |
| Daemon | `daemon` | `docs/daemon.md` |
| HTTP server | `server` | `architecture/server.md` |
| Remote execution | `eggwork` | `architecture/jobs.md`, `architecture/config.md` |

The `skills.paths` entries are used as skills directories directly, without
appending a vendor subdirectory. Set `skills.enabled` to `false` to disable
skill discovery. See `docs/agents-skills.md` and `architecture/skills.md`.

Remote execution settings are under `eggwork`. For node targeting, isolation,
network policy, and resource limits, see `architecture/jobs.md` and
`architecture/config.md`.

## Source verification

Checked 2026-10-23 against the source tree.

Rechecked 2026-11-15 against `crates/codegg-config/src/paths.rs`,
`crates/codegg-config/src/schema.rs`, `src/agent/semantic_router.rs`, and
`src/scheduler/config.rs`. Confirmed config precedence, accepted config shapes,
model-router provider behavior, skill path semantics, and scheduler quota names.

- Config layers and their order: `crates/codegg-config/src/paths.rs`
  (`CODEGG_TUI_CONFIG`, `system_config_path`, `global_config_path`,
  `find_project_config`).
- System paths: macOS `/Library/Application Support/codegg/codegg.json`, other
  Unix `/etc/codegg/codegg.json`, Windows `%ProgramData%\codegg\codegg.json`
  (`crates/codegg-config/src/paths.rs`).
- Global file candidates `codegg.jsonc`, `codegg.json`, `config.json`:
  `crates/codegg-config/src/paths.rs`.
- Project discovery (walk upward; `.codegg/codegg.jsonc`,
  `.codegg/codegg.json`, `codegg/codegg.jsonc`, `codegg/codegg.json`):
  `crates/codegg-config/src/paths.rs`.
- Untagged config types: `AutoupdateConfig`, `ProviderTimeout`,
  `PermissionRule`, `FormatterConfig`, `LspConfig`, `LspRule`, and `PluginSpec`
  in `crates/codegg-config/src/schema.rs`.
- The schema does not use `deny_unknown_fields`: no occurrence in
  `crates/codegg-config/src/schema.rs`.
- `shell.windows_shell` accepts `powershell`, `cmd`, `git-bash`, `wsl` and
  rejects `bash`: `crates/codegg-config/src/schema.rs` (`WindowsShellKind`).
- `model_routers` fields (`selector_model`, `default_model`, `routes`,
  `sticky`, `affinity_ttl_s`, `selector_timeout_s`, `max_input_bytes`,
  `repair_attempts`): `crates/codegg-config/src/schema.rs`.
- `skills.paths` / `skills.enabled`: `crates/codegg-config/src/schema.rs`
  (`SkillsConfig`).
- `eggwork` node fields (`node_id`, `endpoint`, `ca_cert_path`,
  `client_cert_path`, `client_key_path`, `required_capabilities`,
  `isolation_policy`, `network_policy`, `resource_limits`,
  `max_concurrent_executions`) and the daemon-level
  `allowed_target_nodes` / `required_isolation_policy` /
  `required_network_policy`: `crates/codegg-config/src/schema.rs`.
- Remote resource limit dimensions and the `required` / `best_effort`
  distinction: `crates/codegg-config/src/schema.rs`.
- `${VAR}` interpolation and the JSON/JSONC/TOML/YAML frontmatter handling:
  `crates/codegg-config/src/`.

- Corrected 2026-10-27: the top-level `Config` carries only
  `#[serde(default)]` (`crates/codegg-config/src/schema.rs:404-406`); the
  schema's single `deny_unknown_fields` is on the nested
  `ReasoningCapabilityRecord` (`:152`).
