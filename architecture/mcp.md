# MCP Module

Model Context Protocol (MCP) client implementation for connecting to
external tool servers — local (stdio) and remote (HTTP).

## Purpose

The MCP module provides a JSON-RPC 2.0 client that discovers, connects
to, and calls tools on external MCP servers. It is the transport layer
that powers both general-purpose MCP integrations (file system, git,
database servers) and Codegg-managed backends like `eggsearch`.

## Where It Lives

```
src/mcp/
├── mod.rs          # McpService, McpTool, McpClientType, McpExposurePolicy
├── local.rs        # JSON-RPC over stdio (child process)
├── remote.rs       # JSON-RPC over HTTP + optional SSE, McpConnectionManager
├── auth.rs         # OAuthManager, token encryption, PKCE, callback server
├── cli.rs          # CLI subcommands: add, list, remove, enable, debug
├── ide_server.rs   # IDE integration MCP server (openDiff tool)
└── protocol.rs     # pub(crate) shared protocol negotiation + bounded metadata
```

`protocol.rs` is a `pub(crate)` internal module (declared at `mod.rs:12`),
not part of the public surface. It owns `MODERN_PROTOCOL_VERSION`
(`2026-07-28`), `LEGACY_PROTOCOL_VERSION` (`2024-11-05`),
`SERVER_DISCOVER_METHOD` (`server/discover`), and the description/schema/
metadata byte ceilings (`MAX_MCP_DESCRIPTION_BYTES` 16 KiB,
`MAX_MCP_SCHEMA_BYTES` 256 KiB, `MAX_MCP_METADATA_BYTES` 64 KiB).

`McpError` is **not** defined in `src/mcp/` — there is no `src/mcp/error.rs`.
It lives in `crates/codegg-core/src/error.rs`.

## How It Works

### Connection Lifecycle

1. **Startup**: `bootstrap::bootstrap_eggsearch` creates an `McpService`,
   calls `connect_stdio` (local) or `connect_http` (remote) for each
   configured server, and shares the daemon-owned handle explicitly:
   production turn/session construction receives it inside
   `SearchRuntimeContext` via `bootstrap_search_runtime` (M005). The
   legacy process-global slot (`search_backend::state`) is retained
   only for cross-entry-point connection reuse, not for execution-time
   lookup.

2. **Protocol negotiation and tool discovery**: After spawning/opening a
   server, clients share one policy: they probe modern servers with
   `server/discover` using `2026-07-28` per-request metadata, then select the
   advertised modern revision. An explicit legacy protocol/method error falls
   back to the `initialize` handshake using `2024-11-05`. Once connected,
   `tools/list` collects bounded `McpTool` definitions, including optional
   output schemas, annotations, and server metadata.

3. **Tool calls**: `McpService::call_tool_structured()` looks up the
   target server, acquires a write lock on the client, and dispatches
   `tools/call`. The response is returned as `McpToolCallResult` with
   both text and optional structured JSON.

4. **Shutdown**: `McpService::shutdown_all()` disconnects every server
   in order. Local clients kill the child process; remote clients send
   `notifications/cancelled` and cancel heartbeat tasks.

### Client Types

```rust
pub enum McpClientType {
    Local(Arc<RwLock<LocalClient>>),
    Remote(Arc<RwLock<McpConnectionManager>>),
    Mock(Arc<Mutex<Box<dyn Fn(&str, Value) -> Result<String, McpError> + Send + Sync>>>),
}
```

`Mock` is a test-only variant. Production never constructs it; the
`register_mock_server` helper is the single entry point for integration
tests.

### Local Client (`local.rs`)

Spawns a child process, communicates via JSON-RPC over stdin/stdout.

- **PATH preservation**: `env_clear()` then re-injects the user's
  `PATH` so the child inherits the real environment.
- **Spawn timeout**: Process spawn wrapped in `min(timeout, 10s)`
  timeout to prevent hangs on slow `npx`/`uvx` cold starts.
- **Read loop**: A background task reads stdout lines, correlates
  JSON-RPC responses by `id` via a `PendingSenders` map, and delivers
  results through `oneshot::Sender`. On EOF, all pending senders
  receive `McpError::Connection("MCP server connection closed")`.
