# Shell Session Module

## Purpose

`shell_session` provides in-memory metadata management for terminal
sessions. It tracks session identity, working directory, shell type, and
terminal dimensions. It does **not** create PTY sessions or execute
commands — that is handled by `tool::terminal` and the shell runtime.

## Where It Lives

`src/shell_session/` — two files:

| File | Contents |
|------|----------|
| `mod.rs` | `ShellSession`, `CreateShellSession`, `ShellResize` structs |
| `session.rs` | `ShellManager` — async CRUD operations backed by `Arc<RwLock<HashMap>>` |

## How It Works

`ShellManager` holds an `Arc<RwLock<HashMap<String, ShellSession>>>`.
All operations are async (tokio RwLock). Sessions are keyed by UUID and
filtered by `project_id` on list.

- `create()` generates a UUID, sets defaults for missing fields, and
  inserts into the map.
- `get()` / `list()` clone the session out of the lock.
- `update_cwd()` / `resize()` / `delete()` acquire a write lock and
  mutate or remove the entry.

Sessions are ephemeral — no disk persistence. When the process exits,
all session metadata is lost.

## Key Types & APIs

### ShellSession (`mod.rs:6`)

```rust
pub struct ShellSession {
    pub id: String,          // UUID v4
    pub project_id: String,  // scoping key
    pub cwd: String,         // current working directory
    pub shell: String,       // shell binary name
    pub cols: u16,           // terminal columns
    pub rows: u16,           // terminal rows
    pub created_at: i64,     // millis since epoch
}
```

### CreateShellSession (`mod.rs:16`)

Request type for session creation. All fields except `project_id` are
optional and fall back to defaults: `cwd="."`, `shell="bash"`,
`cols=80`, `rows=24`.

### ShellResize (`mod.rs:25`)

Terminal resize event: `{ cols: u16, rows: u16 }`.

### ShellManager (`session.rs:9`)

| Method | Signature | Notes |
|--------|-----------|-------|
| `new()` | `fn -> Self` | Empty map |
| `create()` | `async fn(CreateShellSession) -> Result<ShellSession>` | Generates UUID |
| `get()` | `async fn(&str) -> Option<ShellSession>` | Clone out |
| `update_cwd()` | `async fn(&str, &str) -> Result<ShellSession>` | NotFound if missing |
| `list()` | `async fn(&str) -> Vec<ShellSession>` | Filter by project_id |
| `resize()` | `async fn(&str, ShellResize) -> Result<()>` | NotFound if missing |
| `delete()` | `async fn(&str) -> Result<()>` | NotFound if missing |

`ShellManager` also implements `Default` (delegates to `new()`).

## Configuration Surface

None. Defaults are hard-coded:
- Default shell: `"bash"`
- Default terminal size: `80×24`
- Default cwd: `"."`

## Invariants & Gotchas

- **No PTY**: This module only tracks metadata. Shell execution lives
  in `src/shell/runtime.rs` (`ShellRuntime`) and
  `src/tool/terminal/`.
- **In-memory only**: No persistence. Sessions vanish on restart.
- **String cwd**: `cwd` is `String`, not `PathBuf`, to support
  serialization over the server protocol.
- **Project scoping**: `list()` filters by `project_id`. Sessions from
  different projects are invisible to each other.
- **Millisecond timestamps**: `created_at` uses `chrono::Utc::now().timestamp_millis()`.

## Testing

```bash
cargo test -p codegg --lib shell_session
```

11 tests covering all CRUD operations, defaults, not-found errors, and
project-scoped listing. All use `#[tokio::test]`.

## Related Docs

- [tool.md](tool.md) — Terminal tool that spawns shell commands
- [human_shell.md](human_shell.md) — Human `!`/`!!` shell execution
  (separate from session metadata)
