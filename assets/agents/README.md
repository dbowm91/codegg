# Built-in Agent Definitions

This directory contains TOML definitions for compiled built-in agents.

Each file defines one agent's metadata and permissions. Prompt text lives in
`../prompts/agents/`. Contract docs live in `../prompts/contracts/`.

**Do not edit generated Rust files directly.** Edit these TOML sources and
re-run `python3 scripts/generate_builtin_agents.py` to regenerate.

See `scripts/AGENTS.md` or the plan doc for the full schema reference.
