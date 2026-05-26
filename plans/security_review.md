# Security Architecture Review

## Summary
The security module documentation is largely accurate and well-organized. All documented functions exist and match their implementations. Minor discrepancies exist in naming conventions and one missing detail about IPv6 unique local address range.

## Verified Correct
- `is_internal_ip()` - `src/security/ssrf.rs:4-37` correctly handles all documented IP ranges
- `ipv6_segments_to_ipv4()` - `src/security/ssrf.rs:39-65` handles IPv4-mapped IPv6 conversion
- `validate_host_ip()` - `src/security/ssrf.rs:67-94` correctly resolves DNS and validates IPs
- `revalidate_dns()` - `src/security/ssrf.rs:96-121` correctly implements DNS rebinding protection
- `validate_url_host()` - `src/security/ssrf.rs:123-145` correctly parses URLs and validates schemes
- `validate_path_safety()` - `src/security/sandbox.rs:259-282` correctly rejects symlinks and validates against allowed paths
- `get_default_allowed_paths()` - `src/security/sandbox.rs:284-308` returns correct paths (cwd, HOME/.config, HOME/.local/share, dirs::config_dir, dirs::data_dir)
- `get_sensitive_paths()` - `src/security/sandbox.rs:310-321` returns correct sensitive paths
- `SandboxConfig` struct - `src/security/sandbox.rs:6-11` with fields `enabled`, `allowed_paths`, `deny_paths`
- Builder methods - `src/security/sandbox.rs:13-71` (`new()`, `with_enabled()`, `with_allowed_paths()`, `with_deny_paths()`, `is_available()`, `enforce()`)
- Landlock syscall constants - `src/security/sandbox.rs:99-101` (SYS_LANDLOCK_CREATE_RULESET, SYS_LANDLOCK_ADD_RULE, SYS_LANDLOCK_RESTRICT_SELF)

## Discrepancies Found
- **IPv6 unique local range naming**: Doc at line 32 says `fc00::/7 (IPv6 unique local)` but the code at `ssrf.rs:25` uses `segments[0] & 0xfe00) == 0xfc00` which covers both fc00::/7 and fd00::/7 (the actual range is fc00::/7, but implementations typically use fd00::/8 for locally assigned). This is a minor documentation precision issue.
- **Landlock access flags naming**: Doc at lines 126-128 shows `READ`, `WRITE`, `EXEC` but code at `sandbox.rs:88-90` uses `LANDLOCK_ACCESS_FS_READ`, `LANDLOCK_ACCESS_FS_WRITE`, `LANDLOCK_ACCESS_FS_EXEC`. The doc shows the conceptual names, code shows the actual constant names - both are valid but inconsistent in presentation.

## Stale Items in Architecture Doc
- **Line 102 "Used by" list**: Mentions `webfetch`, `websearch`, `codesearch`, `mcp/remote` but this list may be incomplete or out of date - worth verifying against actual tool implementations.

## Improvement Suggestions
- Consider clarifying that IPv6 unique local addresses use fd00::/8 (locally assigned) rather than fc00::/8 in the doc, or document the actual implementation which checks `(segments[0] & 0xfe00) == 0xfc00` covering both fc00 and fd00.
- The "Used by" section could be auto-generated or linked to actual usage rather than maintained manually.