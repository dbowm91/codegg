# Providers and Credentials

CodeGG includes built-in LLM providers. Which ones are available to you
depends on your configuration and resolved credentials.

## Built-in providers

| Id | Display name | Environment variable |
|---|---|---|
| `anthropic` | Anthropic | `ANTHROPIC_API_KEY` |
| `openai` | OpenAI | `OPENAI_API_KEY` |
| `google` | Google | `GOOGLE_API_KEY` |
| `openrouter` | OpenRouter | `OPENROUTER_API_KEY` |
| `opencode_zen` | OpenCode Zen | `OPENCODE_ZEN_API_KEY` |
| `mistral` | Mistral | `MISTRAL_API_KEY` |
| `groq` | Groq | `GROQ_API_KEY` |
| `deepinfra` | DeepInfra | `DEEPINFRA_API_KEY` |
| `cerebras` | Cerebras | `CEREBRAS_API_KEY` |
| `cohere` | Cohere | `COHERE_API_KEY` |
| `together` | Together AI | `TOGETHERAI_API_KEY` |
| `perplexity` | Perplexity | `PERPLEXITY_API_KEY` |
| `xai` | xAI | `XAI_API_KEY` |
| `venice` | Venice | `VENICE_API_KEY` |
| `minimax` | MiniMax | `MINIMAX_API_KEY` |
| `opencode_go` | OpenCode Go | `OPENCODE_GO_API_KEY` |
| `generalcompute` | GeneralCompute | `GENERALCOMPUTE_API_KEY` |

Environment variables generally follow `{PROVIDER_UPPER}_API_KEY`. One
exception is `together`, which uses `TOGETHERAI_API_KEY`.

### OpenCode Go model support

CodeGG chooses OpenCode Go's endpoint from a reviewed model mapping. Models
without a mapping are not selectable; CodeGG does not guess an endpoint from a
model name. If an expected model is missing, the mapping needs an update. You
do not need to configure the endpoint or authentication header.

## Checking what is actually available

These commands reflect your configured providers and stored credentials:

```bash
codegg providers            # every provider CodeGG can currently reach
codegg models               # every model it can currently reach
codegg models -p anthropic  # one provider's models
```

`codegg providers` lists providers whose credentials resolved. It does not
verify them with a request. If a provider is missing, check its environment
variable, config block, or selected stored account.

For a read-only check of configured providers that makes no network calls:

```bash
codegg doctor providers
```

To see stored account metadata without exposing secrets:

```bash
codegg auth status
```

## Three ways to supply a credential

### 1. Environment variable

Simplest, and it keeps the secret out of any file:

```bash
export ANTHROPIC_API_KEY='...'
codegg providers
```

Each provider independently resolves its own conventional variable, so
setting one never disables the others.

### 2. The config file

Add a `provider` block keyed by provider id. Config is JSON with JSONC
comments; discovery walks `CODEGG_TUI_CONFIG`, then the system location, then
the global config, then project config from the working directory upward.

The modern shape is a typed `auth` block:

```jsonc
{
  "provider": {
    "anthropic": {
      "auth": {
        "type": "api_key",
        "env": "MY_ANTHROPIC_KEY"   // optional env var name override
      }
    }
  }
}
```

`auth` is optional. Without it, CodeGG checks the conventional environment
variable and then the legacy `api_key` / `encrypted_api_key` fields.

Resolution order for an `api_key` block is: the explicit `env` name, the
conventional `{PROVIDER}_API_KEY` variable, an inline `value`, then an
`encrypted_value` that needs a master key. Only Anthropic, OpenAI, Google, and
OpenRouter honor a custom `env` name. Other built-in providers always check
their conventional variable, so set that variable instead of using `auth.env`.

Prefer an environment variable or the credential store over an inline
`value`, which is plaintext in a file on disk.

### 3. The encrypted credential store

The most appropriate option for a key you do not want in your shell history or
your config file:

```bash
printf '%s' "$ANTHROPIC_API_KEY" | codegg auth set-key anthropic
```

`set-key` reads the key from stdin, never from an argument, so it does not
land in your shell history or in the process table. Pass `--account <id>` to
keep more than one account for the same provider. Provider and account ids
accept only `[A-Za-z0-9_-]`.

Reference a stored key from config with:

```jsonc
{
  "provider": {
    "openai": {
      "auth": { "type": "stored", "account_id": "work" }
    }
  }
}
```

Inspect and remove entries with:

```bash
codegg auth status                          # ids, kinds, expiry; never secrets
codegg auth logout openai                   # one provider
codegg auth logout openai --account work    # one account
codegg auth logout openai --account '*'     # every account for that provider
```

