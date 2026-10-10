# Configuration

CodeGG configuration is JSON with JSONC support, so `//` and `/* */` comments
are allowed. A fully commented example lives at
[`codegg.example.jsonc`](../codegg.example.jsonc) in the repository root —
copy it as a starting point and edit down.

Check a config before relying on it:

```bash
codegg validate                        # auto-detected config
codegg validate --config ./my.jsonc    # a specific file
```

To change one in your editor instead:

```bash
codegg edit                            # $VISUAL/$EDITOR, creating it if missing
codegg edit --config ./my.jsonc        # a specific file
codegg edit --project                  # the project-local config
codegg edit --editor hx                # override $VISUAL/$EDITOR
```

`edit` resolves the editor from `--editor`, then `$VISUAL`, then `$EDITOR`,
then the first of `hx`, `vim`, `vi`, `nano` on `PATH`, and re-checks the file
once the editor exits. See [`cli.md`](cli.md#edit) for the full contract.

`validate` exits 0 and prints `Configuration is valid: <path>` for the file you
passed to `--config`, and exits 1 with a message otherwise. With no `--config`
it validates the auto-detected config and prints `Configuration is valid.`
without naming a path.

## Where configuration is read from

All of these layers are loaded and **merged** — this is not first-match-wins.
They are collected in this order, and a later layer overrides an earlier one:

| Order | Location | Notes |
|---|---|---|
| 1 | `$CODEGG_TUI_CONFIG` | An explicit file path, if it exists. Merged first, so it is the **lowest** precedence. |
| 2 | System | macOS `/Library/Application Support/codegg/codegg.json`; other Unix `/etc/codegg/codegg.json`; Windows `%ProgramData%\codegg\codegg.json`. |
| 3 | Global | `<config_dir>/codegg/codegg.jsonc`, then `.json`, then `config.json`. Falls back to `<config_dir>/codegg/codegg.jsonc`. |
| 4 | Project | Walked upward from the working directory. Merged last, so it is the **highest** precedence. |

So effective precedence, highest first, is: **project → global → system →
`$CODEGG_TUI_CONFIG`**.

How the layers combine depends on the field (the full table is under
"Merge Strategies" in `architecture/config.md`): most sections combine
key-by-key, maps replace per key, and `instructions` concatenates. A section
whose fields are all non-`Option` (`security`, `provider_connections`,
`deterministic_tools`) is replaced wholesale rather than merged. Within a
key-by-key section, a nested block you specify replaces that whole block.

`<config_dir>` is the platform config directory — `~/.config` on Linux,
`~/Library/Application Support` on macOS.

Project discovery checks these four paths in each ancestor directory,
starting at the working directory and walking up:

```text
<dir>/.codegg/codegg.jsonc
<dir>/.codegg/codegg.json
<dir>/codegg/codegg.jsonc
<dir>/codegg/codegg.json
```

Because the walk goes upward, a config at the repository root applies to every
subdirectory beneath it.

## The three gotchas that break configs silently

### 1. Unknown keys are ignored, not rejected

The schema does not use `deny_unknown_fields`. A misspelled key parses
cleanly and does nothing. If a setting appears to be ignored, check the
spelling against [`architecture/config.md`](../architecture/config.md).

### 2. Several keys are untagged enums with no "object" variant

These accept a specific set of shapes. Passing the wrong one is a hard parse
error, and because the whole file fails to parse, CodeGG falls back to
defaults — so a single bad value discards your entire configuration.

| Key | Accepted shapes |
|---|---|
| `autoupdate` | `true` / `false`, or a string. **Not** an object. (Also inert — `codegg upgrade` only runs when you invoke it.) |
| `provider.<id>.timeout` | An integer number of **milliseconds**, or a boolean. `"60s"` is rejected. |
| `lsp` | `false`, or a map keyed by **server id**. |
| `formatter` | `false`, or a map keyed by **file extension**. |
| `plugin` | An array of **path strings**. |
| `permission.<tool>` | An action string (`"allow"` / `"ask"` / `"deny"`), or an object whose values are all strings. |

### 3. Several maps are keyed directly — there is no wrapper level

A nested wrapper silently registers the wrong thing instead of failing:

- `mcp` — servers are keyed directly. `"mcp": {"servers": {...}}` registers a
  server literally named `servers`.
- `lsp` — keyed directly by server id (`rust-analyzer`, `pyright`,
  `typescript-language-server`, …), not by language name.
- `formatter` — keyed directly by extension, so `{"rules": {...}}` is wrong.

## A minimal working config

```jsonc
{
  "model": "anthropic/claude-sonnet-4-20250514",
  "provider": {
    "anthropic": {
      "auth": { "type": "api_key", "env": "ANTHROPIC_API_KEY" }
    }
  }
}
```

That is enough to run a turn once `ANTHROPIC_API_KEY` is set. See
`docs/providers.md` for the credential options, including the encrypted
store.

## Semantic model routing

Semantic routing is opt-in and only reachable through an exact `virtual:<name>`
alias configured under `model_routers`. A concrete model selection bypasses
routing entirely.

```jsonc
{
  "model_routers": {
    "virtual:code": {
      "selector_model": "openai/gpt-4o-mini",
      "default_model": "openai/gpt-4o-mini",
      "routes": {
        "fast":    { "model": "openai/gpt-4o-mini", "description": "short implementation task" },
        "deep":    { "model": "anthropic/claude-opus-4-20250514", "description": "complex reasoning" }
      }
    }
  }
}
```

Then select it with `codegg -m virtual:code` or `/model virtual:code`.

How it behaves:

- CodeGG first settles the **provider connection** selected for the turn or
  session, then applies only a route whose model is compatible with that
  connection. A route can never migrate a turn to a different provider.
- If the selected connection is EggPool, EggPool remains responsible for
  account and provider routing behind its endpoint.
- If the selector fails or emits invalid output, CodeGG uses the configured
  `default_model` when that is compatible with the connection.
- Caller cancellation propagates.

Routing is model selection, not provider failover. CodeGG performs selection
per turn and keeps no local sticky affinity cache; the `sticky` and
`affinity_ttl_s` fields exist for shared policy and fingerprint compatibility
and do not pin a session to a route.

See `architecture/config.md` and `README.md` for the full contract.

## Environment variable interpolation

Any `${VAR}` sequence in a config file is replaced with that environment
variable's value before parsing, anywhere in the file:

```jsonc
{
  "provider": {
    "openai": {
      "base_url": "${OPENAI_BASE_URL}",
      "auth": { "type": "api_key", "value": "${OPENAI_API_KEY}" }
    }
  }
}
```

An unterminated `${` is emitted literally rather than swallowing the rest of
the file, and a variable that is not set expands to an empty string. Note that
the config file is read fresh on each load, so this is the supported way to
keep secrets out of the file itself — but prefer a provider's `auth.env` field,
which keeps the lookup explicit and avoids writing an expanded secret to disk.


## What else configuration controls

`codegg.example.jsonc` annotates each block. The main groups are:

| Group | Key | Guide |
|---|---|---|
| Model selection | `model`, `small_model`, `model_routers`, `model_profile` | above, and `architecture/config.md` |
| Providers | `provider`, `disabled_providers`, `enabled_providers`, `provider_connections` | `docs/providers.md` |
| MCP servers | `mcp` | `docs/MCP.md` |
| Language servers | `lsp`, `experimental.lsp_tool` | `docs/LSP.md` |
| Permissions | `permission`, `approval_reviewer` | `docs/tui.md`, `architecture/permission.md` |
| Compaction | `compaction` | `architecture/compaction.md` |
| Skills | `skills` | `docs/agents-skills.md` |
| Custom commands | `commands` | `docs/agents-skills.md` |
| Tools | `tools`, `formatter` | `docs/tools.md` |
| Plugins | `plugin`, `keybinds`, `theme`, `watcher`, `instructions` | `docs/PLUGINS.md`, `docs/themes.md` |
| Daemon | `daemon` | `docs/daemon.md` |
| HTTP server | `server` | `architecture/server.md` |

Note that the `skills` block adds two extra roots and a kill switch on top of
those fixed directories: `skills.paths` adds skills directories, each used
**directly** rather than as a parent that gets `<vendor>/skills` appended, and
`skills.enabled = false` disables discovery everywhere. See
`architecture/skills.md`.
