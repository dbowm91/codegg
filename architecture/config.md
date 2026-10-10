# Config Module

## Purpose

The config crate (`crates/codegg-config/`) handles configuration discovery,
loading, JSONC parsing, field-level merging, encryption, hot-reload via
file watching, and schema validation. It is the single source of truth for
all runtime configuration.

**Re-export**: `codegg::config` via `pub use codegg_config as config`
in `src/lib.rs`.

## Where It Lives

| Path | Role |
|------|------|
| `crates/codegg-config/src/schema.rs` | Config struct, all type definitions |
| `crates/codegg-config/src/paths.rs` | Discovery, loading, merging, env interpolation |
| `crates/codegg-config/src/watcher.rs` | Hot-reload file watching with debounce |
| `crates/codegg-config/src/encryption.rs` | Master key lookup, encrypt/decrypt provider keys |
| `crates/codegg-config/src/error.rs` | ConfigError, AppError types |

## How It Works

### Discovery Order (later overrides earlier)

1. `CODEGG_TUI_CONFIG` environment variable
2. System config (`/Library/Application Support/codegg/codegg.json` on macOS,
   `/etc/codegg/codegg.json` on Unix, `%ProgramData%/codegg/codegg.json` on Windows)
3. Global config (`~/.config/codegg/codegg.jsonc`, `codegg.json`, or `config.json`)
4. Project config (searches upward from `$PWD` for `.codegg/codegg.{jsonc,json}`
   or `codegg/codegg.{jsonc,json}`)

### Loading Flow

```
Config::load()
  1. resolve_config_paths()    -> collect config file paths
  2. load_config() per path    -> interpolate_env_vars(), then
                                 JSONC comment stripping + JSON5 parse
  3. merge_configs()           -> combine with per-field strategies
  4. migrate()                 -> "0" -> "1" version upgrade
  5. validate()                -> produce warnings (not errors)
```

There is deliberately **no decrypt step here.** The config crate owns
master-key *resolution* only; `provider.<id>` credential encryption lives in
the providers crate (`resolve_provider_credential`) and in the MCP auth store.
`encryption::encrypt_provider_keys` / `decrypt_provider_keys`
(`encryption.rs:544`/`:549`) are no-ops that exist only for call-site
compatibility.

### Merge Strategies (`paths.rs:164`, `merge_configs`)

Different strategies per field type:

- **Field-by-field**: `provider` (via `ProviderConfig::merge()`),
  `server` (via `ServerConfig::merge()`), `watcher`, `search`,
  `discovery`, `eggwork` (via `EggworkConfig::merge()` per node key,
  mirroring the `provider` map merge), and the ten optional sections
  `approval_reviewer`, `command_intent`, `daemon`, `human_shell`,
  `preflight`, `research`, `scheduler`, `shell`, `tool_backends`,
  `tool_deferral`, each via its own `merge(&mut self, other: &Self)` in
  `schema.rs`
- **Key replacement**: `agent`, `mcp`, `commands`, `mode`, `model_profile` (insert
  overwrites existing keys)
- **Key replacement**: `model_routers` (each virtual model key from a later
  config layer replaces the earlier policy)
- **Concatenation**: `instructions` (appended to list)
- **Whole-value replace**: `theme`, plus `provider_connections`, `security`,
  and `deterministic_tools`. Those three are replace-only because every one
  of their fields is non-`Option`, so serde bakes the default into the
  struct and field-by-field combination cannot distinguish "unset" from
  "set to the default". See "Merge coverage is enforced" below.
- **Simple override** (via `merge_option!`): `schema`, `version`, `log_level`,
  `model`, `small_model`, `medium_model`, `auto_route_models`, `default_agent`,
  `username`, `share`, `autoupdate`, `disabled_providers`,
  `enabled_providers`, `permission`, `compaction`, `subagent`, `skills`,
  `templates`, `layout`, `tools`, `formatter`, `lsp`, `lsp_semantic_cache`,
  `snapshot`, `snapshot_config`, `plugin`, `enterprise`, `experimental`,
  `keybinds`, `vim_mode`, `hooks`, `notifications`, `catalog`, `context`,
  `context_packer`, `context_policy`, `tool_advisor`, `decision_engine`,
  `orchestration`

