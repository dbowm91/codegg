---
name: documentation
description: Keeping AGENTS.md, README.md, docs/, architecture/, and the skills index accurate against source in codegg
version: 1.0.0
tags:
  - documentation
  - agents-md
  - docs
  - architecture
---

# Documentation Maintenance Guide

Operational guide for changing prose in this repository. Source code is the only
evidence; every other document is a claim that has to be re-derived.

## Authority order

When two documents disagree, the higher entry wins — but fix the lower one anyway,
because a reader will only ever see it.

1. **Source code** — the only ground truth.
2. `architecture/<module>.md` — the authoritative module contract.
3. `AGENTS.md` — the agent-facing index and invariant list.
4. `.opencode/skills/<name>/SKILL.md` — the on-demand module guide.
5. `docs/` — user-facing guidance.
6. `README.md` — quickstart entry point and `docs/` index.

`plans/` is a separate system: it is a live control surface plus historical
evidence, never a place to restate code facts.

## The core discipline

Verify before you assert. A file path, symbol name, line number, count, flag,
env var, config key, or command is a factual claim and costs one `grep` to check.

- **Counts** live once, in `architecture/overview.md`'s `## Verified counts`
  table, with a `Source` column. Verify against that `Source`; never copy a
  number from another document or another session's notes.
- **Line numbers** are verified or dropped. Prefer a symbol reference
  (`fn foo` in `path/to/file.rs`) when a line range will drift.
- **Untraceable claims get deleted**, not softened or guessed. A removed
  sentence is cheaper than a plausible wrong one.
- When you correct something, record what was wrong and the evidence. These
  files carry a `## Source verification` section; add to it rather than
  silently overwriting, so the same mistake is not re-derived later.

## Configuration is a trap

Several config keys are untagged enums with `#[serde(default)]` and no
`deny_unknown_fields`. A wrong shape either hard-fails the whole file or is
**silently ignored**, and ignored keys read as "my setting does nothing".

Two distinct failure modes, both seen in this repo:

- **Hard failure** — the wrong shape makes serde error, discarding the *entire*
  config file. Example: `lsp.servers.<id>.command` as a string instead of an
  object.
- **Silent no-op** — the block parses as something valid but wrong. Example: a
  `permission` block using allow/deny arrays when permission is per-tool; an
  `mcp` block nesting servers under a `servers` key, which registers one phantom
  server named `servers`.

So: validate any config snippet you write or change.

```bash
codegg validate --config codegg.example.jsonc   # the annotated example
```

Keep `codegg.example.jsonc` valid. When adding a key there, verify the real
schema in `crates/codegg-config/src/schema.rs` — not a neighbouring key's shape.

## Cross-references

- Between documents, use backticked repo-relative paths
  (`` `architecture/skills.md` ``), **not** `../../`-style relative markdown
  links. Those break whenever a doc moves.
- A `SKILL.md` names its authoritative `architecture/` doc in its intro and in
  `## See Also`. When a module contract changes, update the skill and its
  architecture doc **together** — they are two views of one contract.
- `AGENTS.md`'s skills index maps each skill to its architecture doc. A new skill
  needs a row there or agents will not find it.

## Generated and pinned content

Some prose is produced and must not be hand-edited:

```bash
python3 scripts/generate_builtin_agents.py            # regenerate built-in agents
python3 scripts/generate_builtin_agents.py --check    # CI mode; fails if stale
```

`src/agent/builtins/generated.rs` is derived from `assets/agents/*.toml` and
`assets/prompts/agents/`. Edit the sources and regenerate.

Some content is deliberately frozen. **Do not "refresh" it:**

- `docs/validation/` — historical closure records whose line numbers are pinned
  to the SHAs they describe. Their staleness is the point.
- `plans/closure/`, `plans/archive/`, and closed rows in `plans/registry.md` —
  immutable evidence. Some contain broken cross-references to renamed
  neighbours; that drift is historical, and guessing a replacement is worse
  than leaving it.
- `README.md`'s "no GitHub release has been published yet" callout is a
  measured fact, not boilerplate. Re-check it before assuming it still holds.

## Verifying a change

```bash
scripts/verify.sh quick    # fmt, guard scripts, workspace check
```

Guard scripts under `scripts/check_*` encode invariants that prose cannot. If a
document describes a boundary, a guard should enforce it — when you find prose
describing an unenforced invariant, either add the guard or say plainly that it
is unenforced.

Check that links and referenced files still resolve after editing. A reference to
a deleted file is the most common way this work introduces a new defect; that is
why superseded example configs and duplicated docs get pruned in the same pass
that adds guidance.

## See Also

- `architecture/overview.md` — module map and the `## Verified counts` table
- `architecture/testing.md` — the test taxonomy referenced by the guard commands
- `.skills/architecture-review/SKILL.md` — the batch process for auditing
  `architecture/` against source
- `AGENTS.md` — the agent-facing index this skill helps keep honest
