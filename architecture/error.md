# Error Module

The error module provides centralized error handling using `thiserror`.

## Purpose

Centralized error enum (`AppError`) with per-domain sub-errors, error
context propagation via `From` trait implementations, HTTP status mapping
for server responses, and retryability determination for resilience
patterns.

## Where It Lives

- **Canonical source**: `crates/codegg-core/src/error.rs` — defines
  `AppError` and all domain error enums except `ConfigError` (which wraps
  `codegg_config::ConfigError`).
- **Root re-export + server wrappers**: `src/error.rs` — re-exports
  `codegg_core::error::*` and adds `AxumAppError` / `AxumServerRuntimeError`
  behind `#[cfg(feature = "server")]`.

> **Note:** `ProviderError` and `StorageError` originate in
> `codegg_providers::error` and are re-exported into `codegg_core::error`
> via `pub use codegg_providers::error::{ProviderError, StorageError};`.

## How It Works

All domain error types live in `crates/codegg-core/src/error.rs`. The
`AppError` enum wraps each domain error via `#[from]` so the `?` operator
can convert automatically. The root `src/error.rs` re-exports everything
and adds axum-specific newtype wrappers that implement `IntoResponse`.

The `IntoResponse` implementation maps each `AppError` variant to an HTTP
status code. Server errors (5xx) are logged at `error` level; client
errors (4xx) at `warn`. Response bodies use canonical reason phrases
without leaking internal details.

## Key Types & APIs

### AppError (`crates/codegg-core/src/error.rs:6`) — 18 variants

```rust
#[derive(Error, Debug)]
pub enum AppError {
    Config(ConfigError),        // #[from]
    Storage(StorageError),      // #[from]
    Provider(ProviderError),    // #[from]
    Agent(AgentError),          // #[from]
    Tool(ToolError),            // #[from]
    Permission(PermissionError),// #[from]
    Mcp(McpError),              // #[from]
    Plugin(PluginError),        // #[from]
    Lsp(LspError),              // #[from]
    Io(std::io::Error),         // #[from]
    Json(serde_json::Error),    // #[from]
    Http(HttpError),            // sanitized, transport-neutral details
    Other(anyhow::Error),       // #[from]
    Worktree(String),
    Upgrade(String),
    Clipboard(String),
    Tui(String),
    RunStore(RunStoreError),    // #[from]
}
```

### HttpError (`crates/codegg-core/src/error.rs:65`)

A sanitized, transport-neutral detail type for outbound HTTP failures. It
carries only a `message: String` and an optional `status: Option<u16>`;
`HttpError::new(message, status)` builds it and `status()` reads the code
back. `AppError::Http` is **not** a `#[from]` — the caller constructs the
`HttpError` explicitly, so no raw transport error (and therefore no URL or
credential) can leak into the enum by accident.

### ConfigError (`crates/codegg-core/src/error.rs:84`)

Wraps `codegg_config::ConfigError` with an explicit `From` impl (line 101).
Variants: `NotFound`, `Invalid`, `Parse`, `Merge`, `Watch`.

### ProviderError (re-exported from `codegg_providers::error`)

Variants: `NotFound`, `Api { code, message, url }`, `Stream`, `RateLimit`,
`RateLimited { retry_after }`, `Transport { kind }`, `Auth`,
`ModelNotFound`, `Timeout`, `CircuitOpen`.

Constructors: `api()` (empty URL), `api_with_url()`, `rate_limited()`,
`rate_limit_from_headers()`, `from_http_status()`. `parse_retry_after()`
and `retry_after_from_headers()` produce a bounded `Retry-After` hint
(clamped to `MAX_RETRY_AFTER_HINT = 30s`; HTTP-date forms are
deliberately unsupported so an adapter never blocks on a far-future server
clock).

`retry_disposition()` is the explicit Permanent/Transient/Conditional
taxonomy; `is_retryable()` returns `true` only for Transient
(`RateLimit`/`RateLimited`, `Timeout`, `Stream`, transient `Transport`,
transient-status `Api` — every 5xx, plus 408/425/429, plus the
`overloaded`/`529` wordings). `Auth`, `ModelNotFound`, `NotFound`, and
`CircuitOpen` (conditional) are never blindly retried. `error_class()` is
the secret-safe diagnostic label; `Transport` carries only the eggfetch
category, never URLs or keys.

### StorageError (re-exported from `codegg_providers::error`)

Variants: `Database`, `Migration`, `NotFound`, `Conflict`, `LlmOperation`,
`Import`, `Export`. `Conflict` is distinct from `Database` on purpose: the
write was **refused**, not failed, so a caller that retries it unchanged
will be refused again.

