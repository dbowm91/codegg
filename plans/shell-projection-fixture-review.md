# Shell Projection Fixture Corpus Review

## Current State

19 fixtures across 6 categories (`tests/fixtures/shell_projection/`):

| Category | Fixtures | What they cover |
|----------|----------|-----------------|
| generic | 5 | long output, mixed streams, spawn_failed, timeout |
| git | 3 | diff small, status clean, status dirty |
| js | 1 | typescript_error (stderr) |
| python | 1 | pytest_failure (stderr) |
| redaction | 5 | fake_api_key, fake_env_vars, false_positive_prose, long_line_cred, multi_cred_one_line |
| rust | 4 | cargo_check (success/1 error/multi error), cargo_test_panic |

Harness: `shell_projection_harness.rs` (884 lines, 8 tests) + `shell_projection_phase10.rs` (1057 lines, 21 synthetic tests)

## What's Missing

### Redaction rules with no fixture coverage (4 of 6 rules missing)

| Rule | Has fixture? | Has unit tests? |
|------|-------------|-----------------|
| AuthorizationRule | YES (3 fixtures) | YES |
| EnvSecretRule | YES (1 fixture) | YES |
| **PemBlockRule** | **NO** | YES |
| **CloudCredentialRule** | **NO** | YES |
| **EmbeddedCredentialUrlRule** | **NO** | YES |
| **SessionMaterialRule** | **NO** | YES |

### Native projectors with no fixture coverage

| Projector | Fixtures | Gap |
|-----------|----------|-----|
| CargoCheckProjector | 3 (success/1 error/multi error) | Covered |
| CargoTestProjector | 1 (panic) | **Missing: all-pass test** |
| GitStatusProjector | 2 (clean/dirty) | Covered |
| GitDiffProjector | 1 (small) | Covered |
| GitLogProjector | 0 | **Missing entirely** |

### Missing edge cases

- Signal exit (`CommandExit::Signal`)
- Cancelled exit (`CommandExit::Cancelled`)
- Binary/non-UTF-8 output
- Unicode output (emoji, CJK)
- Partial output (`OutputCompleteness::Partial`)
- Very large output (100KB+) testing store capping

## Plan: Add 12 New Fixtures

### Batch 1: Missing redaction rules (5 fixtures)

1. **`redaction/pem_block`** — RSA private key block in stdout
   - Command: `cat server.key`
   - Stream: stdout
   - `must_redact`: the PEM body content
   - `must_contain`: `-----BEGIN RSA PRIVATE KEY-----`, `-----END RSA PRIVATE KEY-----`
   - Tests: PemBlockRule end-to-end

2. **`redaction/aws_credentials`** — AWS access key + secret key
   - Command: `aws sts get-caller-identity`
   - Stream: stdout
   - Content: JSON with `AccessKeyId: AKIAIOSFODNN7EXAMPLE` and `aws_secret_access_key=wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY`
   - `must_redact`: both the access key ID and the secret key value
   - Tests: CloudCredentialRule (AWS path)

3. **`redaction/gcp_service_account`** — GCP service account JSON
   - Command: `cat service-account.json`
   - Stream: stdout
   - Content: JSON blob with `"private_key": "-----BEGIN RSA PRIVATE KEY-----\nMIIE..."`
   - `must_redact`: the private key value
   - Tests: CloudCredentialRule (GCP path)

4. **`redaction/embedded_cred_url`** — Git clone with embedded credentials
   - Command: `git clone https://ghp_abc123def456ghijklmn@github.com/user/repo`
   - Stream: stdout (clone output)
   - `must_redact`: `ghp_abc123def456ghijklmn`
   - `must_contain`: `github.com/user/repo`
   - Tests: EmbeddedCredentialUrlRule

5. **`redaction/session_cookie`** — Cookie and CSRF token
   - Command: `curl -v https://example.com`
   - Stream: stdout
   - Content: HTTP headers including `Set-Cookie: session=abc123def456ghi789` and body containing `csrf_token=xK9mN2pL5qR8sT1uV4wY7zA3bC6dE9fG`
   - `must_redact`: session value, csrf token
   - Tests: SessionMaterialRule

### Batch 2: Missing native projector fixtures (3 fixtures)

