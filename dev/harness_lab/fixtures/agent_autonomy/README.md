# Agent autonomy benchmark taxonomy (C004 work package A)

This directory holds the **versioned, opt-in** case taxonomy for the sustained
agent benchmark. It is the frozen contract only: the loader in
`dev/harness_lab/autonomy.py` parses and validates it, and the harness never
runs a verifier from here.

- Manifest: `cases.jsonl` — one JSON object per line, UTF-8, no comments.
- Schema version: `1` (`autonomy.AUTONOMY_SCHEMA_VERSION`).
- Loader: `dev/harness_lab/autonomy.py`.
- Offline tests: `dev/harness_lab/tests/test_autonomy.py`.

C004 as a whole stays **conditionally closed** — its strict closure needs a
live-provider authorization that has not been granted. Everything in this
directory is the part that is deliverable without a provider.

## Families

| Family | Cases | Support |
|---|---|---|
| `read_search_patch` | `read-search-patch-rust`, `read-search-patch-python` | supported |
| `compile_debug` | `compile-debug-rust` | supported |
| `background_job` | `background-job-wait` | supported |
| `repeated_tool_calls` | `repeated-distinct-tool-calls`, `repeated-identical-failure-stall` (protected control) | supported |
| `delegation` | `delegated-nonconflicting-workstreams` | **deferred** |
| `integration` | `mcp-lsp-integration-isolation` | **deferred** |

A deferred case is a legitimate outcome, not a failure: the family depends on a
CodeGG feature that is not closed and live-capable in this branch, so it is
recorded with a `deferral_reason` and is never scored. `require_supported()`
refuses it by name.

The `repeated_tool_calls` group deliberately holds two cases: a task whose
calls are all **distinct** (it must not be stopped as a repeated-call stall)
and a **protected negative control** whose identical repeated failure must
stop the trial as a stall. Both share one `group_id`.

## Loading and checking

```bash
python3 -m unittest discover -s dev/harness_lab/tests -v
```

```python
from pathlib import Path
from dev.harness_lab.autonomy import (load_autonomy_suite, require_supported,
                                      suite_digest, verify_isolation)

suite = load_autonomy_suite()
assert suite.suite_sha256 == suite_digest()      # content addressed
verify_isolation(suite, ("src/", "crates/"))     # candidate may not reach a verifier
case = require_supported(suite.case("compile-debug-rust"))
```

`load_autonomy_suite(root)` accepts the repository root or a directory that
directly holds `cases.jsonl`. `suite_digest(root)` hashes the manifest bytes
without parsing them, so any edit to the manifest changes the digest.

## Verifier isolation

Every case names a `verifier_scope`: the path prefix its hidden verifier lives
under. The fixtures and hidden verifiers are **not** inside this package. They
are reserved under `dev/autonomy_fixtures/<case_id>/`, outside the harness
tree, so a candidate patch cannot reach them:

- `dev/harness_lab/` — the harness itself (`improve.check_scope` forbids a
  candidate from changing it).
- `plans/` — planning and closure records.
- `dev/autonomy_fixtures/` — the frozen task fixtures and hidden verifiers.

`verify_isolation(suite, candidate_editable_prefixes)` refuses a suite whose
`verifier_scope` falls under any prefix the candidate may edit. A scope that is
*exactly* a candidate-editable prefix is a violation too, not only a scope
nested below one. Run it with the same `--allow-prefix` list the candidate
patch was admitted under, before any trial starts.

## Verifier bodies

The manifest freezes where and how each verifier runs: the `verifier.command`
argv, its `cwd`, and the `verifier_scope` it lives under. **One case is
materialized today** and is the reference implementation of the pattern:

- `read-search-patch-python` — fixture under
  `dev/autonomy_fixtures/read-search-patch-python/fixture/`, independent
  verifier under `.../verifier/`. The verifier re-derives the contract itself
  instead of trusting the fixture's own test, and checks that the frozen files
  the agent was given were not edited.

The remaining cases specify their verifier contract but do **not** yet ship a
verifier body. `support: "supported"` therefore means *specified,
provider-free and network-free* — not *materialized*. The five unmaterialized
verifier bodies are the next concrete authoring step, alongside the live runs.

The materialized case's two properties are proven by
`dev/harness_lab/tests/test_autonomy.py`:

- the verifier **fails** while the task is unsolved — a verifier that passed on
  the broken baseline would prove nothing;
- the verifier **passes** once the defect is repaired;
- the verifier **refuses** an edited frozen file, so weakening the provided test
  is not a way to pass.

A verifier written to pass trivially is a defect, not a pass.

## Rules the loader enforces

The loader rejects, never repairs:

- an unsupported `schema_version`, a missing required field, an unknown field;
- a duplicate `case_id`, a family outside the six supported families;
- a `support` mode outside `{"supported", "deferred"}`;
- a deferred case without a non-empty `deferral_reason`, a supported case
  carrying one;
- a non-boolean `protected`, an empty `allowed_tools`;
- a non-positive or non-integer `max_provider_requests`, `max_tool_calls` or
  `max_wallclock_seconds`; a negative, non-finite or boolean `max_cost_usd`;
- a `network` value that is neither `"none"` nor an object with a non-empty
  `endpoints` list — **a missing `network` field is an error**, because the
  plan requires an explicit disclosure and "no network" is never inferred;
- a verifier without a real argv `command` list, an unknown verifier `kind`,
  or an empty `cwd`.

`network` is `"none"` for every case in this manifest. A case that needs
internet must disclose it as `{"endpoints": [...], "reason": "..."}`; no case
here speculates about endpoints it does not use.
