# Providers and Credentials

CodeGG ships with seventeen built-in LLM providers. Which ones are actually
available to you depends on your own configuration and credentials, not on
this list.

## Built-in providers

| Id | Display name | Environment variable |
|---|---|---|
| `anthropic` | Anthropic | `ANTHROPIC_API_KEY` |
| `openai` | OpenAI | `OPENAI_API_KEY` |
| `google` | Google | `GOOGLE_API_KEY` |
| `openrouter` | OpenRouter | `OPENROUTER_API_KEY` |
| `opencode_zen` | Codegg Zen | `OPENCODE_ZEN_API_KEY` |
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

This is the registration order CodeGG uses internally. The convention is
`{PROVIDER_UPPER}_API_KEY`, with one exception worth memorizing: `together`
uses `TOGETHERAI_API_KEY`.

## Checking what is actually available

These commands reflect **your** resolved configuration, not a static catalog:

```bash
codegg providers            # every provider CodeGG can currently reach
codegg models               # every model it can currently reach
codegg models -p anthropic  # one provider's models
```

`codegg providers` prints `id - Display name` per provider, and says so
plainly when nothing is configured. If a provider you expect is missing, its
credential did not resolve — the usual culprits are an unset environment
variable, a config block that has no usable key, or a stored key that needs
an account id you did not select.

For a read-only check of configured providers that makes no network calls:

```bash
codegg doctor providers
```

Expect noise on a fresh machine: provider registration logs a `WARN` line for
every built-in whose credential did not resolve, so `codegg providers` is
usually preceded by a block of `NO KEY for provider '...'` lines. That is
diagnostic output, not failure — the exit status is still 0, and only the
providers listed under "Available providers" are actually usable. Silence the
diagnostics if you want just the list:

```bash
RUST_LOG=off codegg providers
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
conventional `{PROVIDER}_API_KEY` variable, an inline `value`, and finally an
`encrypted_value` — which needs a master key to decrypt. One ordering caveat:
the OpenAI-compatible providers plus `opencode_zen` and `minimax` are
registered with their conventional variable, and that variable is checked
before an explicit `env`.

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
codegg auth status                          # ids, kinds, expiry — never secrets
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
creates one — only a secret-store write bootstraps it.

The store file itself is `<config_dir>/codegg/credentials.json`.

The important edge case is recovery. If encrypted material already exists and
no key is available, CodeGG fails closed with an error rather than generating a
replacement key. That is deliberate: silently minting a new key would make the
existing ciphertext permanently undecryptable while looking like success. If
you hit this, restore the correct environment variable or `master.key` file
from where the secret came from.

The key value is never printed, logged, written into normal config, exported
with sessions, or included in diagnostics.

## Multiple providers in config do not disable the others

A natural worry is that defining one provider in config might suppress
env-var registration for the rest. It does not. Each provider is resolved
independently — its own config block first, then its own environment
variable. The pure env-var fallback sweep only runs when config-based
registration produced nothing at all, which is a safety net rather than a
switch you will encounter.

If a provider you expect is absent, check `codegg providers` output before
changing config — the usual answer is simply that no credential resolved for
it yet.

## Typed auth modes that are not complete yet

The `auth` schema accepts four variants beyond `api_key`. Two of them parse
but do not resolve:

| Variant | Status |
|---|---|
| `api_key` | Supported. |
| `stored` | Supported — resolves against the credential store. |
| `none` | Supported — explicitly no credential. |
| `external_command` | Parses, but resolution returns an unsupported error. |
| `oauth_device` | Parses, but resolution returns an unsupported error. |

A provider configured with an unsupported mode is logged and skipped; it does
not prevent your other providers from loading. **API keys and stored keys
are the working credential paths today.**

There is a related capability distinction worth knowing. Providers marked
API-key-only — `anthropic`, `openai`, `google`, `openrouter`, `opencode_zen`,
and `minimax` — reject a stored bearer token outright, and expired stored
records fail before any network call.

## Connecting from the TUI

`/connect` is the onboarding surface inside the TUI. It loads a secret-free
provider catalog, lets you pick an entry, and collects the credential through
a masked input that is never echoed into prompt history, command text, toasts,
or debug output. The plaintext secret travels only in the trusted local core
request.

`/connections` selects which provider connection — and which model within it
— the current session uses. It never collects secrets.

In other words: use `/connect` to add a provider connection, `/connections` to
switch the current session between the ones you already added.

### What "connected" does and does not mean

A successful `/connect` proves two separate things, and the confirmation toast
tells you which of them actually happened:

| Toast | Meaning |
|---|---|
| `credential verified` | The provider itself accepted the credential during a non-billable metadata request. |
| `no credential required` | The provider needs no credential. |
| `catalog loaded … credential not yet verified` | The model catalog was loaded, but nothing has authenticated the credential yet. |

The third case is the common one and is not a warning. Most providers expose a
model catalog that does not require — or even check — your API key: some
return a fixed built-in list without touching the network at all, and some
serve `/models` publicly. Listing models therefore cannot prove a key is
valid. For those providers the credential is confirmed by the first real
request, and the connection's status updates to `verified` once a request
succeeds.

If your key is actually wrong, you will see it in two ways: `/connect` fails
immediately for providers whose catalog endpoint authenticates, and otherwise
the first real request fails with an authentication error. Either way the
connection is marked as having a rejected credential rather than silently
looking healthy.

`/connections` shows each connection's health (catalog reachable or not)
alongside its credential status, so the two can be read independently.

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

- `architecture/provider.md` — registry, setup catalog, credential resolution
  order, and connection lifecycle.
- `architecture/auth.md` — the durable authorization surface and how secrets
  are handled end to end.
- `docs/TROUBLESHOOTING.md` — "Model not found" and other user-facing failures.
- `architecture/config.md` — the full configuration schema.
