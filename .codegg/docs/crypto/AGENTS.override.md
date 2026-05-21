# Crypto Module Override

This file contains crypto-specific guidance and overrides root AGENTS.md.

## API Key Encryption

Provider API keys can be encrypted at rest using `CODEGG_MASTER_KEY` (preferred) or `CODEGG_ENCRYPTION_KEY`. `OPENCODE_ENCRYPTION_KEY` is also accepted for compatibility. Set `encrypted_api_key` in config and `encrypted: true`.
