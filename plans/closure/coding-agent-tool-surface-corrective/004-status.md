# Coding-agent tool surface corrective M004 closure

Status: closed

## Source and reviewed baseline

- Source plan: `plans/implementation/coding-agent-tool-surface-corrective/004-structured-verification-facade.md`
- Roadmap: `plans/subsystems/coding-agent-tool-surface-corrective-roadmap.md`
- Reviewed implementation baseline: `a978484` (`feat(tool-surface): add structured verification facade`)
- Closure/status commit: recorded by the commit that updates this record and the registry.

## Executive result

M004 is complete. `verify` is a bounded model-facing semantic facade for
routine Cargo verification. It accepts only an allow-listed action, workspace
scope, timeout, and bounded report size; it generates offline Cargo argv and
delegates to the already configured `BashTool`. No second process executor,
scheduler, command-intent router, audit path, sandbox, or TestTool was added.

## Resolver support matrix

| Project evidence | `auto` | `check` / `typecheck` | `build` | `lint` | `format_check` |
|---|---|---|---|---|---|
| `Cargo.toml` | Cargo check | `cargo check --offline` | `cargo build --offline` | `cargo clippy --offline --all-targets --all-features -- -D warnings` | `cargo fmt --all -- --check` |
| Other/unknown project | explicit unsupported resolver miss | explicit unsupported resolver miss | explicit unsupported resolver miss | explicit unsupported resolver miss | explicit unsupported resolver miss |

The narrow first facade deliberately does not invent Python/JS/TS/Go/Make
commands. Callers receive an actionable fallback to `bash` or a
project-specific tool until an existing canonical resolver can own those
families.

## Owner trace and structured result

```text
verify action/schema validation
  -> fixed offline Cargo argv + workspace root
  -> configured BashTool
  -> command_intent::pipeline::prepare_command
  -> existing sandbox / permission / scheduler-or-managed process path
  -> existing bounded Bash result and audit/run provenance
  -> verify structured status projection
```

The result reports requested/effective action, project family, argv, cwd,
terminal status and exit code, bounded summary, truncation, elapsed time, and
next-action guidance. A non-zero exit marker is reported as `failed`; missing
terminal status is `unknown`, never success.

## Negative and security evidence

- The public schema has no `command`, package, path, install, or auto-fix field.
- Arbitrary command syntax and unsupported scopes are rejected before Bash.
- Cargo commands use `--offline`; no package installation or network behavior
  is inferred.
- Workspace execution uses the configured Bash workspace root and its existing
  path/sandbox/permission checks.
- `test` remains owned by `TestTool`; verify does not copy test resolution or
  scheduler submission.
- The facade is ShellExec-classified, so normal capability/permission and
  parent-ceiling policy still applies.

## Verification

- `cargo test -p codegg --lib tool::verify`: 3 passed.
- `cargo test -p codegg --lib command_intent`: 263 passed.
- `cargo test -p codegg --lib tool::bash`: 87 passed.
- `cargo test -p codegg --lib tool::test`: 54 passed.
- `cargo test --test command_routing_adversarial -- --test-threads=1`: 139 passed.
- `cargo test --test tool_execution -- --test-threads=1`: 55 passed.
- `cargo test --test tool_surface_minimization -- --test-threads=1`: 13 passed.
- `python3 scripts/check_execution_ownership.py`: passed.
- `python3 scripts/check_scheduler_bypass.py`: passed.
- `cargo fmt --all -- --check`: passed.
- `git diff --check`: passed.

## Unblock audit

M005 remains blocked, not closed. Its required adapter cannot be safely
implemented with the current construction seams: `ToolExecutionContext`
provides trusted session/cwd/turn fields but not the complete workspace/project
authority represented by `CoreRequest::LspPreviewApply`; `ToolRegistryOptions`
does not carry the daemon's canonical `WorkspaceLockTable`; and each
model-facing `LspTool` owns a private preview registry, so a separate adapter
cannot resolve the same host-owned preview candidate without inventing a new
shared registry seam. The M005 plan explicitly says to stop in this case.
