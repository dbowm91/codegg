# E2E Testing Skill

## Description
Guide for End-to-End (E2E) testing of the opencode-rs TUI application, including infrastructure setup, alternatives, and CI/CD non-interactive testing.

## Triggers
E2E test, end-to-end test, TUI test, PTY test, integration test, CI/CD test, non-interactive mode

## Infrastructure Options

### 1. ratatui-testlib (Investigated, Non-Functional)
- **Crate**: `ratatui-testlib v0.1.0` (added as dev-dependency)
- **Features**: PTY-based testing, Tokio support, screen state assertions
- **Status**: Non-functional on this system (tests hang on `TuiTestHarness::spawn`)
- **Dependencies**: `portable-pty v0.8` (added as dev-dependency)
- **Example Code**:
  ```rust
  use ratatui_testlib::{TuiTestHarness, Result};
  use portable_pty::CommandBuilder;
  
  #[test]
  fn test_e2e() -> Result<()> {
      let mut harness = TuiTestHarness::new(80, 24)?;
      let mut cmd = CommandBuilder::new("opencode-rs");
      cmd.arg("--run").arg("test");
      harness.spawn(cmd)?;
      harness.wait_for(|state| state.contains("output"))?;
      Ok(())
  }
  ```

### 2. Non-Interactive Mode (Recommended for CI/CD)
- **Flags**: `--run <prompt>`, `exec` subcommand
- **Output Formats**: `--output-format text|json`
- **Example**:
  ```bash
  opencode-rs --run "say hello" --output-format text
  opencode-rs exec "say hello"
  ```
- **Test Approach**: Run binary with `--run`, capture stdout/stderr, assert output

### 3. Alternative Tools
- `expect` - Classic tool for interactive program testing
- `script` - Capture terminal sessions
- `pty` - Python PTY library for custom test harnesses

## Test Organization
```
tests/
├── e2e.rs           # E2E test infrastructure (ratatui-testlib)
├── minimal_e2e.rs   # Minimal E2E examples
├── tui.rs            # Existing 156 TUI integration tests
└── ...               # Other integration tests
```

## CI/CD Integration
- Use `--run` non-interactive mode for automated testing
- Example GitHub Actions step:
  ```yaml
  - name: Run E2E tests
    run: |
      opencode-rs --run "hello" --output-format json
      # Assert output contains expected fields
  ```

## Known Issues
- `ratatui-testlib` PTY harness hangs on spawn in this environment
- TUI mode (default) requires interactive terminal, not suitable for CI
- Provider configuration required for full `--run` tests (mock providers recommended)