- **Stderr drain**: A separate task continuously drains stderr to
  prevent pipe deadlock when the child writes diagnostic output.
- **Graceful shutdown**: Sends `notifications/cancelled`, then
  `shutdown_notify.notify_waiters()` to break the read loop, then
  `child.kill()` + `child.wait()`. `Drop` calls `start_kill()` as a
  safety net.
- **Server version**: Extracted from modern discovery metadata when present,
  or from the legacy `initialize` response.
- **Legacy compatibility**: Only explicit unsupported-method/parameter
  protocol errors from the modern probe trigger the legacy retry. Connection,
  authentication, malformed-response, and timeout failures retain their
  original classification.

### Remote Client (`remote.rs`)

Sends JSON-RPC over HTTP POST. Supports server-initiated SSE for
responses that arrive as `text/event-stream`.

- **DNS rebinding protection**: Validates host IP on `new()` and
  before every `post_json()` call via `revalidate_dns()`. The accepted IP
  snapshot is converted to port-qualified socket addresses and attached to
  the actual Eggfetch request, so connection-time DNS cannot escape the
  revalidated destination. Internal IPs (loopback, private, link-local,
  CGNAT) are blocked.
- **Protocol modes**: Modern requests carry `MCP-Protocol-Version`,
  `Mcp-Method`, and (for tool calls) `Mcp-Name` headers plus `_meta` in the
  JSON-RPC params. Legacy requests retain `Mcp-Session-Id` handling from
  `initialize`.
- **Session management**: Stores `Mcp-Session-Id` from legacy `initialize`
  responses; modern requests are stateless and do not send that header.
- **OAuth**: Bearer token injected from `OAuthManager` when available.
- **Redirect policy**: Eggfetch redirects are explicitly disabled — redirects
  are not followed to prevent SSRF via redirect chains.
- **SSE response parsing**: When `post_json` receives a body starting
  with `event:`, it parses the SSE data lines as a JSON-RPC response.
  The `connect_sse_stream` method for persistent SSE subscriptions
  exists but is `#[allow(dead_code)]` and not called in production.

### McpConnectionManager (`remote.rs`)

Wraps `RemoteClient` with auto-reconnect and heartbeat:

- **Exponential backoff**: 1s → 2s → 4s → ... → max 60s
- **Max retries**: 5 attempts before giving up
- **Heartbeat**: Every 30s sends a `ping` notification. On failure,
  triggers reconnect.
- **`ensure_connected()`**: If disconnected, spawns reconnect in a
  background task, waits up to 30s for success, then falls back to a
  synchronous `connect()`.

### McpService (`mod.rs`)

Central registry holding `HashMap<String, McpServer>` and an
`OAuthManager`. Key methods:

| Method | Description |
|--------|-------------|
| `connect_stdio` | Spawn local server via stdio |
| `connect_stdio_host_scoped` | Spawn a local server with an authenticated host-local scope |
| `connect_http` | Connect to remote server via HTTP |
| `connect_from_config` | Dispatch canonical `local`/`remote` config types to stdio/HTTP |
| `disconnect` | Gracefully disconnect one server |
| `shutdown_all` | Disconnect all servers |
| `call_tool` | String-only tool call |
| `call_tool_structured` | Tool call retaining `structuredContent` |
| `call_tool_structured_host_scoped` | Tool call carrying a host-local scope |
| `list_tools` | All MCP tools as `ToolDefinition` |
| `list_filtered_tools` | Tools filtered through `McpExposurePolicy` |
| `handle_tool_list_changed` | Re-discover tools for a server |
| `list_prompts` / `get_prompt` | Prompt management |
| `list_resources` / `read_resource` | Resource management |

Resource and prompt projections

The turn-scoped `mcp_resource_search` and `mcp_resource_read` wrappers project
only bounded descriptors and exact, previously admitted resource handles. The
wrappers use the resolved `McpService` view, retain configured/plugin origin
provenance, and never accept an arbitrary server/URI pair. Text is truncated
at the host byte ceiling; blob content is returned as metadata rather than
unbounded base64.