### ToolError (`crates/codegg-core/src/error.rs:141`)

Variants: `NotFound`, `Execution`, `Timeout`, `Permission`, `Format`,
`Disabled`, `MissingWorkspaceRoot { context }`,
`ProcessDenied { kind, reason, recovery }`, `BackendUnavailable`, `Io`,
`Network`. `is_retryable()` for `Io`, `Network`, `Timeout`.

`MissingWorkspaceRoot` and `ProcessDenied` are **refusals, not failures**,
and are deliberately distinct from `Execution` (a failure *inside* a rooted
call) and from `Permission` (a judgement about *this caller*):

- `MissingWorkspaceRoot` — the caller supplied no workspace root at all.
  The daemon's process working directory is never an acceptable substitute.
- `ProcessDenied` — a fact about *this checkout*: a shared checkout admits
  no unqualified process, because the program's write set is not
  enumerable. It carries the recovery path so a caller that cannot tell
  what to do next does not simply retry the same call.

### PermissionError (`crates/codegg-core/src/error.rs:225`)

Variants: `Denied { tool, path }`, `Check`.

### McpError (`crates/codegg-core/src/error.rs:234`)

Variants: `Connection`, `Server`, `ToolCall`, `OAuth`, `Encryption`,
`Timeout`. `is_retryable()` for `Connection`, `Server`, `ToolCall`,
`OAuth`, `Timeout`. `Encryption` is intentionally NOT retryable.

### LspError (`crates/codegg-core/src/error.rs:329`)

Variants: `ServerNotFound`, `DownloadFailed`, `LaunchFailed`,
`NotInitialized`, `RequestFailed`, `RequestTimeout`, `UnsupportedLanguage`,
`Io`, `Json`, `UnsupportedSourceAction`, `CommandOnlySourceAction`,
`NoEditForSourceAction`, `AmbiguousSourceAction`, `CommandOnlyCodeAction`,
`Unsupported(LspUnavailable)`.

`is_retryable()` for `DownloadFailed`, `LaunchFailed`, `RequestFailed`,
`RequestTimeout`, `Io`.

Note: `egglsp::LspError` has additional variants (`UnsupportedEdit`,
`PathOutsideRoot`, `Utf16Position`, `OverlappingEdits`, `Protocol`,
`WriterClosed`, `InitializationCancelled`, `ServerRestarted`,
`ServerUnavailable`, `ServerDegraded`, `InvalidConfig`) that are
collapsed into `RequestFailed` by the `From` conversion (line 267).
`egglsp::LspError::Unavailable` maps to `LspError::Unsupported`.

### PluginError (`crates/codegg-core/src/error.rs:393`)

Variants: `NotFound`, `LoadFailed`, `HookFailed`, `InstallFailed`,
`InvalidManifest`.

### ServerRuntimeError (`crates/codegg-core/src/error.rs:411`)

Variants: `Bind`, `Shutdown`, `WebSocket`, `Rpc`, `Auth`.

### ClientError (`crates/codegg-core/src/error.rs:429`)

Variants: `Connection`, `Unreachable`, `Rpc`, `WebSocket`, `Auth`.

### RunStoreError (`crates/codegg-core/src/error.rs:447`)

Variants: `Io`, `Json`, `NotFound`, `PathTraversal`, `IntegrityViolation`,
`RetentionError`, `ConcurrentWrite`.

### AgentError (`crates/codegg-core/src/error.rs:132`)

Variants: `NotFound`, `Invalid`.

### AxumAppError (`src/error.rs:17`)

Newtype wrapper for `AppError` implementing `IntoResponse`. Also has `From`
impls for `AppError`, `StorageError`, `std::io::Error`,
`serde_json::Error`, `anyhow::Error`, so `?` works directly in axum
handlers. HTTP transport failures are converted at their owning boundary
into CodeGG error types.

### AxumServerRuntimeError (`src/error.rs:194`)

Newtype wrapper for `ServerRuntimeError` implementing `IntoResponse`.

## Configuration Surface

None. Error types are determined by the codebase, not configuration.

## Invariants & Gotchas

- **Orphan rule**: `AppError` lives in `codegg-core` but `IntoResponse`
  is implemented in root `src/error.rs` via newtype wrappers because axum
  is a forbidden dependency of `codegg-core`.
- **ConfigError bridge**: `codegg_config::ConfigError` is converted to
  `codegg_core::error::ConfigError` via explicit `From` impl (line 101),
  not a direct `#[from]` on `AppError`. `codegg_config::AppError` is
  likewise bridged by an explicit `From` impl (line 121) that matches
  `Config`/`Io`/`Other`.
