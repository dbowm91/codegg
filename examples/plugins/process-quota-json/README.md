# process-quota-json — Structured JSON Plugin

Demonstrates the structured plugin path: the process reads a
`PluginInvocation` JSON from stdin and emits a `PluginResponse` JSON to
stdout, including UI effects (dialog + chat) and diagnostics.

## What it shows

- `stdin: json` / `stdout: json` frontmatter for the plugin protocol
- Parsing `PluginInvocation` fields: `protocol_version`, `invocation_id`,
  `args`, `input`, `context`
- Emitting `PluginResponse` with `effects`, `data`, and `diagnostics`
- Building `UiNode::Table` and `UiEffect::OpenDialog` in JSON
- Graceful error handling for malformed input (never crashes)

## Wire format

### PluginInvocation (input)

```json
{
  "protocol_version": 1,
  "invocation_id": "test-001",
  "plugin_id": "process-quota-json",
  "capability": {"type": "command", "name": "quota-json"},
  "args": ["--provider", "anthropic"],
  "input": {},
  "context": {
    "session_id": "sess-test",
    "turn_id": null,
    "project_dir": "/tmp",
    "model": null,
    "agent": null,
    "frontend_capabilities": [],
    "metadata": {}
  }
}
```

### PluginResponse (output)

```json
{
  "ok": true,
  "effects": [
    {
      "type": "emit_chat",
      "block": {
        "format": "markdown",
        "content": "Quota for anthropic: 6,579 req remaining"
      }
    },
    {
      "type": "open_dialog",
      "dialog": {
        "id": "quota",
        "title": "Provider Quota",
        "body": {
          "kind": "table",
          "columns": ["Provider", "Limit", "Used", "Remaining"],
          "rows": [["anthropic", "10,000 req/day", "3,421 req", "6,579 req"]]
        },
        "modal": true
      }
    }
  ],
  "data": {
    "provider": "anthropic",
    "limit": "10,000 req/day",
    "used": "3,421 req",
    "remaining": "6,579 req"
  },
  "diagnostics": [
    {"level": "info", "message": "rendered quota for provider anthropic"}
  ]
}
```

## Manual testing

```bash
echo '{"protocol_version":1,"invocation_id":"test-001","plugin_id":"process-quota-json","capability":{"type":"command","name":"quota-json"},"args":["--provider","anthropic"],"input":{},"context":{"session_id":"sess-test","turn_id":null,"project_dir":"/tmp","model":null,"agent":null,"frontend_capabilities":[],"metadata":{}}}' | python3 scripts/quota_json.py
```

Or use the provided sample file:

```bash
cat sample_invocation.json | python3 scripts/quota_json.py
```

### Debug mode

Pass `--print-invocation` to dump the parsed invocation back:

```bash
echo '{"protocol_version":1,"invocation_id":"test-001","plugin_id":"process-quota-json","capability":{"type":"command","name":"quota-json"},"args":["--print-invocation"],"input":{},"context":{"session_id":null,"turn_id":null,"project_dir":null,"model":null,"agent":null,"frontend_capabilities":[],"metadata":{}}}' | python3 scripts/quota_json.py
```

## Safety

- Process plugin: local executable, not sandboxed
- No network access, no secrets, no env passthrough
- Only reads stdin and writes stdout
- Malformed input produces an error `PluginResponse` (never crashes)
