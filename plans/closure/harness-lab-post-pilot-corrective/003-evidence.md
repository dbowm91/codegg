# Harness Lab Post-Pilot Corrective 003 — Verification Evidence

Branch: `plans/harness-lab-post-pilot-correctives`
Baseline reviewed: `2a9fc376707ef14dab4a2f9a27f6aba8ac00dd59`
Verified: 2026-10-21

## 1. The installed OpenCode CLI

```
$ opencode --version
1.19.3

$ opencode run --help
run opencode with a message

Positionals:
  message  message to send                                                     [array] [default: []]

Options:
  --format       format: default (formatted) or json (raw JSON events)
                                          [string] [choices: "default", "json"] [default: "default"]
  --dir          directory to run in, path on remote server if attaching                [string]
  --model        model to use in the format of provider/model                   [string]
  --agent        agent to use                                                           [string]
  --auto         auto-approve permissions that are not explicitly denied (dangerous!)
                                                                          [boolean] [default: false]
```

Every flag the harness adapter passes to the proposer — `run`, `--auto`,
`--format json`, `--dir` — is present in the installed version with an
unchanged meaning.

## 2. The version gate against the real binary

```
$ python3 -c "..."
pinned: 1.19.3
observed stdout: '1.19.3'
gate passes: True
```

Before this change the pin was `1.19.0`, so the gate refused the only
installed binary:

```
pinned: 1.19.0
observed stdout: '1.19.3'
gate passes: False
```

The refusal is now distinct and nameable. With a fake CLI reporting `1.18.9`:

```
OpenCode version preflight failed: expected 1.19.3, observed 1.18.9 from /path/to/opencode
```

Asserted by `test_an_unsupported_opencode_version_is_a_named_refusal`.

## 3. The bounded compile-correction mechanic

The mechanic was extracted into `_build_with_compile_correction` so its
bounding is directly testable. It is qualified against a **labelled fault
injection** — a fake build that fails once with a compiler-shaped error and a
fake proposer that returns a correction. This is the branch §5-C explicitly
allows for a mechanics qualification; it is not a real OpenCode-generated
compile correction and claims no production gain.

| Test | Assertion |
|---|---|
| `test_a_compile_failure_is_corrected_once_and_the_build_is_rerun` | The build is attempted twice; the correction is requested exactly once; the successful build is the recorded gate; the correction prompt identifies itself as a compile-correction retry. |
| `test_the_compile_correction_retry_is_bounded` | With two allowed retries, the build is attempted three times and the proposer twice, then the failure propagates instead of retrying forever. |
| `test_a_non_opencode_proposer_never_retries` | A manual proposer gets exactly one build attempt and no correction request. |
| `test_a_correction_that_drops_the_changelog_is_refused` | A correction that removes the required `CHANGELOG.harness.md` is refused. |

```
$ python3 -m unittest dev.harness_lab.tests.test_improve.CompileCorrectionMechanicsTests -v
test_a_compile_failure_is_corrected_once_and_the_build_is_rerun ... ok
test_a_correction_that_drops_the_changelog_is_refused ... ok
test_a_non_opencode_proposer_never_retries ... ok
test_the_compile_correction_retry_is_bounded ... ok

Ran 4 tests in 0.7s

OK
```

## 4. The full harness suite

```
$ python3 -m unittest discover -s dev/harness_lab/tests
Ran 117 tests in 4.544s

OK

$ python3 -m pyflakes dev/harness_lab/*.py dev/harness_lab/tests/*.py
(no findings)
```

## 5. What was NOT run, and why

- **No authenticated OpenCode proposal.** It needs an operator-approved model
  route and a C001 budget policy with a verifiable enforceable cap. Neither
  has been authorized, so no live provider request was made.
- **No live candidate qualification.** It needs a real defect, a real
  candidate, and a built CodeGG binary. No genuine defect is in evidence, and
  the plan forbids planting one.
- **No provider-free seeded-regression acceptance run through the full
  candidate → ledger → compare → qualify chain.** The contract-level
  acceptance wiring is proven in C002; the end-to-end chain is part of the
  blocked live work.

## 6. Workspace guards

`scripts/verify.sh quick` exits 0 — see `001-evidence.md` §5 for the guard
list and the `cargo check --workspace --all-targets --locked` result.
`cargo fmt --check --all` is clean and `cargo clippy -p codegg --all-targets
-- -D warnings` finishes with no warning.