Within a field-by-field section, top-level keys combine individually, but a
**nested block replaces wholesale** — specifying `[scheduler.queue]` in a
later layer overrides the whole `queue` block rather than merging into the
`queue` block from the layer beneath it.

#### Merge coverage is enforced

`merge_configs` is an explicit whitelist, and it is the **only** path from a
parsed file to `Config`: both `Config::load` (`schema.rs`) and
`ConfigWatcher::reload_config` (`watcher.rs:154`) call it. A field with no arm
is therefore dropped for **every** layer, not only for multi-layer loads — the
setting parses without error and then silently does nothing.

`scripts/check_config_merge_coverage.py` derives the `Config` field set from
the struct definition and the merged field set from `merge_configs`, and fails
when they differ. It embeds no field names, so adding a covered field passes
without editing it and adding an uncovered one fails. It runs in
`scripts/verify.sh quick` and in CI. Run it directly with `--verbose`; the
guard's own sensitivity is exercised by `--self-test`.

When adding a `Config` field, add the merge arm in the same change and extend a
merge assertion in `paths.rs`'s test module. The guard will catch an omission,
but the test is what pins the intended precedence.

**Previously broken.** Thirteen fields — `approval_reviewer`,
`command_intent`, `daemon`, `deterministic_tools`, `human_shell`, `preflight`,
`provider_connections`, `research`, `scheduler`, `security`, `shell`,
`tool_backends`, `tool_deferral` — had no arm at all and were dropped on every
load, so those config sections were entirely inert. The ten optional sections
got field-by-field `merge()` impls and the three default-backed sections got
whole-value replace. Regression coverage is
`test_merge_configs_preserves_single_layer_sections` (parses a single layer
containing all thirteen and asserts each survives), plus
`test_merge_configs_combines_optional_sections_field_by_field` and
`test_merge_configs_replaces_default_backed_sections_whole`.

### ProviderConfig Merge (`schema.rs:1106`)

`decision_engine` is a simple optional override. Its schema defaults to
disabled, uses `reference` or `ollama` as an explicit compatibility profile,
and carries only a `codegg_config::schema::AuthConfig` reference; runtime code
resolves credentials through the existing provider `AuthResolver`. The engine
does not perform model discovery unless an operator explicitly calls its
discovery method.

Learned tool-advisor inference is opt-in separately through
`tool_advisor.enabled` and its policy `mode`. Set
`tool_advisor.runtime_backend = "sdm_local_v1"` with a pinned local artifact to
use SDM Rank. Build the optional backend with `--features decision-runtime-sdm`.
Selecting `system_one` through `decision_engine` does not emulate Rank; the
current System One profile reports Rank unsupported and tool advice falls back
deterministically. The effective policy/backend split is available from
`codegg tool-advisor status`.

Example remote configuration:

```jsonc
{
  "decision_engine": {
    "enabled": true,
    "backend": "system_one",
    "profile": "reference",
    "base_url": "https://system-one.dev/v1",
    "model": "jev-latest",
    "timeout_ms": 2000,
    "discover_models": false,
    "auth": { "type": "api_key", "env": "SYSTEM_ONE_API_KEY" }
  }
}
```

For local Ollama, set `profile` to `ollama`, `base_url` to its loopback
`/v1` endpoint, and `model` to an installed System One model. The adapter
never starts Ollama or installs models.

Field-by-field: non-None fields from override replace base. Unlike
HashMap fields (key replacement), `ProviderConfig::merge()` merges
each optional field independently. If global has `api_key` and project
has `base_url`, merged result has both.

## Key Types & APIs

### Config (`schema.rs:218`)