The ceilings live in `src/mcp/mod.rs` and are the only knob the wrappers
read:

| Constant | Value | Bounds |
|----------|-------|--------|
| `MAX_RESOURCE_RESULTS` | 10 | `mcp_resource_search` `limit` maximum |
| `MAX_RESOURCE_TEXT_BYTES` | 64 KiB | `mcp_resource_read` `max_chars` maximum |
| `MAX_RESOURCE_ENTRIES` | 8 | content entries returned per read |
| `MAX_RESOURCE_DESCRIPTOR_BYTES` | 2 KiB | descriptor truncation in the local/remote clients |

`mcp_resource_read` accepts only a handle it minted earlier in the turn
(`mcpres:v1:<server>:<sha256-hex>`); a handle that is not admitted is
refused rather than resolved against the live catalog.

Prompts are different from resources: `McpService::invoke_user_selected_prompt`
is an explicit host/user activation seam. It validates the prompt descriptor
and required arguments before calling `prompts/get`. Returned role labels remain
untrusted source metadata and must be inserted through the ordinary next-turn
context compiler; they never become provider system or assistant authority.
There is no model-facing prompt invocation tool.

### McpExposurePolicy (`mod.rs`)

Controls which raw `mcp__<server>__<tool>` definitions reach the model:

```rust
pub struct McpExposurePolicy {
    pub show_raw: bool,           // default false
    pub hidden_servers: Vec<String>, // servers to hide even when show_raw=true
}
```

When `show_raw` is `false` (the default), `list_filtered_tools` returns
an empty vec. This is how eggsearch's raw MCP tools are kept hidden
from the model — the native wrappers (`websearch`, `webfetch`) own that
surface.

### Structured Call Path

`McpToolCallResult` carries both the text projection and an optional
`structured` JSON value, plus bounded `resultType`/`_meta` and explicit
tool-level `isError`. The structured value is extracted
from either:
1. `result.structuredContent` (protocol-level), or
2. `content[type=json].json` (content-level)

The eggsearch adapter uses `call_tool_structured` to retain machine-
readable evidence before display caps are applied.

### Eggsearch 0.4.4 compatibility policy

The stable eggsearch facade is audited against eggsearch `0.4.4`. **Two
different commits of 0.4.4 are pinned in the tree**, and they do not agree:

| Pin site | Constant | Commit |
|---|---|---|
| `scripts/test-eggsearch-pair.sh:6` | `EXPECTED_COMMIT` | `13fcac0b7ac0125d38e4e9775ebe1d62028fda72` |
| `src/install.rs:24` | `PINNED_EGGSEARCH_COMMIT` | `32f48bb57a43274d763c7f9ba919eab84d125edc` |

Both pins agree on the **version** (`0.4.4` — `EXPECTED_VERSION` in the
script, `PINNED_EGGSEARCH_VERSION` in `install.rs`), and `install.rs` is the
installation-owned authority: it is what the prebuilt release bundle is
built from and what `check_eggsearch_version_output` enforces at startup.
The pair gate builds from its own commit. Treat the commit disagreement as
an open divergence, not as a single pin; the version is the stable fact.

CodeGG exposes the provider-neutral additive controls that fit its ownership
boundary: search `excerpt_count` (`0..=3`), web-fetch `focus` and
its bounded extraction controls, and fetch `cache_policy` plus the
tightening-only `max_cache_age_seconds` (including per-web-item batch
controls). CodeGG always requests `response_detail=diagnostic` internally so
the structured result retains warnings, provider/retrieval state, trust
markers, stable IDs, suggested fetches, and next actions; its existing
bounded/trust-framed text remains the model-facing projection. `response_detail`
is not a user-facing option, and next actions are data only—CodeGG never
auto-executes them. Retained provider hints map to the exact v0.4.4 inventory;
unknown and retired explicit values fail before dispatch. The pinned CodeGG
development contract is v0.4.4. Its pair gate launches one real pinned MCP
process with an explicit private `search.mode = "off"` config, scoped local
root, and cleared credentials. Native web and security wrappers assert tagged
policy errors; native `repo_search(include_local=true)` proves private local
retrieval, `local_workspace` provenance, and zero remote provider attempts even
with an explicit remote provider hint. The gate does not enforce OS-level
network containment. Historical v0.4.1/v0.4.2 local-search behavior remains
documented in C001's immutable closure and is not used for current
qualification.

