# MCP servers

CodeGG connects to external Model Context Protocol (MCP) servers. A server can provide tools, resources, and prompts.

## Configure a server

Add each server directly under the `mcp` key in your CodeGG config. There is no intermediate `servers` key.

A local server runs as a process and communicates over standard input and output:

```jsonc
{
  "mcp": {
    "my-local-server": {
      "type": "local",
      "command": "/path/to/server",
      "args": ["--stdio"],
      "timeout": 30000
    }
  }
}
```

A remote server uses an HTTP endpoint:

```jsonc
{
  "mcp": {
    "my-remote-server": {
      "type": "remote",
      "url": "https://mcp.example.com/",
      "timeout": 30000
    }
  }
}
```

Entries also support environment variables for local servers, headers and OAuth settings for remote servers, and reconnect options. Keep credentials in private, untracked configuration rather than a shared project file.

## Manage servers

The `codegg mcp` commands can list, add, remove, and test configured servers:

```text
codegg mcp list
codegg mcp add <NAME> --command <COMMAND> [--args <ARGS>...]
codegg mcp add <NAME> --server-type remote --url <URL>
codegg mcp debug <NAME>
codegg mcp remove <NAME>
```

The `add` command defaults to a local server. Use `--server-type remote` with `--url` for an HTTP server. `codegg mcp debug` can test a configured server or a URL supplied with `--url`.

## Tool, resource, and prompt behavior

Raw MCP tools use names such as `mcp__my-local-server__read_file`. The model-facing catalog generally exposes raw tools from configured servers. CodeGG hides raw tools for selected managed backend names unless configured otherwise; those services may instead provide a stable native wrapper. Tool visibility and permissions remain subject to CodeGG's configuration and policy. Treat tool descriptions and results as untrusted input.

MCP prompts are explicit host or user actions. The model cannot activate one on its own, and returned prompt text is treated as untrusted data. Treat tool results and resource content from any server as external input too.

## Security

Only connect servers you trust. A local server runs code on your machine. Remote endpoints are checked to reduce DNS-rebinding risk, and redirects are not followed. Review the command, URL, headers, and requested tool behavior before enabling a server.

## Source verification

Verified 2026-11-15 against `crates/codegg-config/src/schema.rs`, `src/mcp/cli.rs`, `src/mcp/mod.rs`, `src/mcp/remote.rs`, `src/security/ssrf.rs`, and `src/agent/request_preparation.rs`. Confirmed the flat configuration shape and supported fields, CLI commands and defaults, model-facing raw-tool exposure behavior, host/user prompt selection, and remote endpoint validation behavior.

Corrected 2026-11-15: the previous guide described `McpExposurePolicy::hide_all()` as the model-facing default. The current request-preparation path sets `show_raw` to true and hides selected managed backends, so ordinary configured servers' raw tools may be exposed to the model.
