# Security Module Override

This file contains security-specific guidance and overrides root AGENTS.md.

## Subprocess Isolation

Always call `.env_clear()` and set a minimal safe `PATH` before spawning subprocesses. Use hardcoded `/usr/local/bin:/usr/bin:/bin` - do NOT use `std::env::var("PATH")` as it restores the original unsafe PATH.

## SSRF Protection

Use `validate_host_ip()` from `src/security/ssrf.rs` for all network-bound tools. Centralized SSRF logic is in `src/security/ssrf.rs`.

## Symlink Protection

Verify path components for symlinks BEFORE canonicalization using `symlink_metadata()`.

## Landlock Sandboxing

The `bash` tool supports OS-level filesystem sandboxing via Landlock (Linux 5.13+). Configure via `security.sandbox` in config.

## Tool Path Validation

Always use `validate_path()` and `check_path_for_symlinks()` from `src/tool/util.rs` before performing filesystem operations.