---
name: e2e
description: End-to-end testing guide for opencode-rs TUI application
version: 1.0.0
tags:
  - testing
  - e2e
  - integration
  - ci-cd
---

# E2E Testing Skill

## Description
Guide for End-to-End (E2E) testing of the opencode-rs TUI application, including infrastructure setup, alternatives, and CI/CD non-interactive testing.

## Triggers
E2E test, end-to-end test, TUI test, PTY test, integration test, CI/CD test, non-interactive mode

## Non-Interactive Mode (Recommended for CI/CD)

- **Flags**: `--run <prompt>`, `exec` subcommand
- **Output Formats**: `--output-format text|json`
- **Example**:
  ```bash
  opencode-rs --run "say hello" --output-format text
  opencode-rs exec "say hello"
  ```
- **Test Approach**: Run binary with `--run`, capture stdout/stderr, assert output

## Alternative Tools
- `expect` - Classic tool for interactive program testing
- `script` - Capture terminal sessions
- `pty` - Python PTY library for custom test harnesses

## Test Organization
```
tests/
├── tui.rs            # Existing TUI integration tests
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
- TUI mode (default) requires interactive terminal, not suitable for CI
- Provider configuration required for full `--run` tests (mock providers recommended)