#### Host-scoped local retrieval

CodeGG can start a **managed** local eggsearch child with an authenticated
host-local scope (`connect_stdio_host_scoped`). The scope is carried as a
`codegg/host-local-scope` metadata object (root + token) on the tool call,
and the service keeps the per-server token in `McpService::host_scope_tokens`.

Two rules bound it:

- The child must advertise a version in `>=0.4.4, <0.5.0`
  (`supports_host_scoped_eggsearch`); otherwise the call is refused with
  "Eggsearch does not advertise the supported host-scoped retrieval protocol
  (0.4.4 through 0.4.x)".
- Only `repo_search` / `repo_fetch` / `repo_map` are host-scope capable, and
  only a local stdio child can carry a scope — a remote client, or a mock
  client, is refused. A raw MCP call to those tools still receives the
  **authenticated disabled scope** rather than the child's configured roots,
  so the managed child never inherits its own configured roots.

### IDE Server (`ide_server.rs`)

An MCP *server* (not client) exposing the `openDiff` tool for IDE
integration. Supports stdio transport. The `handle_connection` method
for Unix socket mode exists but is `#[allow(dead_code)]`.

### OAuth (`auth.rs`)

`OAuthManager` manages per-server token storage with encryption:

- **Current encryption**: New token-store writes use the canonical
  `codegg_config::encryption::get_master_key()` lookup and
  `codegg_providers::crypto` Argon2id/AES-256-GCM encryption. The MCP
  envelope is explicitly prefixed with `CODEGG_MCP_ENC_v2:` and contains
  the canonical `v2:` ciphertext.
- **Legacy migration**: `CODEGG_ENC_v1` stores remain readable only through
  the named legacy v1 decrypt path when `CODEGG_TOKEN_KEY` is available.
  When a canonical master key is available, startup writes a 0600 temporary
  file, decrypts and compares it semantically, then atomically replaces the
  legacy file. Missing keys, write/read-back/replace failures, corrupt data,
  and unknown or plaintext formats fail closed without overwriting the
  source. With only the legacy key, tokens remain available in memory but
  mutations require a canonical master key and do not produce new v1 data.
- **Token storage**: `~/.config/codegg/mcp_tokens.json` is encrypted and
  `~/.config/codegg/mcp_used_codes.json` is owner-only. New used-code entries
  store SHA-256 digests of authorization codes in a versioned JSON envelope;
  legacy raw-key entries are digest-migrated while preserving expiry and
  replay equality.
- **Key configuration**: `CODEGG_MASTER_KEY` is preferred, followed by the
  existing canonical compatibility aliases `CODEGG_ENCRYPTION_KEY` and
  `OPENCODE_ENCRYPTION_KEY` — the chain is `codegg_config::encryption::ENV_CHAIN`.
  When no environment variable is set, the canonical resolver falls back to
  the managed key file (`CODEGG_MASTER_KEY_FILE`, else
  `<config_dir>/codegg/master.key`). `CODEGG_TOKEN_KEY` is deprecated and is
  retained only to read and migrate historical MCP token stores.
- **PKCE**: S256 challenge, callback server on `127.0.0.1:0`.
- **Replay protection**: Used authorization codes tracked with expiry.
- **Redirect validation**: Must be HTTPS or `localhost`/`127.0.0.1`.

### CLI (`cli.rs`)

```
codegg mcp add <NAME> --server-type local|remote [--command <CMD>] [--args <ARG>...] [--url <URL>]
codegg mcp list
codegg mcp remove <NAME>
codegg mcp enable <NAME> [--enabled]
codegg mcp debug [<NAME>] [--url <URL>]
```

The CLI flag is `--server-type` (clap's kebab-case of the `server_type`
field); the **config** key it writes is still `"type"`. `debug`'s server
name is a **positional** argument, not `--name`.

