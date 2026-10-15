# C049 — Broad-read compatibility decision pack

Status: decision-ready proposal; ADR-0030 remains proposed and unratified.
Baseline: CodeGG `39f149a15ef529d66787d1d11551872ea22ec384`.
Authority: ADR-0004, ADR-0016, accepted ADR-0029, proposed ADR-0030, and C042.

## Decision requested

Maintainers must explicitly accept or reject the separately named
`WorkspaceWriteBroadRead` profile proposal in ADR-0030. This packet does not
ratify it. Acceptance would authorize a future implementation plan for a
versioned public profile and policy presentation; it would not admit a Windows
backend or claim that any specific backend is secure. Rejection leaves the
current strict profiles unchanged and Windows `Required` unavailable.

## Threat and guarantee matrix

| Dimension | Strict ReadOnly / WorkspaceWrite today | WRITE_RESTRICTED disposable-root candidate | Dedicated principal / granted paths | Strict BaseContainer or guest |
|---|---|---|---|---|
| Confidential host reads | Required by the strict profile; Windows currently unavailable | **Not provided.** Ordinary user-readable host files may be read; this is not universal access to every path | Depends on explicit path ACLs and token; must be measured | Candidate goal, not established by prior CodeGG evidence |
| Writes | Profile-specific confinement; Windows currently unavailable | Candidate write checks require both normal-token access and a restricting-SID grant; only newly created stage/scratch roots may receive grants | Potentially scoped to granted paths; original-tree grants violate current no-mutation rule absent new authority | Candidate only; prior C043 had an unqualified DACL fallback |
| Delete, rename, ADS, reparse aliases | Included in strict enforcement claim, not string-path checks alone | Must be tested by handle/object identity, parent DELETE_CHILD, aliases and descendants; until then unknown | Must be tested for each granted object and alias | Must be proven on actual child and effective tier |
| Host owner/security descriptors/MIC | No unapproved mutation | No changes outside newly created disposable roots; no original-tree mutation | Original-tree grants are ineligible under current invariant | Must prove no fallback mutation before candidate re-entry |
| Child, descendants, handles, IPC | Must be contained by admitted backend | Independent test gates; a restricting SID does not revoke already-open handles or mediate IPC | Requires separate process/job/handle proof | Requires actual child, handle, descendant and teardown evidence |
| Network | Unrestricted unless independently enforced | Unrestricted; environment proxy variables do not prove egress restriction | Offline only after separately approved WFP and socket tests | Unknown unless separately qualified |
| FullHost / fallback | Explicit authorization only | Never a fallback | Never a fallback | Never a fallback |

`WRITE_RESTRICTED` adds an access check for write operations through the
restricted SID list; it does not create a read-denial boundary. A CodeGG-owned
new directory is not by itself a write boundary: the token, its restricting
SID, ACLs, aliases, handles and descendants must all be exercised.

## Proposed user and compatibility contract (if ratified)

- Keep existing `ReadOnly` and `WorkspaceWrite` meanings byte-for-byte and
  behaviorally strict across platforms. A broad-read mode has a distinct
  identity, candidate name `WorkspaceWriteBroadRead`, and must not alias or
  deserialize as either existing profile.
- Opt-in only after operator selection and an independent approval event. No
  default change, silent migration, automatic retry, or fallback from strict
  mode. `FullHost` remains a separate explicit escalation.
- Persist the selected profile in the immutable session/attempt policy
  snapshot and approval receipt, including profile version, requested
  dimensions, obtained dimensions, backend/tier, and unavailable/refusal
  reason. Audit must distinguish broad host reads from universal access.
- Parent/subagent ceiling: strict parent → broad-read child is denied before
  spawn; broad-read parent → broad-read child is allowed only if separately
  authorized; no child may request FullHost beyond its parent ceiling. A
  stricter parent cannot be weakened by a child or automatic retry.
- Old serialized strict profiles retain their current meaning. Unknown or
  unsupported profile versions fail closed and never coerce to broad-read or
  FullHost. Migration requires explicit human re-authorization.
- Describe risk plainly: “This mode may read files your Windows account can
  read, including credentials or other private host data. It does not promise
  host-secret confidentiality. Writes remain limited only to the separately
  reported, verified write boundary. Network access is unrestricted unless a
  separately verified network control is reported.”
- A capability result must report each dimension separately. Write denial,
  process Job ownership, a vendor tier label, or absent output cannot attest
  confidential host reads, IPC denial, or offline network.

## Candidate disposition

1. **Same-user WRITE_RESTRICTED plus new disposable root:** eligible for C050
   test-only study. It preserves the no-original-object-mutation rule if all
   grants are applied only at creation to synthetic roots. It cannot meet
   confidential host-read denial and is not strict `WorkspaceWrite`.
2. **Original-worktree DACL grants or low-integrity relabeling:** excluded
   under current authority. C013 recorded hardlink mutation and descriptor
   drift; this would alter user-owned security state and require separate
   explicit architecture/consent and recovery design.
3. **Dedicated principal with path grants:** not selected. Existing-tree grants
   still mutate host descriptors; persistent identity, provisioning and
   recovery need separate authority. C052 may compare launch context but does
   not authorize provisioning.
4. **MXC BaseContainer / guest strict path:** no admission. C043/C044 are
   negative; C053 may re-open one bounded test only after a versioned API
   delta satisfies both its fallback and launch/lifecycle gates. A guest
   remains a separate architecture decision.

## C042 contract and test additions for any later profile implementation

C042's internal `RequestedGuarantees` / `AttestedCapabilities` remains the
negotiation authority. A new profile would need an explicit versioned
serialization and conversion contract; broad-read cannot map to
`confidential_host_reads=true`, and an absent capability is a refusal, not a
downgrade. Add compatibility tests for old profile decoding/round trips,
strict-parent → broad-child denial, automatic retry denial, no implicit
FullHost fallback, per-dimension requested/obtained audit, and unsupported
backend/version refusal before target entry. No code or public enum changes
are authorized by C049.

## Evidence basis and limits

- OpenAI's primary design note describes the two write checks for a
  WRITE_RESTRICTED token and separately describes its dedicated-account
  elevated design: <https://openai.com/index/building-codex-windows-sandbox/>.
- Microsoft's MXC schema describes `fallback.allowDaclMutation` as defaulting
  to true and controlling a host-DACL fallback; the current Rust SDK/API must
  be checked independently before any candidate claim:
  <https://github.com/microsoft/mxc/blob/main/docs/schema.md>.
- CodeGG evidence remains C011/C013/C014/C038/C043/C044 and C042. See
  `plans/closure/windows-native-sandbox/{011,013,014,038,043,044}-status.md`
  and `src/security/sandbox/guarantee.rs`. None proves a public broad-read
  profile or production Windows backend.
- C050, C052 and C053 are separate bounded test-only gates. C051 additionally
  requires positive C050 results. C045/C046/C047 integration/C048 are not
  unblocked by this dossier.

## Maintainer decision record

Decision owner: CodeGG maintainers.

Requested response: **accept or reject ADR-0030's separately named broad-read
profile direction**, with any required name/threat-model changes recorded in
ADR-0030 before a public implementation plan is registered. Until that
decision is recorded, the profile, UI, config, protocol, and backend remain
unimplemented and Windows `SandboxRequest::Required` remains unavailable.