6. **`git/git_log_recent`** — Git log output
   - Command: `git log --oneline -5`
   - Stream: stdout
   - Content: 5 recent commits
   - `must_contain`: commit hashes, commit messages
   - Tests: GitLogProjector fixture path

7. **`rust/cargo_clippy_warning`** — Clippy warnings only
   - Command: `cargo clippy`
   - Stream: stderr
   - Content: 2-3 clippy warnings (no errors)
   - `must_contain`: `warning:` lines
   - Tests: CargoCheckProjector warnings-only path

8. **`rust/cargo_test_all_pass`** — All tests passing
   - Command: `cargo test`
   - Stream: stdout
   - Content: test results summary with 0 failures
   - `must_contain`: `test result: ok`, `passed`
   - Tests: CargoTestProjector success path

### Batch 3: Edge cases (4 fixtures)

9. **`generic/signal_exit`** — Process killed by signal
   - Command: `sleep 300` (killed by SIGKILL)
   - Stream: stdout
   - `exit_state`: `"signal"`
   - Content: empty stdout, empty stderr
   - Tests: CommandExit::Signal path

10. **`generic/unicode_output`** — Non-ASCII content
    - Command: `python3 -c "print('日本語テスト 🎉 café résumé')"`
    - Stream: stdout
    - Content: mixed Unicode (CJK, emoji, accented Latin)
    - `must_contain`: the Unicode characters verbatim
    - Tests: UTF-8 handling edge cases

11. **`generic/empty_output_success`** — Empty stdout on success
    - Command: `true`
    - Stream: stdout
    - `exit_code`: 0
    - Content: empty
    - Tests: minimal-output success path (distinct from timeout_exit which has exit_state)

12. **`python/pytest_all_pass`** — All tests passing
    - Command: `pytest`
    - Stream: stderr
    - Content: `==== 15 passed in 0.42s ====`
    - `must_contain`: `passed`
    - Tests: success-summary path for Python projectors (if any) or raw fallback

## File Format

Each fixture follows the existing pattern:
- `<name>.toml` — metadata (fixture info + expect assertions)
- `<name>.stdout` or `<name>.stderr` — raw output content

## Harness Updates

The existing harness tests automatically pick up new fixtures via `discover_fixtures()`. No harness changes needed for basic coverage. However:

1. Add a `test_all_redaction_rules_have_fixtures` test that asserts all 6 rule names appear in at least one fixture's `must_redact` list
2. Add a `test_exit_state_variants_have_fixtures` test that checks all exit states (timeout, spawn_failed, signal, cancelled, code) appear in fixtures

## Verification

After adding fixtures:
```bash
cargo test --test shell_projection_harness -- --nocapture  # verify all fixtures parse and project
cargo test --test shell_projection_phase10                   # synthetic tests still pass
cargo test -p codegg --lib shell::redactor                  # unit tests still pass
cargo test -p codegg --lib shell::rtk                       # RTK unit tests still pass
```

Check the metrics report output for the new fixtures to confirm token reduction is reasonable.

## Files to Create/Modify

**Create (24 files):**
- `tests/fixtures/shell_projection/redaction/pem_block.{toml,stdout}`
- `tests/fixtures/shell_projection/redaction/aws_credentials.{toml,stdout}`
- `tests/fixtures/shell_projection/redaction/gcp_service_account.{toml,stdout}`
- `tests/fixtures/shell_projection/redaction/embedded_cred_url.{toml,stdout}`
- `tests/fixtures/shell_projection/redaction/session_cookie.{toml,stdout}`
- `tests/fixtures/shell_projection/git/git_log_recent.{toml,stdout}`
- `tests/fixtures/shell_projection/rust/cargo_clippy_warning.{toml,stderr}`
- `tests/fixtures/shell_projection/rust/cargo_test_all_pass.{toml,stdout}`
- `tests/fixtures/shell_projection/generic/signal_exit.{toml,stdout}`
- `tests/fixtures/shell_projection/generic/unicode_output.{toml,stdout}`
- `tests/fixtures/shell_projection/generic/empty_output_success.{toml,stdout}`
- `tests/fixtures/shell_projection/python/pytest_all_pass.{toml,stderr}`

**Modify (1 file):**
- `tests/shell_projection_harness.rs` — add 2 new invariant tests

**No changes needed to:**
- `architecture/testing.md`
- `AGENTS.md`
- `.codegg/skills/testing/SKILL.md`
- Projector or redactor source code