The `debug` command tests connection to a named server from config or
an arbitrary URL. For remote servers, it connects, discovers tools,
and reports the count.

## Key Types & APIs

| Type | File:Line | Purpose |
|------|-----------|---------|
| `McpService` | `mod.rs:165` | Server registry + OAuth manager |
| `McpServer` | `mod.rs:115` | Per-server state (name, status, tools, version, client) |
| `McpClientType` | `mod.rs:147` | Local / Remote / Mock dispatch |
| `McpExposurePolicy` | `mod.rs:192` | Controls raw MCP tool visibility |
| `McpTool` | `mod.rs:78` | Tool definition from server |
| `McpToolCallResult` | `mod.rs:98` | Text + optional structured JSON |
| `McpServerStatus` | `mod.rs:107` | `Disconnected` (default) / `Connecting` / `Connected` / `Error(String)` |
| `McpPrompt` | `mod.rs:27` | Prompt definition |
| `McpResource` | `mod.rs:57` | Resource definition |
| `McpResourceContent` | `mod.rs:65` | Resource content (text or blob) |
| `LocalClient` | `local.rs:58` | JSON-RPC over stdio child process |
| `RemoteClient` | `remote.rs:378` | JSON-RPC over HTTP with SSE parsing |
| `McpConnectionManager` | `remote.rs:32` | Auto-reconnect + heartbeat wrapper |
| `ConnectionState` | `remote.rs:23` | Connected / Disconnected / Reconnecting |
| `OAuthManager` | `auth.rs:154` | Token lifecycle, encryption, PKCE |
| `TokenSet` | `auth.rs:80` | Access + refresh tokens with expiry |
| `McpCli` | `cli.rs:17` | CLI command handler |
| `McpCommand` | `cli.rs:185` | Clap subcommand enum |
| `IdeServer` | `ide_server.rs:51` | IDE MCP server (openDiff) |
| `McpError` | `crates/codegg-core/src/error.rs:234` | Connection, Server, ToolCall, OAuth, Encryption, Timeout |
| `McpServerOrigin` | `mod.rs:130` | `Configured` / `Plugin { plugin_id, plugin_version }` ownership |
| `McpPluginReconcileReport` | `mod.rs:139` | connected / removed / collisions / failed from plugin reconcile |
| `NegotiatedProtocol` | `protocol.rs:20` | Modern vs legacy negotiated protocol revision |
| `DiscoveryInfo` | `protocol.rs:46` | Bounded `server/discover` metadata |
| `parse_mcp_tool_server` | `mod.rs:233` | Extract server from `mcp__<server>__<tool>` |

## Configuration Surface

MCP servers are configured under `[mcp]` in `config.json`, keyed **directly by
server name**. `Config.mcp` is `Option<HashMap<String, McpEntry>>`
(`crates/codegg-config/src/schema.rs:245`) — there is **no `servers` level**. A
nested `"mcp": {"servers": {...}}` does not register its children at all:
`servers` is read as one entry named `servers`, and because `McpEntry`/
`McpServerConfig` are `#[serde(default)]` without `deny_unknown_fields`, it
silently deserializes to an all-`None` entry (a phantom server with no
`command`/`url`) instead of failing.

```json
{
  "mcp": {
    "filesystem": {
      "enabled": true,
      "type": "local",
      "command": "npx",
      "args": ["-y", "@modelcontextprotocol/server-filesystem", "/home/user/projects"],
      "env": { "PATH": "${PATH}" },
      "timeout": 30000
    },
    "github": {
      "enabled": true,
      "type": "remote",
      "url": "https://api.github.com/mcp",
      "headers": { "Authorization": "Bearer ${GITHUB_TOKEN}" },
      "timeout": 60000,
      "reconnect": {
        "enabled": true,
        "max_retries": 5,
        "base_delay_secs": 1,
        "max_delay_secs": 60,
        "heartbeat_interval_secs": 30
      }
    },
    "slack": {
      "enabled": true,
      "type": "remote",
      "url": "https://slack-mcp.example.com",
      "oauth": {
        "client_id": "${SLACK_CLIENT_ID}",
        "client_secret": "${SLACK_CLIENT_SECRET}",
        "scope": "chat:write,channels:read"
      }
    }
  }
}
```

