# LSP Module

The `lsp` module provides Language Server Protocol support for IDE-like features. It implements a **client-side LSP integration** that spawns and manages external LSP server processes.

**Location**: `src/lsp/`

## Key Responsibilities

- LSP server lifecycle management (download, launch, initialize)
- Diagnostics collection with debouncing
- Code operations (goto definition, find references, hover, completion)
- Language detection from file extensions
- Project root detection

## Architecture

The module uses a client-per-root pattern: `LspService` maintains a `HashMap<String, ClientEntry>` where the key is `"{project_root}:{server_id}"`.

## Components

### mod.rs - Main Entry Point

```rust
pub struct Lsp {
    pub service: Arc<LspService>,
    pub operations: Arc<LspOperations>,
    pub diagnostics: Arc<DiagnosticsCollector>,
}

impl Lsp {
    pub async fn open_file(&self, path: &Path, content: &str) -> Result<(), LspError>
    pub async fn update_file(&self, path: &Path, content: &str) -> Result<(), LspError>
    pub async fn close_file(&self, path: &Path) -> Result<(), LspError>
    pub async fn save_file(&self, path: &Path, content: Option<&str>) -> Result<(), LspError>
    pub async fn shutdown(&self)
}
```

### service.rs - Client Management

```rust
pub struct LspService {
    clients: Arc<RwLock<HashMap<String, ClientEntry>>>,
    config: LspConfig,
}

impl LspService {
    pub async fn get_or_create_client(&self, file_path: &Path) -> Result<(String, PathBuf), LspError>
    pub async fn open_file(&self, file_path: &Path, text: &str) -> Result<(), LspError>
    pub async fn update_file(&self, file_path: &Path, text: &str) -> Result<(), LspError>
    pub async fn close_file(&self, file_path: &Path) -> Result<(), LspError>
    pub async fn save_file(&self, file_path: &Path, text: Option<&str>) -> Result<(), LspError>
    pub async fn shutdown_all(&self)
}
```

### client.rs - LSP Client

Manages JSON-RPC communication with a single LSP server process:

```rust
pub struct LspClient {
    pub server_id: String,
    pub root: PathBuf,
    pub process: tokio::sync::Mutex<LspProcess>,
    pub request_id: AtomicI64,
    pub capabilities: Mutex<Option<ServerCapabilities>>,
    pub opened_files: Mutex<HashMap<String, i32>>,
    pub diagnostics: Arc<Mutex<HashMap<String, Vec<lsp_types::Diagnostic>>>>,
}

impl LspClient {
    pub async fn new(server: &LspServerDef, binary: &Path, root: &Path, env: &[(String, String)]) -> Result<Self, LspError>
    pub async fn initialize(&self, init_opts: Option<serde_json::Value>) -> Result<ServerCapabilities, LspError>
    pub async fn send_request(&self, method: &str, params: serde_json::Value) -> Result<serde_json::Value, LspError>
    pub async fn send_notification(&self, method: &str, params: serde_json::Value) -> Result<(), LspError>
    pub async fn shutdown(&self) -> Result<(), LspError>
}
```

**Key operations**: `open_file`, `update_file`, `close_file`, `save_file`, `go_to_definition`, `find_references`, `hover`, `document_symbols`, `code_actions`, `completion`, `signature_help`

### operations.rs - High-Level Operations

```rust
pub struct LspOperations {
    service: Arc<LspService>,
}

impl LspOperations {
    pub async fn go_to_definition(&self, file_path: &Path, line: u32, column: u32) -> Result<Vec<LocationLink>, LspError>
    pub async fn find_references(&self, file_path: &Path, line: u32, column: u32) -> Result<Vec<Location>, LspError>
    pub async fn hover(&self, file_path: &Path, line: u32, column: u32) -> Result<Option<String>, LspError>
    pub async fn document_symbols(&self, file_path: &Path) -> Result<Vec<DocumentSymbol>, LspError>
    pub async fn code_actions(&self, file_path: &Path, start_line: u32, start_col: u32, end_line: u32, end_col: u32, diagnostics: Vec<Diagnostic>, only: Option<Vec<CodeActionKind>>) -> Result<Vec<CodeActionOrCommand>, LspError>
    pub async fn completion(&self, file_path: &Path, line: u32, column: u32, trigger_kind: Option<CompletionTriggerKind>, trigger_char: Option<String>) -> Result<Vec<CompletionItem>, LspError>
    pub async fn signature_help(&self, file_path: &Path, line: u32, column: u32) -> Result<Option<String>, LspError>
    pub async fn code_lens(&self, file_path: &Path) -> Result<Vec<CodeLens>, LspError>
}
```

### diagnostics.rs - Diagnostics Collection