- **McpError::Encryption not retryable**: Intentional; encryption failures
  require manual intervention.
- **ProviderError::api()** sets `url` to empty string. Use
  `api_with_url()` when the URL is available.
- **LspError variant collapse**: Several `egglsp::LspError` variants are
  collapsed into `LspError::RequestFailed` by the `From` conversion.
  Callers see less granularity than the LSP crate provides.
- **A refusal is not a failure.** `ToolError::MissingWorkspaceRoot`,
  `ToolError::ProcessDenied`, and `StorageError::Conflict` all mean "not
  admitted", not "the attempt failed". Each is deliberately distinct from
  the neighbouring error it resembles, and each maps to a status that says
  so (400 / 409) rather than a generic server fault. A caller that cannot
  tell what to do next will simply retry the same call — so these carry
  their recovery path.
- **`HttpError` is not a `#[from]`.** It is constructed explicitly so no
  raw transport error (which may carry a credential-bearing URL) can leak
  into `AppError` by accident.
- **Never log a secret.** `ProviderError::error_class()` and
  `ProviderError::Transport`'s `kind` are the only fields safe for
  diagnostics; the raw `Api` message and any transport detail are not.

## HTTP Status Mapping (`src/error.rs:59-157`)

| Error Type | Status |
|------------|--------|
| Config::NotFound | 404 |
| Config::Invalid/Parse/Merge | 400 |
| Config::Watch | 500 |
| Storage::NotFound | 404 |
| Storage::Conflict | 409 |
| Storage::Database/Migration/Import/Export/LlmOperation | 500 |
| Provider::Auth | 401 |
| Provider::RateLimit/RateLimited | 429 |
| Provider::Timeout | 504 |
| Provider::NotFound/ModelNotFound | 404 |
| Provider::Api/Stream/Transport/CircuitOpen | 502 |
| Agent::NotFound | 404 |
| Agent::Invalid | 400 |
| Tool::NotFound | 404 |
| Tool::MissingWorkspaceRoot | 400 |
| Tool::Permission / Permission::Denied | 403 |
| Tool::Timeout | 504 |
| Tool::Disabled | 403 |
| Tool::ProcessDenied | 409 |
| Tool::BackendUnavailable | 503 |
| Tool::Execution/Format/Io/Network | 502 |
| Permission::Check | 500 |
| Mcp::OAuth | 401 |
| Mcp::Timeout | 504 |
| Mcp::Connection/Server/ToolCall/Encryption | 502 |
| Plugin::NotFound | 404 |
| Plugin::InvalidManifest | 400 |
| Plugin::LoadFailed/HookFailed/InstallFailed | 500 |
| Lsp::ServerNotFound | 404 |
| Lsp::UnsupportedLanguage | 400 |
| Lsp::NotInitialized | 409 |
| Lsp::RequestTimeout/DownloadFailed/LaunchFailed/RequestFailed + source action errors + CommandOnlyCodeAction + Unsupported | 502 |
| Lsp::Io/Json | 500 |
| Json | 400 |
| Http | upstream status or 502 |
| Io/Other/Worktree/Upgrade/Clipboard/Tui/RunStore | 500 |

Two arms carry a status that is a *conflict*, not a server fault:
`Storage::Conflict` (a checkout writer refused because the checkout is
reserved by a live agent-run group) and `Tool::ProcessDenied` (the same
fact seen from the tool side). `Tool::BackendUnavailable` is 503, not 502:
the backend is temporarily absent, not proxying a broken upstream.

**Response body.** Every body is `{"error": <canonical reason>, "code":
<status>}` — the canonical reason phrase only, never the error's own
message, so no path, URL, or credential reaches the client. The single
exception is `Storage::Conflict`, whose body adds a `"message"` field
carrying the conflict's recovery guidance. That message names no path and
no other run's content.

**ServerRuntimeError IntoResponse** (`src/error.rs:206-212`):

| Status | Variants |
|--------|----------|
| 401 | `Auth` |
| 500 | `Bind`, `Shutdown`, `WebSocket`, `Rpc` |

## Key Conversions

| From | To | Notes |
|------|-----|-------|
| `codegg_config::ConfigError` | `ConfigError` | Explicit `From` impl |
| `codegg_config::ConfigError` | `AppError` | Explicit `From` impl (line 115) |
| `codegg_config::AppError` | `AppError` | Matches Config/Io/Other |
| `sqlx::Error` | `StorageError::Database` | Via `codegg-providers` |
| `eggfetch_core::Error` | `ProviderError::Transport` (transient kinds) or `ProviderError::Api` (permanent) | HTTP failures are classified without retaining raw endpoint URLs |
| `CircuitError::Open` | `ProviderError::CircuitOpen` | Circuit breaker integration |
| `egglsp::LspError` | `LspError` | Several variants collapsed to `RequestFailed` |
| `eggsentry::EggsecError` | `ToolError` | Io/FileTooLarge/Join mapped |