`auth status` deliberately prints no plaintext, no ciphertext, and no
fingerprint.

## How the master key works

The encrypted store needs a master key. CodeGG resolves it in this order:

1. `CODEGG_MASTER_KEY`
2. `CODEGG_ENCRYPTION_KEY`
3. `OPENCODE_ENCRYPTION_KEY`
4. a CodeGG-managed key file at `<config_dir>/codegg/master.key`

**You do not need to set any of these environment variables.** On a fresh
profile, the first protected write creates the managed key automatically: 32
CSPRNG bytes, hex-encoded, written to a `0o600` file. Reading a key never
creates one. Only a secret-store write bootstraps it.

The store file itself is `<config_dir>/codegg/credentials.json`.

The important edge case is recovery. If encrypted material already exists and
no key is available, CodeGG fails closed with an error rather than generating a
replacement key. That is deliberate: silently minting a new key would make the
existing ciphertext permanently undecryptable while looking like success. If
you hit this, restore the correct environment variable or `master.key` file
from where the secret came from.

The key value is never printed, logged, written into normal config, exported
with sessions, or included in diagnostics.

## Supported authentication modes

The `auth` schema accepts four variants beyond `api_key`. Two of them parse
but do not resolve:

| Variant | Status |
|---|---|
| `api_key` | Supported. |
| `stored` | Supported. Resolves against the credential store. |
| `none` | Supported. Explicitly uses no credential. |
| `external_command` | Parses, but resolution returns an unsupported error. |
| `oauth_device` | Parses, but resolution returns an unsupported error. |

A provider configured with an unsupported mode is logged and skipped; it does
not prevent your other providers from loading. **API keys and stored keys
are the working credential paths today.**

Some providers accept API keys only: `anthropic`, `openai`, `google`,
`openrouter`, `opencode_zen`, and `minimax`. They reject stored bearer tokens.
Expired stored credentials also fail before a network request.

## Model discovery

CodeGG has no universal bundled model catalog. `codegg models` asks each
configured provider adapter for its catalog. Depending on the provider, this
may come from a remote endpoint or a local list. Some adapters also accept
operator-declared `models` entries, which supplement discovery.

A provider's model list may be public or locally generated, so it does not
always verify that your credential works. If discovery fails, check the error
and your provider setup. Run `codegg doctor providers` to inspect configured
providers without a network probe.

Providers that use CodeGG's reviewed shared profile require a reviewed wire
mapping for each selectable model. Generic OpenAI-compatible providers use
their configured route and model IDs instead. If a catalog model cannot be
selected, check whether its provider uses the reviewed profile and has a
mapping for that model.

## Connecting from the TUI

`/connect` lets you choose a provider and enter a credential in a masked
field. The secret is not echoed into prompt history, command text, toasts, or
debug output. Use `/connections` to choose the provider connection and model
for the current session. That command does not collect credentials.

### Catalog status and credential status

A successful `/connect` reports catalog reachability separately from
credential verification. A loaded model catalog does not always prove the key
works. Many providers return a local or public catalog; in those cases, the
first real request verifies the credential. `/connections` shows catalog
health and credential status independently.

## Choosing a model

`--model` accepts a `provider/model-id` string. Prefer that form: it is
unambiguous and works regardless of what else is configured.

```bash
codegg -m anthropic/claude-sonnet-4-20250514
```

A bare model id is also accepted, but on the one-shot and `exec` paths the
provider portion defaults to `openai` when no `/` is present, so a bare id is
not portable. Spell out the provider.

Semantic routing is opt-in and separate. It activates only through an exact
`virtual:<name>` alias you configure under `model_routers`; concrete model ids
bypass it entirely.

```bash
codegg providers
codegg models -p anthropic
codegg -m anthropic/<model-id>
```

## See also

- `architecture/provider.md`: registry, credential resolution, and connection lifecycle.
- `architecture/auth.md`: credential handling and authorization.
- `docs/TROUBLESHOOTING.md`: model discovery and other common failures.
- `architecture/config.md`: the full configuration schema.

## Source verification

Checked 2026-11-23 against `crates/codegg-providers/src/provider_core.rs`, `crates/codegg-providers/src/auth_types.rs`, `crates/codegg-providers/src/setup_catalog.rs`, `crates/codegg-providers/src/discovery.rs`, `crates/codegg-config/src/encryption.rs`, `crates/codegg-cli/src/main.rs`, and `crates/codegg-tui/src/command.rs`.

Corrected: only Anthropic, OpenAI, Google, and OpenRouter honor a custom `auth.env`; other built-in providers use their conventional variable. Reviewed-profile models need a reviewed model mapping, while generic OpenAI-compatible providers use their configured route.