The TOML spelling is the same shape, one level of tables:

```toml
[mcp.eggsearch]
type = "local"
command = "eggsearch"
args = ["mcp", "stdio"]
```

The `schema.rs:4718` test `explicit_mcp_eggsearch_does_not_force_search_section`
parses exactly this direct-key form and asserts
`cfg.mcp.contains_key("eggsearch")`.

Config types (`crates/codegg-config/src/schema.rs`):

| Struct | Line | Keys |
|--------|------|------|
| `McpEntry` | `:1569` | `enabled`, flattened `McpServerConfig` |
| `McpServerConfig` | `:1577` | `type`, command, args, env, url, headers, etc. |
| `McpReconnectConfig` | `:1594` | `enabled`, `max_retries`, delay, heartbeat intervals |
| `McpOAuthConfig` | `:1604` | `client_id`, `client_secret`, `scope` |

Note: The JSON key is `"type"` (via `#[serde(rename = "type")]` on
`McpServerConfig.server_type`). The `environment` field is merged with
`env` when present.

## Invariants & Gotchas

- **`McpService` is `Send + Sync`** — every field is (`HashMap<String,
  McpServer>`, `HashMap<String, String>` host-scope tokens, `OAuthManager`),
  and the service is held in a `static StdRwLock<Option<Arc<RwLock<McpService>>>>`
  (`src/search_backend/state.rs`), which only compiles under that bound.
  The `Arc<RwLock<McpService>>` wrapper is for *mutual exclusion*, not for
  thread-safety of the type itself.
- **Write lock during tool calls** — `call_tool_structured` acquires a
  write lock on the inner client, so concurrent calls to the same
  server serialize. This is acceptable because tool calls are
  infrequent relative to other operations.