```rust
pub struct Config {
    pub schema: Option<String>,
    pub version: Option<String>,
    pub log_level: Option<String>,
    pub model: Option<String>,
    pub small_model: Option<String>,
    pub medium_model: Option<String>,
    pub auto_route_models: Option<bool>,
    pub model_routers: Option<HashMap<String, ModelRouterConfig>>,
    pub default_agent: Option<String>,
    pub username: Option<String>,
    pub share: Option<String>,
    pub autoupdate: Option<AutoupdateConfig>,
    pub server: Option<ServerConfig>,
    pub provider: Option<HashMap<String, ProviderConfig>>,
    pub provider_connections: Option<ProviderConnectionsConfig>,
    pub disabled_providers: Option<Vec<String>>,
    pub enabled_providers: Option<Vec<String>>,
    pub agent: Option<HashMap<String, AgentConfig>>,
    pub mcp: Option<HashMap<String, McpEntry>>,
    pub permission: Option<PermissionConfig>,
    pub approval_reviewer: Option<ApprovalReviewerConfig>,
    pub compaction: Option<CompactionConfig>,
    pub subagent: Option<SubagentConfig>,
    pub skills: Option<SkillsConfig>,
    pub commands: Option<HashMap<String, CommandConfig>>,
    pub templates: Option<HashMap<String, SessionTemplate>>,
    pub instructions: Option<Vec<String>>,
    pub layout: Option<String>,
    pub tools: Option<HashMap<String, bool>>,
    pub formatter: Option<FormatterConfig>,
    pub lsp: Option<LspConfig>,
    pub lsp_semantic_cache: Option<LspSemanticCacheConfig>,
    pub watcher: Option<WatcherConfig>,
    pub snapshot: Option<bool>,
    pub snapshot_config: Option<SnapshotConfig>,
    pub plugin: Option<Vec<PluginSpec>>,
    pub enterprise: Option<EnterpriseConfig>,
    pub experimental: Option<ExperimentalConfig>,
    pub mode: Option<HashMap<String, ModeConfig>>,
    pub keybinds: Option<HashMap<String, String>>,
    pub vim_mode: Option<bool>,
    pub hooks: Option<Vec<HookConfigEntry>>,
    pub notifications: Option<NotificationConfig>,
    pub daemon: Option<DaemonConfig>,
    pub scheduler: Option<SchedulerConfig>,
    pub catalog: Option<CatalogConfig>,
    pub discovery: Option<DiscoveryConfig>,
    pub tool_deferral: Option<ToolDeferralConfig>,
    pub tool_advisor: Option<ToolAdvisorConfig>,
    pub decision_engine: Option<DecisionEngineConfig>,
    pub model_profile: Option<HashMap<String, ModelProfileConfig>>,
    pub security: Option<SecurityConfig>,
    pub research: Option<ResearchConfig>,
    pub theme: Option<ThemeConfig>,
    pub search: Option<SearchConfig>,
    pub tool_backends: Option<ToolBackendConfigSchema>,
    pub context: Option<ContextConfig>,
    pub context_packer: Option<ContextPackerConfig>,
    pub context_policy: Option<ContextPolicyConfig>,
    pub human_shell: Option<HumanShellConfig>,
    pub shell: Option<ShellConfig>,
    pub deterministic_tools: Option<DeterministicToolsConfig>,
    pub preflight: Option<PreflightConfig>,
    pub command_intent: Option<CommandIntentConfig>,
    pub orchestration: Option<OrchestrationConfig>,
    pub eggwork: Option<EggworkConfig>,
}
```

### ProviderConfig (`schema.rs:1068`)

```rust
pub struct ProviderConfig {
    pub api_key: Option<String>,
    pub encrypted_api_key: Option<String>,
    pub encrypted: Option<bool>,
    pub base_url: Option<String>,
    pub enterprise_url: Option<String>,
    pub set_cache_key: Option<bool>,
    pub timeout: Option<ProviderTimeout>,
    pub chunk_timeout: Option<u64>,
    pub whitelist: Option<Vec<String>>,
    pub blacklist: Option<Vec<String>>,
    pub models: Option<HashMap<String, ModelConfig>>,
    pub options: Option<HashMap<String, serde_json::Value>>,
    pub auth: Option<AuthConfig>,
    pub account_id: Option<String>,
}
```

`models` is **additive**. It augments whatever model discovery returned
for that provider; it never replaces it and never suppresses the
discovery attempt. Discovery is always attempted and is the canonical
way a model becomes usable. `models` exists for providers whose
endpoint does not serve a catalog, so an operator can drive one
manually. CodeGG ships no built-in catalogs — see "Model discovery
authority" in `architecture/provider.md`.