## Testing

```bash
cargo test -p codegg --features server -- error::tests
cargo test -p codegg-core --lib error
```

The root crate's tests (`src/error.rs`) cover HTTP status mapping for each
error family, canonical reason phrases in response bodies, no secret
leakage in error messages, and `ServerRuntimeError` status mapping. The
`codegg-core` test asserts that `HttpError` keeps its status without
transport or URL coupling.

## Related Docs

- [core.md](core.md) — root `src/error.rs` rationale and boundary
- [resilience.md](resilience.md) — Circuit breaker patterns using `is_retryable()`
- [exec.md](exec.md) — Exec mode error classification
- [provider.md](provider.md) — Provider retry logic using `is_retryable()`
- [retry.md](retry.md) — the bounded retry chain these dispositions feed

## Source verification

Verified 2026-10-23 against `crates/codegg-core/src/error.rs`,
`crates/codegg-providers/src/error.rs`, `crates/codegg-config/src/error.rs`,
and `src/error.rs`.

Corrections made in this pass:
- **Corrected nine stale line refs** that had drifted further since the
  2026-10-06 pass: `PermissionError` `:189` → `:225`,
  `McpError` `:198` → `:234`, the `egglsp::LspError` `From` impl
  `line 231` → `line 267`, `LspError` `:293` → `:329`,
  `PluginError` `:357` → `:393`, `ServerRuntimeError` `:375` → `:411`,
  `ClientError` `:393` → `:429`, `RunStoreError` `:411` → `:447`, and
  `AxumServerRuntimeError` `src/error.rs:169` → `:194`.
- **Corrected the `ToolError` variant list.** The doc listed 8; the enum
  has **11** — `MissingWorkspaceRoot { context }`,
  `ProcessDenied { kind, reason, recovery }`, and `BackendUnavailable`
  were all missing. (The old "ToolError 8" count in this doc's own
  verification section was the same error.) `BackendUnavailable` also
  affects the tool retry matrix in `src/tool/retry.rs`.
- **Corrected the HTTP status mapping table**, which was missing four
  arms: `Storage::Conflict` → 409, `Tool::MissingWorkspaceRoot` → 400,
  `Tool::ProcessDenied` → 409, and `Tool::BackendUnavailable` → 503.
- **Corrected the two mapping line ranges**: `src/error.rs:57-165` →
  `:59-157` and `src/error.rs:179-206` → `:206-212`.
- **Documented `HttpError`** (`crates/codegg-core/src/error.rs:65`),
  which the doc referenced in the `AppError` listing but never described.
  It is deliberately **not** a `#[from]` target.
- **Documented `StorageError`** and its `Conflict` variant, which the doc
  referenced only implicitly through the status table.
- **Documented the response-body rule**, including the one exception
  (`Storage::Conflict` carries a `"message"` recovery field) and the
  conflict / backend-unavailable statuses.
- **Corrected the `ProviderError` retry taxonomy**: `NotFound` is
  Permanent (the doc omitted it), and the transient-`Api` set is "every
  5xx plus 408/425/429 plus the `overloaded`/`529` wordings" — the old
  wording ("transient-status `Api`") understated it and would have
  mis-classified Anthropic's `529 overloaded_error`.
- **Added `MAX_RETRY_AFTER_HINT = 30s` and the `Retry-After` helpers**
  (`parse_retry_after`, `retry_after_from_headers`), and the fact that
  HTTP-date `Retry-After` forms are intentionally unsupported.
- **Added the `codegg_config::ConfigError` → `AppError` bridge** to the
  conversions table (it was missing).
- **Fixed the `Related Docs` links**, which pointed at bare directory
  names (`resilience/`, `exec/`, `provider/`) rather than the documents.

Confirmed correct as written: the `AppError` variant list and its count
(18); the `ConfigError` ref `:84` and its `From` impl `line 101`;
`AgentError` `:132`; `ToolError` `:141`; `AxumAppError` `src/error.rs:17`;
the `ProviderError` variant/constructor list; the `McpError`,
`PermissionError`, `PluginError`, `ServerRuntimeError`, `ClientError`,
`RunStoreError`, and `LspError` variant lists; the `is_retryable()`
predicates for `ToolError`, `McpError`, and `LspError`; and the
`ServerRuntimeError` status table.
