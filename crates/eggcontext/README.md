# eggcontext

Token counting and context packing primitives for Rust tools.

`eggcontext` provides deterministic token accounting that can be tested
without booting any host application.

## Two layers

- **Deterministic tokenizer layer** (stable): pick a `TokenizerType`
  explicitly and count with `count_with_tokenizer` or
  `estimate_for_tokenizer`. `Cl100kBase` and `O200kBase` run the public
  tiktoken BPE encoding for those vocabularies exactly; `Claude` and
  `Gemini` run `cl100k_base` and apply a documented per-family
  multiplier, so they are heuristic. This layer never parses model
  names.
- **Volatile model-name policy layer** (convenience, replaceable):
  `TokenizerType::for_model` maps a model-name hint to a
  `TokenizerType`, and `estimate_tokens_sync` / `estimate_tokens` /
  `estimate_with_provenance` combine that mapping with the deterministic
  layer. Treat the mapping and multipliers as policy: pin or replace
  `for_model` when exact accounting matters.

## Approximation

Counts for Claude and Gemini are upper-bound estimates, not vendor-exact
tokenization. `TokenEstimate::approximate` reports whether a count was
exact BPE or heuristic. Never present heuristic counts as universally
exact vendor tokenization.