`api_key(&self, prefix)` checks `{PREFIX}_API_KEY` env var first, then
inline `api_key` field.

### AuthConfig (`schema.rs:16`)

```rust
pub enum AuthConfig {
    ApiKey { env, value, encrypted_value },
    Stored { account_id },
    ExternalCommand { command, args, timeout_ms },
    OAuthDevice { client_id, scopes, auth_url, token_url },
    None,
}
```

### ProviderConnectionsConfig (`schema.rs:385`)

Daemon-owned provider-connection refresh policy. Defaults:
`background_refresh=false`, `max_concurrent_refreshes=1`,
`global_refresh_cap=4`, `health_stale_after_ms=300000`.

### ServerConfig (`schema.rs:1020`)

```rust
pub struct ServerConfig {
    pub port: Option<u16>,
    pub hostname: Option<String>,
    pub token: Option<String>,
    pub mdns: Option<bool>,
    pub mdns_domain: Option<String>,
    pub cors: Option<Vec<String>>,
    pub cors_origins: Option<Vec<String>>,
    pub tool_timeout_seconds: Option<u64>,
    pub max_parallel_tools: Option<usize>,
}
```

Has its own `merge()` method for field-by-field merging.

### ConfigWatcher (`watcher.rs:12`)

```rust
pub struct ConfigWatcher {
    watcher: Option<RecommendedWatcher>,
    rx: mpsc::Receiver<()>,
    tx: mpsc::Sender<()>,
    watched_paths: Vec<PathBuf>,
    started: bool,
    debounce_duration: Duration,
    last_hash: Option<u64>,
    ignore_patterns: Vec<String>,
}
```

Key methods: `new()` (500ms default debounce),
`with_config(&WatcherConfig)` (configurable debounce + ignore patterns),
`start()` (watches config file parent dirs, non-recursive),
`recv()` (async, content-hash deduplication),
`reload_now()` (force immediate reload).

Uses `notify` crate. Content hash deduplication avoids spurious reloads.

### Encryption (`encryption.rs`)

```rust
pub fn get_master_key() -> Option<String>;
pub fn encrypt_provider_keys(config: &mut Config) -> Result<(), AppError>;
pub fn decrypt_provider_keys(config: &mut Config) -> Result<(), AppError>;
```

`encrypt_provider_keys` and `decrypt_provider_keys` are **no-ops** that
return `Ok(())` (`encryption.rs:544`/`:549`). They exist so call sites compile;
the config crate owns master-key resolution only. Provider credential
encryption is owned by the providers crate (`resolve_provider_credential`),
and the MCP OAuth token store owns its own `TokenSet` encryption.

Master key lookup order:
1. `CODEGG_MASTER_KEY`
2. `CODEGG_ENCRYPTION_KEY`
3. `OPENCODE_ENCRYPTION_KEY`
4. Existing CodeGG-managed key under the user config directory

Protected-store write paths use the corresponding create-on-write resolver:
if no explicit or managed key exists and the protected store is genuinely
fresh, CodeGG atomically bootstraps the managed key. Read/decrypt paths never
create one.

### ModelProfileConfig (`schema.rs:112`)

Per-model tuning: `prompt_profile`, `family`, `context_window`,
`max_output_tokens`, `tool_call_reliability`, `instruction_adherence`,
`patch_reliability`, `supports_late_system_messages`,
`prefers_user_control_messages`, `prefers_small_patches`,
`requires_explicit_tool_contract`, `requires_post_tool_continue_nudge`,
`text_tool_repair`, `default_reasoning_effort`,
`default_thinking_budget`, `max_parallel_tools`, `preferred_tools`,
`disabled_tools`, `task_state_policy`.

### ContextPolicyConfig (`schema.rs:679`)

Gated active context policy. First use: tool-palette reduction driven
by effective-cost diagnostics. Disabled by default. Modes: `Observe`,
`Warn`, `ToolPaletteReduce`. Includes volatile-tail compaction fields.

### SearchConfig (`schema.rs:774`)

