# Git Phase F — Cross-Platform Behavior

## Supported Platforms

| Platform | Status | Notes |
|----------|--------|-------|
| Linux (x86_64/aarch64) | **Tested** | Primary development and CI target |
| macOS (x86_64/aarch64) | **Tested** | Primary development target |
| Windows | **Not supported** | Known limitations documented below |

---

## 1. Path Encoding / Separators

**Design principle:** All path manipulation uses `std::path::PathBuf::join()` and `Path::new()`. String concatenation for paths is never used.

| Aspect | Linux | macOS | Windows |
|--------|-------|-------|---------|
| Path separator in `RepoPath` | `/` (forward slash) | `/` (forward slash) | `/` (forward slash) — repo-relative paths always use `/` |
| `RepoRoot::new()` canonicalization | `std::fs::canonicalize()` resolves symlinks, normalizes to `/` | Same | Would resolve to `\` but `RepoPath` normalizes back to `/` |
| `PathBuf::join()` in `operation_state.rs` | Uses `/` via OS | Uses `/` via OS | Would use `\` but git plumbing files (`.git/MERGE_HEAD`) use `/` internally |
| `normalize_path()` in `path.rs:111` | Strips `./` prefix only | Same | Same — no separator conversion needed |

**Cross-platform concern:** `RepoRoot::new()` calls `canonicalize()` which returns OS-native separators. On Windows this would produce `\` separators in the `RepoRoot` internal `PathBuf`. However, `RepoPath` stores normalized forward-slash paths and joins them against the `RepoRoot` via `Path::join()`, which handles separator differences.

**Divergence from git:** None. Git itself uses forward slashes for repository-relative paths on all platforms.

**Code references:**
- `crates/codegg-git/src/path.rs:25-36` — `RepoRoot::new()` with canonicalization
- `crates/codegg-git/src/path.rs:55-81` — `RepoPath::new()` with NUL, absolute, and traversal rejection
- `crates/codegg-git/src/path.rs:111-118` — `normalize_path()` strips `./` prefix

---

## 2. Executable Discovery

**Design principle:** No hardcoded `git` path. The `"git"` string is passed directly to `Command::new()`, which searches `$PATH` (or equivalent) at runtime.

| Aspect | Linux | macOS | Windows |
|--------|-------|-------|---------|
| How `git` is found | `$PATH` lookup | `$PATH` lookup | Would need `git.exe` on `$PATH` |
| Fallback PATH in `worktree.rs:22` | `/usr/local/bin:/usr/bin:/bin` | Same fallback | N/A — would need Windows `PATH` entries |
| Fallback PATH in `git_service.rs:821` | `/usr/local/bin:/usr/bin:/bin` | Same fallback | N/A |

**Cross-platform concern:** Two fallback `PATH` values hardcode Unix paths (`/usr/local/bin:/usr/bin:/bin`). On Windows, if `$PATH` is not set in the environment (unlikely but possible), git discovery would fail because these Unix paths do not exist.

**Divergence from git:** None on supported platforms. Git itself uses `$PATH` lookup.

**Code references:**
- `crates/egggit/src/worktree.rs:18-24` — `run_git_async()` uses `Command::new("git")`
- `crates/egggit/src/status.rs:34` — `std::env::var_os("PATH")`
- `src/git_service.rs:813-822` — `GitExecutionService` with fallback PATH

---

## 3. Process Termination

**Design principle:** All git subprocess invocations use Tokio's `kill_on_drop(true)`, which kills the child process if the owning `Command`/`Child` is dropped (e.g., timeout, parent exit).

| Aspect | Linux | macOS | Windows |
|--------|-------|-------|---------|
| `kill_on_drop(true)` behavior | Sends `SIGKILL` | Sends `SIGKILL` | Calls `TerminateProcess()` |
| Timeout enforcement | `tokio::time::timeout()` + `kill_on_drop` | Same | Same |
| Default mutation timeout | 30s (`git_mutations.rs:456`) | Same | Same |
| Default network timeout | 120s (`git_network_policy.rs:68`) | Same | Same |

**Cross-platform concern:** The semantics of `kill_on_drop` differ — Unix sends `SIGKILL` (immediate, no cleanup), Windows calls `TerminateProcess` (also immediate, no cleanup). In practice the behavior is equivalent for our use case (we want the git process dead on timeout/drop).

**Divergence from git:** None. This is a process-management concern, not a git behavioral difference.

**Code references:**
- `src/git_mutations.rs:106` — `cmd.kill_on_drop(true)`
- `src/git_network_ops.rs:85` — `cmd.kill_on_drop(true)` for network ops
- `src/git_service.rs:825` — `cmd.kill_on_drop(true)` for read service
- `crates/egggit/src/worktree.rs:19` — synchronous `Command` (no `kill_on_drop`, uses `spawn_blocking`)

---

## 4. HOME / XDG and Credential Helper Handling

**Design principle:** The environment is cleared (`env_clear()`) before each git subprocess, then a controlled allowlist of variables is restored. This prevents parent environment leakage while preserving credential helper function.

### Local Mutations

| Variable | Restored? | Source |
|----------|-----------|--------|
| `HOME` | Yes | `ALLOWED_ENV_VARS` in `git_mutations.rs:39` |
| `XDG_CONFIG_HOME` | Yes | `ALLOWED_ENV_VARS` in `git_mutations.rs:40` |
| `XDG_DATA_HOME` | Yes | `ALLOWED_ENV_VARS` in `git_mutations.rs:41` |
| `XDG_CACHE_HOME` | Yes | `ALLOWED_ENV_VARS` in `git_mutations.rs:42` |
| `GIT_TERMINAL_PROMPT` | Pinned to `0` | Prevents credential helper from blocking |
| `GIT_EDITOR` | Pinned to `true` | Prevents editor launch |
| `EDITOR` / `VISUAL` | Removed | Prevents editor launch |

### Network Operations

Additional variables restored via `NETWORK_ALLOWED_ENV_VARS` in `git_network_policy.rs:41-63`:

| Variable | Purpose |
|----------|---------|
| `GIT_ASKPASS` | Credential helper entry point |
| `GIT_SSH_COMMAND` | Custom SSH command |
| `GIT_SSH_VARIANT` | SSH variant selection |
| `GIT_CONFIG_GLOBAL` | Global git config path |
| `GIT_CONFIG_SYSTEM` | System git config path |
| `HTTP_PROXY` / `HTTPS_PROXY` / `NO_PROXY` | Proxy configuration |

| Aspect | Linux | macOS | Windows |
|--------|-------|-------|---------|
| `HOME` resolution | `$HOME` env var | `$HOME` env var | Would need `USERPROFILE` (not in allowlist) |
| Credential helper config path | `$XDG_CONFIG_HOME/git/credentials` or `$HOME/.gitcredentials` | `$HOME/.gitcredentials` | `%APPDATA%\git\credentials` (not restored) |
| SSH agent socket | `$SSH_AUTH_SOCK` restored | `$SSH_AUTH_SOCK` restored | N/A — Windows uses `ssh-agent.exe` service |

**Cross-platform concern:** On Windows, `USERPROFILE` is not in `ALLOWED_ENV_VARS`. Git on Windows uses `USERPROFILE` as a fallback when `HOME` is not set. Since `HOME` IS restored, this typically works (git checks `HOME` first), but Windows-specific credential helpers that look at `%APPDATA%` would not function.

**Divergence from git:** Git itself handles Windows `USERPROFILE` / `HOMEDRIVE`+`HOMEPATH` fallback natively. Codegg's environment hardening intentionally limits this to `HOME`, which is sufficient for standard credential helpers.

**Code references:**
- `src/git_mutations.rs:37-53` — `ALLOWED_ENV_VARS` list
- `src/git_mutations.rs:83-108` — `GitEnvPolicy::apply()` environment construction
- `src/git_network_policy.rs:39-63` — `NETWORK_ALLOWED_ENV_VARS`

---

## 5. SSH Agent Handling

**Design principle:** The SSH agent is accessed through environment variables, not through direct socket/pipe manipulation.

| Aspect | Linux | macOS | Windows |
|--------|-------|-------|---------|
| `$SSH_AUTH_SOCK` | Restored in `ALLOWED_ENV_VARS` | Same | Not applicable — Windows uses named pipes |
| `$SSH_AGENT_PID` | Restored in `ALLOWED_ENV_VARS` | Same | Not applicable |
| `$GIT_SSH_COMMAND` | Restored in network ops | Same | Would be restored if set |
| `$GIT_SSH_VARIANT` | Restored in network ops | Same | Would be restored if set |

**Cross-platform concern:** On Windows, the SSH agent runs as a Windows service and communicates via named pipes, not Unix domain sockets. `$SSH_AUTH_SOCK` is not meaningful on Windows. Git for Windows handles this natively via its bundled `ssh.exe`, but Codegg's environment hardening would not restore Windows-specific SSH configuration.

**Divergence from git:** Git for Windows bundles its own `ssh.exe` and `ssh-agent.exe` that handle Windows-native agent communication. Codegg relies on the system SSH agent, which is the standard approach on Linux/macOS.

**Code references:**
- `src/git_mutations.rs:50-51` — `SSH_AUTH_SOCK`, `SSH_AGENT_PID` in allowlist
- `src/git_network_policy.rs:20-21` — SSH env vars documented

---

## 6. Temporary Repository Fixtures

**Design principle:** Tests use `tempfile::tempdir()` and `tempfile::Builder::new().tempdir()` for temporary directories, which delegate to the OS temp directory.

| Aspect | Linux | macOS | Windows |
|--------|-------|-------|---------|
| Temp directory | `/tmp` (or `$TMPDIR`) | `/tmp` (or `$TMPDIR`) | `%TEMP%` or `%TMP%` |
| Cleanup | Automatic on `TempDir` drop | Same | Same |
| Prefix support | `tempfile::Builder::new().prefix("egggit-")` | Same | Same |
| Symlink behavior | Tests create real dirs | Same | Would need developer mode |

**Cross-platform concern:** No significant concerns. `tempfile` is cross-platform and handles cleanup correctly on all platforms. Tests that use `Command::new("git")` would need `git.exe` on Windows `%PATH%`.

**Divergence from git:** None. This is test infrastructure, not production behavior.

**Code references:**
- `crates/egggit/src/status_v2.rs:507` — `use tempfile::TempDir`
- `crates/egggit/src/worktree.rs:170-172` — `tempfile::Builder::new().prefix(...)`
- `crates/egggit/src/operation_state.rs:593` — `tempfile::TempDir::new()`

---

## 7. File Permissions and Symlinks

**Design principle:** Codegg does not explicitly set file permissions or create symlinks in production code. Git handles file mode bits internally.

| Aspect | Linux | macOS | Windows |
|--------|-------|-------|---------|
| File mode bits in git index | Full support (`100644`, `100755`, `040000`, `120000`) | Limited — APFS ignores most mode bits | No native support — git emulates via `core.fileMode=false` |
| Symlink support | Native | Native (requires SIP-aware paths) | Requires developer mode or admin privileges |
| `std::fs::canonicalize()` | Resolves symlinks | Resolves symlinks | Resolves symlinks (may fail without permissions) |
| Conflict mode reporting | Mode stored in `ConflictEntry.mode` (`conflict.rs:97-98`) | Same | Git would not report mode differences |

**Cross-platform concern:** On macOS, `canonicalize()` resolves symlinks, which can produce unexpected paths when the home directory or project directory is a symlink (common with Homebrew, pyenv, etc.). On Windows, `canonicalize()` may fail on paths with restricted permissions.

**Divergence from git:** Git on macOS/APFS treats file mode as advisory-only (matching APFS behavior). Codegg inherits this — mode bits in status display reflect what git reports, which may differ from actual filesystem permissions.

**Code references:**
- `crates/codegg-git/src/path.rs:30-32` — `RepoRoot::new()` calls `canonicalize()`
- `crates/codegg-git/src/path.rs:70-73` — `RepoPath::new()` calls `canonicalize()` for containment check
- `crates/egggit/src/conflict.rs:97-98` — `ConflictEntry.mode` field
- `crates/egggit/src/worktree.rs:101-104` — `canonicalize()` for worktree path comparison

---

## 8. Newline and NUL-Delimited Parsing

**Design principle:** All structured git output uses NUL-delimited (`-z`) formats for safe machine parsing. Newlines in user-provided content (commit messages) use `\n` consistently.

| Aspect | Linux | macOS | Windows |
|--------|-------|-------|---------|
| Porcelain v2 `-z` output | NUL-delimited (`\0`) | Same | Same — git porcelain v2 `-z` always uses NUL |
| `split('\0')` parsing | Works correctly | Same | Same — NUL is not valid in file paths on any platform |
| Commit message newlines | `\n` (LF) | `\n` (LF) | `\n` (LF) — git always stores LF internally |
| `core.autocrlf` effect | Off by default | Off by default | May convert on checkout if enabled — Codegg does not set this |
| `MERGE_MSG` parsing | Splits on `\n` (`operation_state.rs:390`) | Same | Same |

**Cross-platform concern:** On Windows with `core.autocrlf=true`, git may convert LF to CRLF in working tree files. This does not affect NUL-delimited porcelain output, but could affect raw file reads. Codegg does not set `core.autocrlf` in its environment policy, relying on the repository's configuration.

**Divergence from git:** None. Git's porcelain v2 format is platform-stable. The NUL delimiter was specifically designed for cross-platform safety.

**Code references:**
- `crates/egggit/src/status_v2.rs:301-304` — Porcelain v2 NUL-delimited parsing
- `src/git_mutations.rs:362` — `raw.split('\0')` for snapshot parsing
- `crates/egggit/src/operation_state.rs:314` — `sequencer/todo` file read (newline-delimited)
- `crates/egggit/src/operation_state.rs:390` — `MERGE_MSG` newline splitting

---

## Summary: Windows Limitations

If Windows support were ever pursued, the following areas would need attention:

| Area | Current Behavior | Required Change |
|------|-----------------|-----------------|
| `USERPROFILE` env var | Not restored | Add to `ALLOWED_ENV_VARS` |
| Fallback PATH | Unix paths only | Add Windows `PATH` entries or remove fallback |
| SSH agent | Unix socket only | Windows named pipe support |
| Credential helpers | `HOME`-based paths | Support `%APPDATA%` paths |
| File mode bits | Passed through git | Git handles internally; no Codegg change needed |
| Symlinks | Not created by Codegg | No change needed |
| Temp directories | `%TEMP%` works via `tempfile` | No change needed |
| NUL-delimited parsing | Cross-platform | No change needed |
| `canonicalize()` | May fail on Windows paths | Error handling already in place |

---

## Divergence from Git Itself

| Behavior | Codegg | Git | Impact |
|----------|--------|-----|--------|
| Environment hardening | Clears env, restores allowlist | Inherits parent environment | More predictable; may break credential helpers that rely on non-standard env vars |
| `GIT_TERMINAL_PROMPT=0` | Always set | User-configurable | Prevents interactive prompts; credential helpers must support non-interactive mode |
| `GIT_EDITOR=true` | Always pinned | User-configurable | Prevents editor launches during commit/amend/rebase |
| PATH fallback | `/usr/local/bin:/usr/bin:/bin` | N/A (uses system PATH) | Only matters if `$PATH` is unset in parent |
| Credential helper timeout | No special timeout | N/A | `kill_on_drop` + 30s default timeout kills blocking credential helpers |
| File permission display | Shows git's reported mode bits | Shows git's reported mode bits | Identical — both reflect APFS/HFS+ advisory modes on macOS |
