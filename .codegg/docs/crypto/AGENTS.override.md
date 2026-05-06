# Crypto Module Override

This file contains crypto-specific guidance and overrides root AGENTS.md.

## API Key Encryption

Provider API keys can be encrypted at rest using `OPENCODE_ENCRYPTION_KEY` environment variable. Set `encrypted_api_key` in config and `encrypted: true`.