Web search/fetch backend: `backend` (Eggsearch/Builtin/Disabled),
`expose_raw_mcp_tools`, `fallback_to_builtin`, output caps per domain,
`eggsearch` sub-config (command, args, timeouts, env vars).

## Configuration Surface

### Environment Variables

| Variable | Description |
|----------|-------------|
| `CODEGG_TUI_CONFIG` | Custom config file path |
| `CODEGG_MASTER_KEY` | Master key for encryption |
| `CODEGG_ENCRYPTION_KEY` | Fallback encryption key |
| `OPENCODE_ENCRYPTION_KEY` | Legacy encryption key |
| `CODEGG_TOKEN_KEY` | Deprecated MCP OAuth v1 migration-read key; never used for new writes |
| `{PROVIDER}_API_KEY` | Provider API key fallback |

### Key Config Sections

- `model` / `small_model` / `medium_model` — model selection
  (format: `provider/model`)
- `provider.<id>` — per-provider config (api_key, base_url, auth, etc.)
- `disabled_providers` / `enabled_providers` — provider allow/deny list
- `server` — HTTP server settings (port, hostname, token, CORS)
- `agent` — agent definitions (model, prompt, permissions, etc.)
- `mcp` — MCP server entries
- `permission` — permission rules per tool
- `approval_reviewer` — M006 Automatic reviewer preferences (optional
  `model`, investigation/deadline/output/headless/backstop bounds; absent
  model means Automatic defers — see `approval_reviewer.md`)
- `compaction` — context compaction settings (M004: explicit
  `mode=programmatic|agent|hybrid` honored; `auto=true` + omitted mode uses
  the resolved Hybrid default — deterministic without a provider/model, so
  no silent billable call; `auto=false` keeps DropMiddle compat)
- `subagent` — delegation bounds (max_concurrent, max_depth, etc.)
- `search` — web search/fetch backend config
- `lsp` / `lsp_semantic_cache` — LSP integration
- `watcher` — file watching config
- `plugin` — plugin specifications
- `experimental` — experimental feature flags
- `mode` — named mode configurations
- `hooks` — event hooks (shell commands)
- `context` / `context_packer` / `context_policy` — context management
- `human_shell` / `shell` — human shell feature config
- `deterministic_tools` / `preflight` — eggsact-backed tools
- `command_intent` — command classification and routing
- `daemon` / `scheduler` — daemon and scheduler settings
- `discovery` — project discovery configuration
- `model_profile` — per-model tuning profiles
- `orchestration` — opt-in bounded convergence defaults and aggregate deadline
- `tool_backends` — per-domain tool backend selection
- `eggwork` — named Eggwork nodes for fixed-target remote execution
  (`nodes: { name: { node_id?, endpoint, ca_cert_path, client_cert_path,
  client_key_path, required_capabilities?, isolation_policy?,
  network_policy? } }`); isolation policy is `none` (default) or `required`,
  network policy is `unrestricted` (default) or `disabled`; merged per node key;
  endpoint must be bare HTTPS, key paths must be absolute, private-key
  reference redacted from `Debug`. `required` needs the named node to advertise
  `isolation.landlock.workspace-rw.v1` in both fresh authenticated capability
  and status views on each attempt. `disabled` is fail-closed before upload:
  Eggwork currently has no qualified network-isolation backend, so unrestricted
  networking must be assumed for the supported mode.

`orchestration.auto_convergence` defaults to `false`. The host clamps
`default_max_cycles` to 1–4, `max_producers_per_cycle` to 1–3, and
`max_wall_clock_ms` to at most 24 hours. Explicit convergence calls remain
available when automatic guidance is disabled.

## Validation

Validation produces **warnings**, not errors — the app starts with a
partially invalid config.

Validated fields:
- `log_level`: `debug|info|warn|error|trace`
- `share`: `manual|auto|disabled`
- `model`/`small_model`/`medium_model`: must be `provider/model` format
- `port`: >= 1024
- Agent `mode`: `subagent|primary|all`
- Agent `color`: hex color or theme name
- MCP types: `local` requires `command`, `remote` requires `url`
- `tool_timeout_seconds`: 1-3600
- `max_parallel_tools`: 1-100
- `compaction.threshold`: 0.1-1.0
- `compaction.max_tokens`: >= 1000
- `deterministic_tools.backend`: `native` or `disabled`
- `deterministic_tools.profile`: a non-empty upstream eggsact profile name;
  the linked in-process runtime is authoritative and reports accepted names
  for invalid values
