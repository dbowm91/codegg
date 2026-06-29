---
name: testing
description: Testing guidance for opencode-rs including unit, integration, and E2E tests
version: 1.0.0
tags:
  - testing
  - unit-test
  - integration-test
---

# Skill: Testing

Guidance for testing opencode-rs, including unit, integration, and E2E tests.

## Test Types

### Unit Tests
- Located in `src/<module>/tests/` or inline `#[cfg(test)]` modules
- Cover individual functions/components
- Run with: `cargo test --lib`

### Integration Tests
- Located in `tests/` directory
- Cover cross-module interactions (156+ TUI tests passing)
- Run with: `cargo test --tests`

### TUI Render Regression Tests
- Located in `tests/tui_render.rs` (49 tests, headless)
- Uses `ratatui::backend::TestBackend` to exercise `App::render()` across multiple terminal sizes
- Covers: empty states, streaming, tool calls, sidebar variants, dialogs, completions, toasts, pathological content, unicode, ANSI escapes, and component panic fallbacks
- Run with: `cargo test --test tui_render`

### E2E Tests
- TUI E2E with PTY infrastructure deferred due to complexity
- Non-interactive E2E testing supported via `exec` mode (see `exec` skill)
- CI/CD testing uses `exec` mode with JSON I/O

## Running Tests

```bash
# All tests
cargo test

# Specific test file
cargo test --test tui

# Specific test function
cargo test test_non_interactive_run_mode

# Exec mode (non-interactive CI test)
opencode exec --json '{"prompt": "hello"}' --json-output
```

## Resolved E2E Issues (2026-05-03)
- Removed non-functional `tests/e2e.rs` and `tests/minimal_e2e.rs`
- Removed unused dev-dependencies `ratatui-testlib` and `portable-pty`
- E2E testing replaced with `exec` mode for CI/CD pipelines

## CI Configuration
- GitHub Actions workflow in `.github/workflows/ci.yml`
- Runs `cargo test` for unit/integration tests
- Exec mode can be added to CI with mock providers for non-interactive testing