```rust
pub struct DiagnosticsCollector {
    service: Arc<LspService>,
    last_update: Arc<Mutex<HashMap<String, Instant>>>,
}

const DEBOUNCE_MS: u64 = 150;

impl DiagnosticsCollector {
    pub async fn get_diagnostics_for_file(&self, file_path: &Path) -> Result<Vec<FileDiagnostic>, LspError>
    pub async fn get_all_diagnostics(&self) -> Result<HashMap<String, Vec<FileDiagnostic>>, LspError>
    pub async fn has_errors(&self, file_path: &Path) -> Result<bool, LspError>
}
```

### download.rs - Binary Download

```rust
pub async fn ensure_server_binary(server: &LspServerDef) -> Result<PathBuf, LspError>
pub fn cache_dir() -> PathBuf
```

1. First checks PATH for binary
2. Falls back to cached download in `$HOME/.cache/codegg/lsp/`
3. Only rust-analyzer has download specification currently

### launch.rs - Process Spawning

```rust
pub struct LspProcess {
    pub stdin: tokio::process::ChildStdin,
    pub stdout: tokio::process::ChildStdout,
    pub stderr: BufReader<tokio::process::ChildStderr>,
    pub child: tokio::process::Child,
}

pub async fn spawn_server(command: &str, args: &[&str], env: &[(String, String)], cwd: Option<&Path>) -> Result<LspProcess, LspError>
pub async fn send_request(process: &mut LspProcess, msg: &str) -> Result<(), LspError>
pub async fn read_response(process: &mut LspProcess) -> Result<String, LspError>
pub async fn drain_stderr(process: &mut LspProcess) -> String
```

Uses Content-Length headers for LSP message framing.

### language.rs - Language Detection

```rust
pub fn detect_language(path: &str) -> Option<&'static str>
pub fn extension_to_language_id(ext: &str) -> Option<&'static str>
pub fn language_id_to_server_id(lang_id: &str) -> Option<&'static str>
```

### root.rs - Project Root Detection

```rust
pub fn find_project_root(start: &Path) -> Option<PathBuf>
```

Detects project roots by looking for marker files like `.git`, `Cargo.toml`, `package.json`, etc.

### server.rs - Server Definitions

```rust
pub struct LspServerDef {
    pub id: &'static str,
    pub languages: &'static [&'static str],
    pub extensions: &'static [&'static str],
    pub repo: &'static str,
    pub command: &'static str,
    pub args: &'static [&'static str],
    pub download: Option<DownloadSpec>,
}

pub fn server_definitions() -> &'static [LspServerDef]
pub fn find_server(id: &str) -> Option<&'static LspServerDef>
pub fn find_server_for_language(lang: &str) -> Option<&'static LspServerDef>
pub fn find_server_for_extension(ext: &str) -> Option<&'static LspServerDef>
```

## Supported Languages (30+)

| Language | Server | Command |
|----------|--------|---------|
| Rust | rust-analyzer | rust-analyzer |
| Python | pyright | pyright-langserver --stdio |
| JavaScript/TypeScript | typescript-language-server | typescript-language-server --stdio |
| Go | gopls | gopls |
| C/C++ | clangd | clangd |
| Java | jdtls | jdtls |
| C# | omnisharp | OmniSharp |
| Ruby | ruby-lsp | ruby-lsp |
| Kotlin | kotlin-language-server | kotlin-language-server |
| Scala | metals | metals |
| Dart | dart-analysis-server | dart language-server --client-id codegg |
| Swift | swift-sourcekit | sourcekit-lsp |
| Haskell | haskell-language-server | haskell-language-server-wrapper --lsp |
| Lua | lua-language-server | lua-language-server |
| PHP | php-language-server | php-language-server |
| Perl/Raku | perl-language-server | perl-language-server |
| Zig | zls | zls |
| ... and more | | |

## Tool Integration

LSP is exposed via `LspTool` in `src/tool/lsp.rs` when `experimental.lsp_tool` config is enabled.

## Error Handling

```rust
pub enum LspError {
    ServerNotFound(String),
    DownloadFailed(String),
    LaunchFailed(String),
    NotInitialized(String),
    RequestFailed(String),
    RequestTimeout(String),
    UnsupportedLanguage(String),
    Io(std::io::Error),
    Json(serde_json::Error),
}
```

## Recent Bug Fixes

- **PATH parsing**: Fixed using `std::env::split_paths()` instead of splitting by `MAIN_SEPARATOR`
- **PHP mapping**: Fixed `intelephense` → `php-language-server`
- **Request timeout**: Added 30-second timeout to `send_request()`
- **Hardcoded PATH**: Now preserves user's actual PATH instead of hardcoding
- **Stderr logging**: Server stderr is now logged during initialization
- **Notification loop redundancy**: Fixed duplicate notification handling in `send_request()`
- **close_file race condition**: Fixed lock handling to use single write lock and properly update `opened_files`
- **save_file race condition**: Fixed lock handling to use single write lock

## See Also

- [.opencode/skills/lsp/SKILL.md](../.opencode/skills/lsp/SKILL.md) - LSP skill guide
- [tool.md](tool.md) - LSP tool wrapper