# Git Agent Integration Phase E — Network, Configuration, and Destructive Operations

## Objective

Extend the unified Git service to operations that contact remotes, mutate repository configuration, rewrite refs/history, or discard worktree/index data. These operations require stronger policy, noninteractive execution guarantees, credential redaction, explicit force semantics, and realistic remote integration tests.

## Dependencies

Phases A-D must be complete. The typed operation model, Git-specific routing, snapshots, mutation executor, permissions, and structured outcomes must already be in production shape.

## Required deliverables

### 1. Network execution policy

Define a dedicated policy for Git network operations.

Requirements:

- require the existing `Network` capability;
- identify remote name and sanitized endpoint before permission where possible;
- set bounded network timeouts;
- disable terminal credential prompts;
- preserve approved credential-helper, SSH-agent, and certificate behavior deliberately;
- prevent editor/pager spawning;
- redact credentials, embedded tokens, signed URLs, and sensitive headers from logs/results;
- distinguish DNS/connect/authentication/authorization/ref-rejection/timeout failures where possible;
- retain raw diagnostics after redaction.

Document environment variables and config sources intentionally allowed to affect execution.

### 2. Fetch

Implement structured fetch requests:

- named remote;
- selected refspecs;
- prune/prune-tags options;
- tags policy;
- depth/unshallow where modeled;
- dry-run where supported;
- no arbitrary upload-pack command;
- report updated/deleted/rejected refs and FETCH_HEAD impact.

Fetch is network read plus local ref mutation. Permission and result types should reflect both.

### 3. Pull

Model pull as an explicit policy-controlled composite operation rather than an opaque convenience command where feasible.

Required behavior:

- resolve remote/branch/upstream;
- expose strategy: fast-forward-only, merge, or rebase;
- capture pre-operation snapshot;
- require clean-state or explicit supported dirty-state handling;
- execute noninteractively;
- report fetched refs, integration result, new HEAD, and conflicts;
- never silently choose a history strategy contrary to repository/user configuration;
- permit managed fallback for unsupported advanced options.

Consider implementing as fetch plus explicit merge/rebase to improve observability, but only if semantics match Git configuration and failure behavior. Otherwise execute `git pull` through the Git service and report the limitation.

### 4. Push

Implement structured push requests:

- remote and refspecs;
- set-upstream;
- tags/follow-tags;
- delete remote ref;
- dry-run;
- atomic where supported;
- normal, force-with-lease, and plain force as distinct types;
- explicit expected lease value where available;
- report per-ref accepted, rejected, up-to-date, deleted, and forced updates.

Policy defaults:

- normal push: ask;
- remote branch deletion: strong confirmation;
- force-with-lease: strong confirmation with expected remote state;
- plain force: deny by default;
- push to protected/default branch may require elevated confirmation when detectable;
- pushing credentials in URL is rejected/redacted.

Do not infer success solely from exit code when machine-readable porcelain output can provide per-ref detail.

### 5. Remote management

Implement typed repository-local operations:

- list/show/get-url as reads;
- add;
- remove;
- rename;
- set-url/add-url/delete-url;
- set-head where safely modeled;
- prune through fetch semantics.

Sanitize URLs before display and storage. Reject configuration outside the repository unless a separate explicit scope is supported.

### 6. Git configuration mutation

Support only an allowlisted repository-local configuration surface initially.

Potential keys:

- branch upstream relationships;
- pull strategy;
- merge/rebase preferences;
- selected Codegg-related repository-local settings.

Requirements:

- `--local` scope enforced;
- global/system/worktree scope denied unless separately designed;
- sensitive credential/helper/url keys excluded or elevated;
- old/new value returned with secrets redacted;
- arbitrary `git config` remains managed fallback with conservative policy or denied.

### 7. Reset

Model reset modes explicitly:

- soft;
- mixed;
- merge;
- keep;
- hard;
- path-scoped index reset/unstage.

Policy:

- path-scoped index reset can be safe/allowed;
- soft/mixed ref movement asks and reports affected commits/index;
- merge/keep asks with worktree analysis;
- hard is destructive and denied by default;
- all modes capture before/after HEAD, index/worktree status, and recoverability hints.

Do not conflate reset used for unstaging with history rewrite.

### 8. Clean

Implement dry-run preview as a read operation and destructive clean as an explicit request.

Requirements:

- preview exact candidate paths first;
- model files, directories, ignored files, and nested repositories separately;
- require force acknowledgement;
- deny by default for `-x`, nested repositories, or broad unscoped cleanup;
- allow path-scoped cleanup only after explicit preview and permission;
- return deleted paths and final status;
- avoid parsing locale-sensitive prose where machine-readable alternatives or precomputed candidate enumeration can be used.

### 9. Forced ref deletion and history rewrite

Apply destructive-history policy to:

- `branch -D`;
- force tag replacement/deletion where relevant;
- force push;
- hard reset moving HEAD;
- rebase of published branches when upstream information indicates risk.

Permission prompts must include old/new oids where known and whether reflog/remote recovery is expected.

### 10. Managed Git argv policy

For unsupported network/destructive commands:

- retain Git-specific managed argv tier;
- conservatively classify capabilities before permission;
- deny unknown force/destructive options by default;
- never fall through to raw shell merely to bypass Git policy;
- raw shell remains appropriate only when shell syntax itself is required.

### 11. Network/destructive projectors

Add concise outputs for:

- fetched refs;
- pull strategy and resulting HEAD;
- pushed/rejected refs;
- force mode;
- remote/config changes;
- reset movement and affected state;
- clean preview/deletions;
- recovery notes;
- redaction markers.

### 12. Configuration surface

Expose user configuration for default network permissions and destructive-operation policy only where it maps cleanly to existing permission architecture. Defaults must remain conservative.

## Likely files

- Git network executor/modules;
- remote, push/fetch/pull request/result types;
- permission and policy modules;
- environment/redaction utilities;
- reset/clean/config operation modules;
- projectors and RunStore metadata;
- configuration schema/documentation;
- local bare-remote integration fixtures.

## Test infrastructure

Use local temporary bare repositories to avoid external network dependence.

Test:

- fetch new/update/delete refs;
- pull fast-forward, ff-only rejection, merge, rebase, conflict;
- push create/update/delete/upstream;
- non-fast-forward rejection;
- force-with-lease success and stale-lease rejection;
- plain force policy denial;
- remote URL redaction;
- auth/prompt prevention using controlled failing helpers;
- timeout and subprocess termination;
- local remote management;
- local config allowlist and global/system denial;
- reset modes and state deltas;
- clean preview and scoped deletion;
- ignored/nested repository clean protections;
- native/Bash equivalence;
- managed fallback conservatism;
- RunStore redaction and risk metadata.

## Validation

Run network/destructive integration tests serially where needed, plus permission, routing, redaction, projection, RunStore, and full Git service suites. Verify no test contacts the public network.

## Exit criteria

Phase E is complete when:

- fetch, pull, push, remote management, selected local config, reset, and clean use the Git service;
- network operations cannot block for terminal input;
- force modes are explicit and policy-distinct;
- credentials and sensitive URLs are redacted;
- destructive operations preview or report affected state and are denied by default where appropriate;
- local bare-remote tests cover accepted and rejected ref updates;
- unsupported advanced operations cannot bypass Git-specific policy through generic managed execution.

## Handoff to Phase F

Phase F should complete conflict/recovery workflows, agent/TUI ergonomics, documentation, compatibility cleanup, and closure testing across the entire integrated subsystem.