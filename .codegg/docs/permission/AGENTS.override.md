# Permission Module Override

This file contains permission-specific guidance and overrides root AGENTS.md.

## Mode System

Specialized permission configurations via `src/permission/modes.rs`:

- `review` - read-heavy, no edit/bash
- `debug` - bash allowed, limited edit
- `docs` - edit/read allowed, no bash

Configure via `mode:` in config.yaml.