- `preflight.mode`: `off`, `observe`, `warn`, `block_on_definite`

## Invariants & Gotchas

- **Merge is per-type**: HashMap fields use key replacement (later wins);
  `ProviderConfig`/`ServerConfig`/`WatcherConfig` use field-by-field;
  `instructions` concatenates. New fields need a new arm — see
  "Merge coverage is enforced" above.
- **Nested blocks replace wholesale**: within a field-by-field section, a
  nested block in a later layer overrides that whole block rather than
  merging into the layer beneath it.
- **No decrypt step on load or reload**: `ConfigWatcher::reload_config()`
  calls `decrypt_provider_keys()` (`watcher.rs:167`), but that function is a
  no-op. Hot reload never decrypts; credential resolution happens at the
  providers crate / MCP auth boundary.
- **Project config searches upward**: From `$PWD`, checks `.codegg/` and
  `codegg/` directories with both `.jsonc` and `.json` extensions.
- **AuthConfig::None**: Explicit "no auth" marker — all credential
  lookups are skipped.
- **ProviderConfig merge**: `auth` field merges like any other optional;
  a project config setting `auth: { type: "stored" }` overrides the
  global `api_key` path.
- **No decryption without a resolvable master key**:
  `get_master_key()` never creates a key; reads fail closed when no explicit
  env key or managed key resolves. Only the protected-store *write* paths
  (`get_or_create_master_key`) may bootstrap one, and only for genuinely
  fresh stores.

## Testing

```bash
cargo test -p codegg-config                 # all config tests
cargo test -p codegg-config -- merge        # merge strategy tests
cargo test -p codegg-config -- watcher      # watcher tests
cargo test -p codegg-config -- validation   # validation tests
```

## Related Docs

- `architecture/provider.md` — provider config and credential resolution
- `architecture/crypto.md` — AES-256-GCM encryption details
- `architecture/search_backend.md` — search backend dispatch
- `architecture/lsp.md` — LSP semantic cache config
- `architecture/agent.md` — agent config usage

## Source Verification

Verified 2026-10-06 against `crates/codegg-config/{schema.rs,paths.rs,watcher.rs}`.
- Corrected 10 stale `file.rs:line` refs. `SearchConfig` had drifted the
  furthest (`461`→`739`), followed by `ContextPolicyConfig` (`366`→`644`),
  `ServerConfig` (`741`→`985`), `ProviderConfig` (`789`→`1033`), and
  `ProviderConnectionsConfig` (`339`→`350`). `ProviderConfig::merge` is at
  `schema.rs:1106`, not `827`.
- Confirmed correct as written: `merge_configs` (`paths.rs:164`) and
  `ConfigWatcher` (`watcher.rs:12`).

