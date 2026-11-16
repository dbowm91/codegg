# LSP (Language Server Protocol)

CodeGG can connect to language servers for diagnostics, navigation, hover information, completion, formatting, rename, and code actions. The implementation lives in `crates/egglsp/`; `src/lsp/` is a thin re-export shim. See `architecture/lsp.md` for the module contract.

## Install and configure a language server

CodeGG resolves a server command from `PATH` and its LSP cache. The built-in `rust-analyzer` definition includes a download specification. Other language servers must be installed separately or configured with an explicit command.

Add server settings to your CodeGG config. The `lsp` object is keyed directly by server id, and `command` is an array containing the executable and its arguments:

```jsonc
{
  "lsp": {
    "rust-analyzer": {
      "command": ["rust-analyzer"]
    },
    "typescript-language-server": {
      "command": ["typescript-language-server", "--stdio"]
    }
  }
}
```

A server rule can also set `extensions`, `env`, `initialization`, `workspace_configuration`, or `restart`. Set `"disabled": true` on one rule to disable that server. Leaving an id out of the map uses its built-in definition.

Common built-in server ids include `rust-analyzer`, `pyright`, `typescript-language-server`, `gopls`, and `clangd`.

## Let the model use LSP

The model-facing `lsp` tool is hidden by default. To expose it, add this setting to your config:

```jsonc
{
  "experimental": {
    "lsp_tool": true
  }
}
```

This setting exposes the tool; it does not install a language server. CodeGG's LSP tool can then use the configured service for navigation, hover, diagnostics, and code actions.

## Editor reads and previews

Editor reads such as hover, navigation, symbols, signature help, formatting preview, and rename preview are warm-only. They do not start a language server. If no server is running for the workspace, `NotReady` means there is no result yet, not that the server found nothing. If more than one server could handle a file and its extension does not select one, `Ambiguous` reports the candidates instead of choosing one silently.

Formatting and rename produce previews. Review the proposed change before applying it. The checked apply path refuses to overwrite unsaved edits or a file that changed after the preview was created. Code actions that include a command are not applied, because CodeGG does not execute LSP commands from that review surface.

## Source verification

Verified 2026-11-15 against `crates/egglsp/src/server.rs`, `crates/egglsp/src/config.rs`, `crates/egglsp/src/service.rs`, `crates/egglsp/src/operations/`, `src/lsp/`, `src/tool/lsp.rs`, `src/main.rs`, `crates/codegg-config/src/schema.rs`, and `crates/egglsp/src/mutation.rs`. Confirmed server resolution and download metadata, configuration fields, the default-off `experimental.lsp_tool` exposure gate, warm-only editor reads, preview application checks, and refusal to execute command-bearing code actions.