- **Mock variant is always compiled** — `McpClientType::Mock` is
  behind `#[cfg(test)]` neither in the enum definition nor the match
  arms. The variant exists unconditionally so integration test binaries
  (which don't share `cfg(test)`) can reach `register_mock_server`.
- **SSE is partially integrated** — `parse_sse_response` handles SSE
  formatted responses in `post_json`, but `connect_sse_stream` for
  persistent SSE subscriptions is `#[allow(dead_code)]`. The agent
  does not initiate long-lived SSE connections.
- **Heartbeat uses `send_notification`** — the heartbeat `ping` is a
  notification (no response expected). If it fails, reconnect is
  triggered.
- **Protocol versions**: The shared client policy prefers modern
  `2026-07-28` discovery/per-request metadata and explicitly falls back to
  legacy `2024-11-05` initialization only for protocol-shape probe errors.
- **Spawn env is cleared** — `env_clear()` then re-injects PATH and
  configured env. This prevents the child from inheriting Codegg's
  own environment variables.
- **Tool catalog identity**: `McpService::catalog_fingerprint()` sorts
  servers/tools and fingerprints names, descriptions, input/output schemas,
  annotations, and discovery metadata. Tool ordering alone does not invalidate
  the identity; meaningful metadata changes do.
- **Plugin contributions**: `McpService::reconcile_plugin_servers` translates
  active declarative plugin declarations into the normal stdio/http lifecycle.
  Plugin `type` aliases `local`/`stdio` and `remote`/`http` are canonicalized
  at the plugin contribution boundary; configured MCP parsing continues to use
  its existing `local`/`remote` vocabulary.
  Plugin server names are `plugin:<plugin-name>:<declared-name>` and carry
  `McpServerOrigin::Plugin`. Reconciliation removes only stale plugin-owned
  servers and reports configured/other-plugin collisions; it never overwrites
  configured servers. All transport, security, exposure, and permission
  behavior remains in this service.

## Testing

```bash
# MCP integration tests live in the `config_provider` test family
cargo test --test config_provider -- mcp
cargo test --test config_provider -- mcp_reconnect
cargo test --test config_provider -- fake_eggsearch_mcp

# Unit tests within the module
cargo test -p codegg mcp::tests
cargo test -p codegg mcp::local::tests
cargo test -p codegg mcp::remote::tests
cargo test -p codegg mcp::auth::tests
cargo test -p codegg mcp::protocol::tests
```

`fake_eggsearch_mcp` exercises the end-to-end eggsearch dispatch
path using an in-process mock `McpService` (no real binary needed).
The real-process pair gate is `scripts/test-eggsearch-pair.sh`, which
builds the pinned eggsearch revision and checks its version identity
before running the native adapters.

## Related Docs

- [search_backend.md](search_backend.md) — eggsearch adapter consumes
  `McpService` for search/fetch operations
- [tool.md](tool.md) — tool execution and `McpExposurePolicy`
- [agent.md](agent.md) — uses MCP tools via `ToolRegistry`
- [security.md](security.md) — DNS rebinding protection

## Source Verification

Verified 2026-10-06 against `src/mcp/` and `crates/codegg-core/src/error.rs`.
- Added the omitted `protocol.rs` to the "Where It Lives" tree, documented as
  `pub(crate)` (`mod.rs:12`) with its protocol-version constants and byte
  ceilings.
- Corrected 18 stale `file.rs:line` refs in the Key Types table (all `mod.rs`
  rows were off by 13-52 lines). `McpCli` (`cli.rs:17`), `McpCommand`
  (`cli.rs:185`), and `IdeServer` (`ide_server.rs:50`) were already correct.
- Corrected a broken ref: `McpError` was documented at `error.rs:177`, but no
  `src/mcp/error.rs` exists — the enum is
  `crates/codegg-core/src/error.rs:198`.
- Confirmed correct as written: `McpExposurePolicy`'s two fields
  (`show_raw`, `hidden_servers`, `:179`), the 3-variant `McpClientType`,
  protocol versions `2026-07-28` / `2024-11-05`, and the
  `CodeGG MCP` OAuth envelope contract.

Verified 2026-10-06 (second pass), with schema references rechecked 2026-10-15
against `crates/codegg-config/src/schema.rs`.
- **Removed a phantom `mcp.servers.<name>` level** from the Configuration
  Surface example. `Config.mcp` is `Option<HashMap<String, McpEntry>>`
  (`schema.rs:245`) — the map key *is* the server name. The prior nested
  `"servers"` object could never register its children: it parsed as one
  entry named `servers` whose flattened `McpServerConfig` was all-`None`,
  because `McpEntry`/`McpServerConfig` are `#[serde(default)]` without
  `deny_unknown_fields` (`schema.rs:1239`, `:1247`). Proof of the real shape:
  the test `explicit_mcp_eggsearch_does_not_force_search_section`
  (`schema.rs:4252`) parses `"mcp": {"eggsearch": {...}}` directly and
  asserts `contains_key("eggsearch")`. Added the equivalent TOML spelling.
- Corrected 4 stale schema line refs: `McpEntry` `:934`→`:1239`,
  `McpServerConfig` `:942`→`:1247`, `McpReconnectConfig` `:959`→`:1264`,
  `McpOAuthConfig` `:969`→`:1274`.
- Re-confirmed all 17 `mod.rs`/`local.rs`/`remote.rs`/`auth.rs`/`cli.rs`/
  `ide_server.rs` rows in the Key Types table as still accurate.

Verified 2026-10-23 against `src/mcp/`, `crates/codegg-core/src/error.rs`,
`crates/codegg-config/src/schema.rs`, `src/security/ssrf.rs`,
`src/tool/mcp_resource.rs`, `src/search_backend/{bootstrap,state,eggsearch}.rs`,
`scripts/test-eggsearch-pair.sh`, and `tests/config_provider/`.
- **Corrected a wrong invariant.** The doc claimed "`McpService` is not
  `Send + Sync`". It **is**: every field is `Send + Sync`, and the service is
  held in a `static StdRwLock<Option<Arc<RwLock<McpService>>>>>`
  (`src/search_backend/state.rs`), which only compiles under that bound. The
  `Arc<RwLock<…>>` wrapper is for mutual exclusion, not for thread-safety of
  the type.
- **Corrected 4 stale Key Types refs**: `McpExposurePolicy` `:179`→`:192`,
  `IdeServer` `ide_server.rs:50`→`:51`, `McpError`
  `error.rs:198`→`:234`, `parse_mcp_tool_server` `mod.rs:220`→`:233`. Added
  rows for `McpServerOrigin` (`:130`), `McpPluginReconcileReport` (`:139`),
  `NegotiatedProtocol` (`protocol.rs:20`), and `DiscoveryInfo`
  (`protocol.rs:46`).
- **Corrected 5 stale schema refs**: `McpEntry` `:1213`→`:1569`,
  `McpServerConfig` `:1221`→`:1577`, `McpReconnectConfig` `:1238`→`:1594`,
  `McpOAuthConfig` `:1248`→`:1604`, and the proof test
  `explicit_mcp_eggsearch_does_not_force_search_section` `:4252`→`:4718`.
  The 2026-10-06 pass had already corrected these to `:1239`/`:1247`/`:1264`/
  `:1274`; they moved again.
- **Corrected the CLI block.** `codegg mcp add --type` is wrong: clap's
  derive generates `--server-type` from the `server_type` field (the config
  key is still `"type"`). `codegg mcp enable <name> --enabled true|false` is
  wrong: `enabled` is a `bool` flag (`#[arg(long, default_value = "true")]`),
  so it takes no value. `codegg mcp debug --name <server>` is wrong: the
  server name is a positional. Now matches `docs/MCP.md`.
- **Corrected the eggsearch pin.** The doc named a single immutable commit.
  There are **two** pins and they disagree: `scripts/test-eggsearch-pair.sh:6`
  pins `13fcac0b7ac0125d38e4e9775ebe1d62028fda72` while
  `src/install.rs:24` (`PINNED_EGGSEARCH_COMMIT`, the installation-owned
  authority for the shipped bundle) still pins
  `32f48bb57a43274d763c7f9ba919eab84d125edc`. Both agree on the version
  `0.4.4`. The doc now records the two pins and the divergence instead of
  silently adopting one of them.
- **Corrected the Testing section.** `tests/mcp.rs`, `tests/mcp_reconnect.rs`
  and `tests/fake_eggsearch_mcp.rs` no longer exist as standalone targets —
  the root package's integration tests are families. The three members now
  live in the `config_provider` family (`tests/config_provider/{mcp,mcp_reconnect,fake_eggsearch_mcp}.inc`)
  and are selected with `cargo test --test config_provider -- <member>`.
- **Added depth**: the four resource byte ceilings with their consumers, the
  `mcpres:v1:<server>:<sha256>` handle format, the managed-master-key
  fallback in `codegg_config::encryption` (`CODEGG_MASTER_KEY_FILE`, else
  `<config_dir>/codegg/master.key`), and a new "Host-scoped local retrieval"
  subsection covering `connect_stdio_host_scoped`, the
  `codegg/host-local-scope` metadata, and the `>=0.4.4, <0.5.0` server-version
  gate.
- **Re-confirmed as accurate**: the `Where It Lives` tree (including
  `pub(crate) mod protocol;` at `mod.rs:12`), the protocol-version constants
  and byte ceilings in `protocol.rs`, `env_clear()` + PATH re-injection, the
  `min(timeout, 10s)` spawn timeout, the EOF error string, the
  `follow_redirects(false)` policy, the port-qualified DNS snapshot pinned to
  the Eggfetch request, the `is_internal_ip` blocked ranges (loopback,
  RFC1918, link-local, CGNAT), the 1s→60s backoff / 5 retries / 30s heartbeat,
  `is_legacy_probe_error`'s narrow classification, the SSRF and OAuth
  contracts, the `McpExposurePolicy` field set and `list_filtered_tools`
  semantics, `catalog_fingerprint`, `reconcile_plugin_servers` (including the
  `plugin:<plugin>:<name>` naming and the `local`/`stdio` ↔ `remote`/`http`
  canonicalization at `src/plugin/manifest.rs`), and the eggsearch adapter
  control names (`excerpt_count` ≤ 3, `focus`, `cache_policy`,
  `max_cache_age_seconds`, internal `response_detail=diagnostic`).