Verified 2026-10-06 against source after upstream `2573f9c0` ("Decision runtime
ownership migration and M006 closure"), which added 30 lines near the top of
`crates/codegg-config/src/schema.rs`. Corrected: `ProviderConfig::merge`
`schema.rs:1071`->`1106` in both the section heading and the note above,
re-checked to land on the method; the audit record that it is at `1106` rather
than `827` is retained. The `decision_engine` override described above was added
by that same commit and is confirmed accurate: default-disabled, explicit
`reference`/`ollama` profile, `AuthConfig` reference only, and no model discovery
without an explicit operator call.

Second pass (2026-10-06), verified against
`crates/codegg-config/src/{paths,schema,watcher,encryption}.rs` plus a
throwaway crate built against `codegg-config`:
- **Merge-strategy list was wrong.** It claimed `daemon`, `scheduler`,
  `tool_deferral`, `security`, `research`, `theme`, `tool_backends`,
  `human_shell`, `shell`, `deterministic_tools`, `preflight`, `command_intent`
  merge via `merge_option!`. They do not — `merge_option!` takes a fixed
  identifier list (`paths.rs:167-207`) and the only other arms are
  `model_routers`, `discovery`, `search`, `server`, `watcher`, `provider`,
  `eggwork`, `agent`, `model_profile`, `mcp`, `commands`, `instructions`,
  `mode`, `theme`. The 13 fields absent from that set are dropped on every
  multi-layer load; documented under a new "Fields `merge_configs` does NOT
  merge" section with the empirical `load_config` vs `merge_configs`
  comparison that proves it. `theme` is now correctly described as whole-value
  replace.
- **Loading flow was wrong.** It listed `decrypt_provider_keys()` as step 5 and
  omitted `migrate()`. `Config::load()` (`schema.rs:2447`) has an explicit
  comment that decryption is a delegated no-op; `decrypt_provider_keys` and
  `encrypt_provider_keys` are themselves no-ops (`encryption.rs:544`/`:549`).
  Rewrote the flow and the encryption/invariants sections.
- **Stale line refs re-corrected** after the same upstream insertion:
  `ProviderConfig` `1033`→`1068`, `ProviderConnectionsConfig` `350`→`385`,
  `ServerConfig` `985`→`1020`, `ContextPolicyConfig` `644`→`679`,
  `SearchConfig` `739`→`774`.
- **`Config` struct listing was incomplete**: omitted `approval_reviewer`,
  `tool_advisor`, `decision_engine`, `orchestration`, `eggwork`. All five type
  names verified against `schema.rs`.
- Confirmed accurate as written: `merge_configs` (`paths.rs:164`),
  `ConfigWatcher` (`watcher.rs:12`), `Config::migrate` (`schema.rs:2739`),
  `CONFIG_VERSION` (`schema.rs:5`), `AuthConfig` (`schema.rs:16`),
  `ModelProfileConfig` (`schema.rs:112`), `ProviderConfig::merge`
  (`schema.rs:1106`), the master-key resolution chain, and every listed
  validation bound (`log_level`, `share`, model format, `port >= 1024`,
  agent `mode`/`color`, `tool_timeout_seconds` 1-3600,
  `max_parallel_tools` 1-100, `compaction.threshold` 0.1-1.0,
  `compaction.max_tokens >= 1000`).

Third pass (2026-10-06) — **the 13-field merge gap recorded above is fixed.**
- **Re-scoped the blast radius.** The second pass called it a "multi-layer
  load" defect. That understated it: `merge_configs` is the only path from a
  parsed file to `Config` (`Config::load` and `ConfigWatcher::reload_config`
  at `watcher.rs:154` both call it), so the 13 sections were dropped even for a
  **single** config file. Confirmed by running the pre-fix `merge_configs`
  against `merge_configs(&[parsed])` for one parsed layer — every one of the
  13 came back `None`.
- **Classified by field shape before choosing a strategy**, rather than
  applying one rule to all 13. The ten all-`Option` sections
  (`approval_reviewer`, `command_intent`, `daemon`, `human_shell`,
  `preflight`, `research`, `scheduler`, `shell`, `tool_backends`,
  `tool_deferral`) can express "unset", so they got a field-by-field
  `merge(&mut self, other: &Self)` each. The three whose fields are **all
  non-`Option`** (`provider_connections`, `security`,
  `deterministic_tools`) cannot — serde bakes the default into the struct, so
  a `bool`/`usize` cannot be distinguished from "explicitly set to the
  default" — and use whole-value replace, which is already the documented
  behaviour of `theme`.
- **Nested blocks replace wholesale** even inside a field-by-field section;
  documented as an explicit rule rather than left implicit.
- **Added `scripts/check_config_merge_coverage.py`** so the class of defect
  cannot recur. It derives both sides (struct fields vs. `merge_configs`
  reads) and embeds no field names, so a future covered field passes without
  editing it and a future uncovered one fails. Verified it reports exactly the
  historical 13 against the pre-fix `paths.rs` and 0 after. Wired into
  `scripts/verify.sh quick` and CI, bringing the guard count to 41.
- **Regression coverage** proves the defect rather than the fix only: the
  three new tests in `paths.rs` were confirmed to fail against the pre-fix
  function and pass after.
