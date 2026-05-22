# Exec Module Override

This file contains exec-specific guidance and overrides root AGENTS.md.

## Exec Mode

Non-interactive exec mode for CI/CD with JSON I/O. Use `codegg exec --json` for machine-readable output.

## Key Implementation Details (2026-05-22)

### Session ID Parameter
The `session_id` parameter in `ExecMode::new()` is now properly used. If not provided, a new UUID is generated.

### Error Classification
All major `ProviderError` variants are now properly classified in `classify_error()`:
- `CircuitOpen` → `CIRCUIT_OPEN`
- `Api { code, message, .. }` → `API_ERROR`
- `Stream` → `STREAM_ERROR`

### Config Errors
Config loading errors are now properly returned as `CONFIG_ERROR` rather than silently using defaults.

### Question Tool
`loop_instance.setup_question_channel()` is called to enable question tool handling with proper timeout.

### Missing Features
- **MCP Service**: Currently hardcoded to `None` in exec mode
- **Session Persistence**: Storage is initialized but not actively used (no message persistence between runs)

## See Also
- `.opencode/skills/exec/SKILL.md` - Full exec skill documentation