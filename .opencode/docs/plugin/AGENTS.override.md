# Plugin Module Override

This file contains plugin-specific guidance and overrides root AGENTS.md.

## WASM Sandboxing

WASM plugins are fuel-bounded. Fuel is tracked per-plugin via `ModuleCache`. Unused fuel is returned after hook execution via `return_fuel()` which initializes entries with `MAX_PLUGIN_FUEL_BUDGET`.