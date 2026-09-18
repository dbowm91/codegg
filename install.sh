#!/bin/sh
# install.sh — verified CodeGG installer for supported Linux/macOS hosts.
#
# Maps the current host to the stable managed-runfile GitHub release assets,
# downloads over a fixed HTTPS origin, verifies the selected archive against
# the release SHA-256 manifest, extracts a controlled payload, and atomically
# installs the managed runfile bundle into a user-writable directory.
#
# The bundle installs three installation-owned siblings into one canonical
# directory: `codegg`, `codegg-sandbox-helper`, and `codegg-eggsearch`
# (plus the fixed `THIRD-PARTY-NOTICES.txt` when the release carries it).
# The end user invokes only `codegg`; the helpers are resolved by CodeGG
# relative to its own executable, never from PATH.
#
# Usage (latest release):
#   curl -fsSL https://raw.githubusercontent.com/dbowm91/codegg/main/install.sh | sh
#
# Usage (pinned version):
#   curl -fsSL https://raw.githubusercontent.com/dbowm91/codegg/main/install.sh | CODEGG_VERSION=0.1.1 sh
#
# Prefer inspecting before executing remote code:
#   curl -fsSL https://raw.githubusercontent.com/dbowm91/codegg/main/install.sh -o install.sh
#   sh install.sh
#
# Environment interface (the only supported configuration surface):
#   CODEGG_VERSION       optional; default latest release.
#                        Bare "0.1.1" or "v0.1.1" (normalized to tag "v0.1.1").
#   CODEGG_INSTALL_DIR   optional; default "$HOME/.local/bin".
#                        Empty means the default. The managed runfiles are
#                        always installed as "<dir>/codegg",
#                        "<dir>/codegg-sandbox-helper",
#                        "<dir>/codegg-eggsearch" (plus "<dir>/THIRD-PARTY-NOTICES.txt"
#                        when the release carries the fixed notice).
#
# Guarantees:
#   - Release origin is hard-coded below and is never built from environment.
#     There is intentionally NO CODEGG_DOWNLOAD_URL / mirror override: such a
#     variable would turn the installer into arbitrary remote-code execution
#     by configuration.
#   - SHA-256 is verified before extraction or execution of the payload.
#   - The existing installed runfiles are not replaced until the new
#     artifact has passed download, checksum, extraction, payload,
#     executable, and version checks.
#   - All three runfiles commit as one bundle: staged replacements are
#     renamed into place only after every payload check passes, with
#     backup/rollback so a failure replacing the second or third runfile
#     cannot leave a mixed old/new installation.
#   - Default destination is user-local. The script never invokes privilege
#     escalation, never edits shell profiles, never starts services, and
#     never installs Rust/Cargo.
#   - Unsupported OS/architecture fails before any download with Cargo/source
#     alternatives.
#   - No GitHub API token is required (plain release download URLs only).
#   - A checksum mismatch or missing asset never triggers an automatic source
#     compilation fallback.
#
# Test seam (NOT a user configuration surface):
#   _CODEGG_INSTALLER_TEST_RELEASE_DIR, when set to a local directory shaped
#   like a release (codegg-<target>.tar.gz + checksums.txt), serves the two
#   fixed filenames by local copy instead of HTTPS download. It cannot change
#   the production origin, filenames, or verification order, and normal
#   pipe-to-shell runs never set it. It exists only so
#   scripts/release/test-installer.sh can verify this script offline.
#   CODEGG_INSTALL_LIB_ONLY=1 stops before main() so the test harness can
#   source this file and unit-test the mapping/URL/checksum functions
#   without touching the network or the filesystem.
#
# POSIX sh compatible (macOS /bin/sh, Linux dash). No bash-only syntax.

set -eu

# --- Fixed release origin (never constructed from environment) ----------------
CODEGG_INSTALLER_ORIGIN="https://github.com/dbowm91/codegg"
CODEGG_INSTALLER_RUNFILES="codegg codegg-sandbox-helper codegg-eggsearch"
CODEGG_INSTALLER_NOTICE="THIRD-PARTY-NOTICES.txt"
CODEGG_INSTALLER_CHECKSUM_FILE="checksums.txt"

# Globals owned by main() and the EXIT/HUP/INT/TERM cleanup below.
codegg_installer_tmpdir=""
codegg_installer_stage=""
codegg_installer_stage_helper=""
codegg_installer_stage_egg=""
codegg_installer_stage_notice=""
codegg_installer_backup_codegg=""
codegg_installer_backup_helper=""
codegg_installer_backup_egg=""
codegg_installer_backup_notice=""
codegg_installer_dest_dir_global=""
codegg_installer_have_bkp_codegg=0
codegg_installer_have_bkp_helper=0
codegg_installer_have_bkp_egg=0
codegg_installer_have_bkp_notice=0

codegg_installer_fail() {
    printf 'error: %s\n' "$1" >&2
    exit 1
}

codegg_installer_usage() {
    cat <<'USAGE'
Usage: sh install.sh [--help]

Installs the CodeGG managed runfile bundle for supported Linux/macOS hosts.

Environment:
  CODEGG_VERSION       optional; default latest; e.g. 0.1.1 or v0.1.1
  CODEGG_INSTALL_DIR   optional; default $HOME/.local/bin

The installer verifies the release SHA-256 manifest before extracting and
installs codegg, codegg-sandbox-helper, and codegg-eggsearch (plus
THIRD-PARTY-NOTICES.txt when present) atomically into one directory
without privilege escalation, shell profile edits, or daemon/service
management.
USAGE
}

# Remove private temp state. Safe to run repeatedly; no-ops when idle.
# Destination staging files are always safe to delete. Backup files are
# restored when they hold the only surviving copy of a destination entry
# (backup-phase failure before commit); otherwise they are removed.
# Committed runfiles are never deleted here.
codegg_installer_cleanup() {
    if [ -n "$codegg_installer_stage" ] && [ -e "$codegg_installer_stage" ]; then
        rm -f -- "$codegg_installer_stage"
    fi
    if [ -n "$codegg_installer_stage_helper" ] && [ -e "$codegg_installer_stage_helper" ]; then
        rm -f -- "$codegg_installer_stage_helper"
    fi
    if [ -n "$codegg_installer_stage_egg" ] && [ -e "$codegg_installer_stage_egg" ]; then
        rm -f -- "$codegg_installer_stage_egg"
    fi
    if [ -n "$codegg_installer_stage_notice" ] && [ -e "$codegg_installer_stage_notice" ]; then
        rm -f -- "$codegg_installer_stage_notice"
    fi
    if [ -n "$codegg_installer_backup_codegg" ] && [ -e "$codegg_installer_backup_codegg" ]; then
        if [ -n "$codegg_installer_dest_dir_global" ] \
            && [ ! -e "$codegg_installer_dest_dir_global/codegg" ] \
            && [ ! -L "$codegg_installer_dest_dir_global/codegg" ]; then
            mv -- "$codegg_installer_backup_codegg" "$codegg_installer_dest_dir_global/codegg" 2>/dev/null || rm -rf -- "$codegg_installer_backup_codegg"
        else
            rm -f -- "$codegg_installer_backup_codegg"
        fi
    fi
    if [ -n "$codegg_installer_backup_helper" ] && [ -e "$codegg_installer_backup_helper" ]; then
        if [ -n "$codegg_installer_dest_dir_global" ] \
            && [ ! -e "$codegg_installer_dest_dir_global/codegg-sandbox-helper" ] \
            && [ ! -L "$codegg_installer_dest_dir_global/codegg-sandbox-helper" ]; then
            mv -- "$codegg_installer_backup_helper" "$codegg_installer_dest_dir_global/codegg-sandbox-helper" 2>/dev/null || rm -rf -- "$codegg_installer_backup_helper"
        else
            rm -f -- "$codegg_installer_backup_helper"
        fi
    fi
    if [ -n "$codegg_installer_backup_egg" ] && [ -e "$codegg_installer_backup_egg" ]; then
        if [ -n "$codegg_installer_dest_dir_global" ] \
            && [ ! -e "$codegg_installer_dest_dir_global/codegg-eggsearch" ] \
            && [ ! -L "$codegg_installer_dest_dir_global/codegg-eggsearch" ]; then
            mv -- "$codegg_installer_backup_egg" "$codegg_installer_dest_dir_global/codegg-eggsearch" 2>/dev/null || rm -rf -- "$codegg_installer_backup_egg"
        else
            rm -f -- "$codegg_installer_backup_egg"
        fi
    fi
    if [ -n "$codegg_installer_backup_notice" ] && [ -e "$codegg_installer_backup_notice" ]; then
        rm -rf -- "$codegg_installer_backup_notice"
    fi
    if [ -n "$codegg_installer_tmpdir" ] && [ -d "$codegg_installer_tmpdir" ]; then
        rm -rf -- "$codegg_installer_tmpdir"
    fi
}

# Map raw `uname -s` / `uname -m` spellings to an M001 target triple.
# Prints the target on stdout; exits nonzero with no output when unsupported.
codegg_installer_map_target() {
    codegg_installer_map_s="$1"
    codegg_installer_map_m="$2"
    codegg_installer_map_os=""
    case "$codegg_installer_map_s" in
        Linux) codegg_installer_map_os="linux" ;;
        Darwin) codegg_installer_map_os="darwin" ;;
        *) return 1 ;;
    esac
    codegg_installer_map_arch=""
    case "$codegg_installer_map_m" in
        x86_64 | amd64) codegg_installer_map_arch="x86_64" ;;
        aarch64 | arm64) codegg_installer_map_arch="aarch64" ;;
        *) return 1 ;;
    esac
    case "$codegg_installer_map_os/$codegg_installer_map_arch" in
        linux/x86_64) printf '%s\n' "x86_64-unknown-linux-gnu" ;;
        linux/aarch64) printf '%s\n' "aarch64-unknown-linux-gnu" ;;
        darwin/x86_64) printf '%s\n' "x86_64-apple-darwin" ;;
        darwin/aarch64) printf '%s\n' "aarch64-apple-darwin" ;;
        *) return 1 ;;
    esac
}

# Validate and normalize CODEGG_VERSION. Prints the bare version (no leading
# "v"), or nothing for the empty input (latest). Exits nonzero when invalid.
# Grammar: strict dotted numerics with an optional conservative suffix, so
# the value cannot escape the canonical release URL structure.
codegg_installer_normalize_version() {
    codegg_installer_nv_raw="$1"
    if [ -z "$codegg_installer_nv_raw" ]; then
        return 0
    fi
    case "$codegg_installer_nv_raw" in
        v*) codegg_installer_nv_raw="${codegg_installer_nv_raw#v}" ;;
    esac
    if [ -z "$codegg_installer_nv_raw" ]; then
        return 1
    fi
    case "$codegg_installer_nv_raw" in
        -*) return 1 ;;
    esac
    if printf '%s' "$codegg_installer_nv_raw" | grep -Eq '^[0-9]+\.[0-9]+\.[0-9]+([.+_-][A-Za-z0-9.+_-]*)?$'; then
        printf '%s\n' "$codegg_installer_nv_raw"
    else
        return 1
    fi
}

# Print the M001 asset basename for a supported target triple.
codegg_installer_asset_for_target() {
    case "$1" in
        x86_64-unknown-linux-gnu | aarch64-unknown-linux-gnu | x86_64-apple-darwin | aarch64-apple-darwin)
            printf 'codegg-%s.tar.gz\n' "$1"
            ;;
        *) return 1 ;;
    esac
}

# Print the fixed-origin download URL for a release filename.
# $1 = filename (asset basename or checksums.txt), $2 = normalized version
# or empty (latest). The origin is hard-coded; the version is re-validated
# defensively so callers cannot inject URL structure.
codegg_installer_download_url() {
    codegg_installer_du_file="$1"
    codegg_installer_du_version="$2"
    case "$codegg_installer_du_file" in
        "" | */*) return 1 ;;
    esac
    if [ -z "$codegg_installer_du_version" ]; then
        printf '%s/releases/latest/download/%s\n' "$CODEGG_INSTALLER_ORIGIN" "$codegg_installer_du_file"
    else
        codegg_installer_du_checked=""
        if ! codegg_installer_du_checked="$(codegg_installer_normalize_version "$codegg_installer_du_version")"; then
            return 1
        fi
        if [ "$codegg_installer_du_checked" != "$codegg_installer_du_version" ]; then
            return 1
        fi
        printf '%s/releases/download/v%s/%s\n' "$CODEGG_INSTALLER_ORIGIN" "$codegg_installer_du_version" "$codegg_installer_du_file"
    fi
}

# Print lowercase hex SHA-256 of a regular file to stdout.
codegg_installer_sha256_file() {
    if command -v sha256sum >/dev/null 2>&1; then
        sha256sum -- "$1" | awk '{print $1}'
    elif command -v shasum >/dev/null 2>&1; then
        shasum -a 256 -- "$1" | awk '{print $1}'
    else
        return 1
    fi
}

# Fail unless every tool the installer needs is present. Runs before any
# download or destination mutation so missing tooling is a clean error.
codegg_installer_require_commands() {
    codegg_installer_missing=""
    for codegg_installer_cmd in uname curl tar mktemp mkdir chmod mv cp rm grep awk; do
        if ! command -v "$codegg_installer_cmd" >/dev/null 2>&1; then
            codegg_installer_missing="$codegg_installer_missing $codegg_installer_cmd"
        fi
    done
    if [ -n "$codegg_installer_missing" ]; then
        printf 'error: missing required command(s):%s\n' "$codegg_installer_missing" >&2
        return 1
    fi
    if ! command -v sha256sum >/dev/null 2>&1 && ! command -v shasum >/dev/null 2>&1; then
        printf 'error: no SHA-256 tool found (need sha256sum or shasum -a 256)\n' >&2
        return 1
    fi
}

# Fetch one release file. Production path is fixed-origin HTTPS via curl.
# The test-only seam serves the same fixed filenames from a local directory.
codegg_installer_download() {
    codegg_installer_dl_name="$1"
    codegg_installer_dl_url="$2"
    codegg_installer_dl_dest="$3"
    case "$codegg_installer_dl_name" in
        "" | */*) printf 'error: unsafe download filename\n' >&2; return 1 ;;
    esac
    if [ -n "${_CODEGG_INSTALLER_TEST_RELEASE_DIR:-}" ]; then
        # TEST-ONLY path (see header): local copy of a fixed filename.
        if [ -f "$_CODEGG_INSTALLER_TEST_RELEASE_DIR/$codegg_installer_dl_name" ]; then
            cp -- "$_CODEGG_INSTALLER_TEST_RELEASE_DIR/$codegg_installer_dl_name" "$codegg_installer_dl_dest" || return 1
        else
            printf 'error: test fixture missing: %s\n' "$codegg_installer_dl_name" >&2
            return 1
        fi
    else
        curl -fsSL --proto '=https' --tlsv1.2 --connect-timeout 20 --max-time 300 \
            -o "$codegg_installer_dl_dest" -- "$codegg_installer_dl_url" || return 1
    fi
}

# Verify the archive at $1 against the manifest at $2, selecting exactly the
# entry whose basename equals $3 (exact equality, never substring/regex).
# The manifest is parsed as data: unsafe entries anywhere in the file fail
# the whole verification. Prints "checksum ok" on success.
codegg_installer_verify_checksum() {
    codegg_installer_vc_archive="$1"
    codegg_installer_vc_manifest="$2"
    codegg_installer_vc_expected="$3"
    if [ ! -f "$codegg_installer_vc_archive" ] || [ -L "$codegg_installer_vc_archive" ]; then
        printf 'error: downloaded archive is not a regular file\n' >&2
        return 1
    fi
    if [ ! -f "$codegg_installer_vc_manifest" ] || [ -L "$codegg_installer_vc_manifest" ]; then
        printf 'error: downloaded checksum manifest is not a regular file\n' >&2
        return 1
    fi
    codegg_installer_vc_matches=0
    codegg_installer_vc_expected_hash=""
    codegg_installer_vc_line=""
    while IFS= read -r codegg_installer_vc_line || [ -n "$codegg_installer_vc_line" ]; do
        codegg_installer_vc_line="$(printf '%s' "$codegg_installer_vc_line" | tr -d '\r')"
        codegg_installer_vc_hash="${codegg_installer_vc_line%% *}"
        codegg_installer_vc_rest="${codegg_installer_vc_line#"$codegg_installer_vc_hash"}"
        if [ "$codegg_installer_vc_hash" = "$codegg_installer_vc_line" ]; then
            printf 'error: malformed checksum manifest line\n' >&2
            return 1
        fi
        if [ "${#codegg_installer_vc_hash}" -ne 64 ]; then
            printf 'error: malformed checksum manifest line (hash must be 64 hex chars)\n' >&2
            return 1
        fi
        case "$codegg_installer_vc_hash" in
            *[!0-9a-f]*)
                printf 'error: malformed checksum manifest line (hash must be lowercase hex)\n' >&2
                return 1
                ;;
        esac
        case "$codegg_installer_vc_rest" in
            " "*) codegg_installer_vc_rest="${codegg_installer_vc_rest# }" ;;
            *)
                printf 'error: malformed checksum manifest line (separator)\n' >&2
                return 1
                ;;
        esac
        case "$codegg_installer_vc_rest" in
            " "*) codegg_installer_vc_rest="${codegg_installer_vc_rest# }" ;;
            "*"*) codegg_installer_vc_rest="${codegg_installer_vc_rest#\*}" ;;
            *)
                printf 'error: malformed checksum manifest line (expected two-column form)\n' >&2
                return 1
                ;;
        esac
        codegg_installer_vc_name="$codegg_installer_vc_rest"
        case "$codegg_installer_vc_name" in
            "" | */* | *\\* | *..* | -*)
                printf 'error: unsafe checksum manifest entry\n' >&2
                return 1
                ;;
        esac
        if printf '%s' "$codegg_installer_vc_name" | grep -q '[[:space:]]'; then
            printf 'error: unsafe checksum manifest entry\n' >&2
            return 1
        fi
        if [ "$codegg_installer_vc_name" = "$codegg_installer_vc_expected" ]; then
            codegg_installer_vc_matches=$((codegg_installer_vc_matches + 1))
            codegg_installer_vc_expected_hash="$codegg_installer_vc_hash"
        fi
    done <"$codegg_installer_vc_manifest"
    if [ "$codegg_installer_vc_matches" -eq 0 ]; then
        printf 'error: checksum manifest has no entry for %s\n' "$codegg_installer_vc_expected" >&2
        return 1
    fi
    if [ "$codegg_installer_vc_matches" -gt 1 ]; then
        printf 'error: checksum manifest has duplicate entries for %s\n' "$codegg_installer_vc_expected" >&2
        return 1
    fi
    codegg_installer_vc_actual=""
    if ! codegg_installer_vc_actual="$(codegg_installer_sha256_file "$codegg_installer_vc_archive")"; then
        printf 'error: unable to compute SHA-256 of downloaded archive\n' >&2
        return 1
    fi
    if [ "$codegg_installer_vc_actual" != "$codegg_installer_vc_expected_hash" ]; then
        printf 'error: checksum mismatch for %s\n' "$codegg_installer_vc_expected" >&2
        printf '  expected: %s\n' "$codegg_installer_vc_expected_hash" >&2
        printf '  actual:   %s\n' "$codegg_installer_vc_actual" >&2
        return 1
    fi
    printf 'checksum ok: %s\n' "$codegg_installer_vc_expected"
}

# Validate the archive member listing before extraction. Requires exactly
# the managed runfile set (`codegg`, `codegg-sandbox-helper`,
# `codegg-eggsearch`) plus optionally the fixed THIRD-PARTY-NOTICES.txt;
# rejects absolute paths, traversal, symlinks, devices, duplicates, and
# unexpected payloads. Never extracts.
codegg_installer_check_archive_members() {
    codegg_installer_am_members=""
    if ! codegg_installer_am_members="$(tar -tzf "$1" 2>/dev/null)"; then
        printf 'error: cannot list archive members\n' >&2
        return 1
    fi
    codegg_installer_am_count="$(printf '%s\n' "$codegg_installer_am_members" | wc -l | tr -d ' ')"
    if [ "$codegg_installer_am_count" != "3" ] && [ "$codegg_installer_am_count" != "4" ]; then
        printf 'error: archive must contain 3 runfiles (+ optional notice), found %s\n' "$codegg_installer_am_count" >&2
        return 1
    fi
    codegg_installer_am_seen=""
    codegg_installer_am_member=""
    for codegg_installer_am_member in $codegg_installer_am_members; do
        case "$codegg_installer_am_member" in
            /*)
                printf 'error: archive has absolute member: %s\n' "$codegg_installer_am_member" >&2
                return 1
                ;;
        esac
        case "$codegg_installer_am_member" in
            *..* | *\\*)
                printf 'error: archive has traversal member: %s\n' "$codegg_installer_am_member" >&2
                return 1
                ;;
        esac
        codegg_installer_am_norm="$codegg_installer_am_member"
        case "$codegg_installer_am_norm" in
            ./*) codegg_installer_am_norm="${codegg_installer_am_norm#./}" ;;
        esac
        case "$codegg_installer_am_norm" in
            "" | */*)
                printf 'error: archive member must be a top-level file: %s\n' "$codegg_installer_am_member" >&2
                return 1
                ;;
        esac
        case " $codegg_installer_am_seen " in
            *" $codegg_installer_am_norm "*)
                printf 'error: archive has duplicate member: %s\n' "$codegg_installer_am_member" >&2
                return 1
                ;;
        esac
        codegg_installer_am_seen="$codegg_installer_am_seen $codegg_installer_am_norm"
        codegg_installer_am_ok=0
        # shellcheck disable=SC2086
        for codegg_installer_am_want in $CODEGG_INSTALLER_RUNFILES; do
            if [ "$codegg_installer_am_norm" = "$codegg_installer_am_want" ]; then
                codegg_installer_am_ok=1
                break
            fi
        done
        if [ "$codegg_installer_am_ok" -eq 0 ] && [ "$codegg_installer_am_norm" = "$CODEGG_INSTALLER_NOTICE" ]; then
            codegg_installer_am_ok=1
        fi
        if [ "$codegg_installer_am_ok" -eq 0 ]; then
            printf 'error: archive has unexpected member: %s (expected runfiles + optional %s)\n' "$codegg_installer_am_member" "$CODEGG_INSTALLER_NOTICE" >&2
            return 1
        fi
    done
    # shellcheck disable=SC2086
    for codegg_installer_am_want in $CODEGG_INSTALLER_RUNFILES; do
        case " $codegg_installer_am_seen " in
            *" $codegg_installer_am_want "*) ;;
            *)
                printf 'error: archive missing required runfile: %s\n' "$codegg_installer_am_want" >&2
                return 1
                ;;
        esac
    done
    if [ "$codegg_installer_am_count" = "4" ]; then
        case " $codegg_installer_am_seen " in
            *" $CODEGG_INSTALLER_NOTICE "*) ;;
            *)
                printf 'error: archive has 4 members but no %s\n' "$CODEGG_INSTALLER_NOTICE" >&2
                return 1
                ;;
        esac
    fi
    codegg_installer_am_verbose_all=""
    if ! codegg_installer_am_verbose_all="$(tar -tvzf "$1" 2>/dev/null)"; then
        printf 'error: cannot inspect archive members\n' >&2
        return 1
    fi
    case "$codegg_installer_am_verbose_all" in
        *" -> "*)
            printf 'error: archive member is a symlink\n' >&2
            return 1
            ;;
    esac
    # Every verbose entry must be a regular file (leading '-').
    codegg_installer_am_vline=""
    while IFS= read -r codegg_installer_am_vline; do
        [ -n "$codegg_installer_am_vline" ] || continue
        case "$codegg_installer_am_vline" in
            -*) ;;
            *)
                printf 'error: archive member is not a regular file: %s\n' "$codegg_installer_am_vline" >&2
                return 1
                ;;
        esac
    done <<EOF
$codegg_installer_am_verbose_all
EOF
}

# Validate `codegg --version` output. $1 = output, $2 = expected normalized
# version or empty (latest: any well-formed CodeGG version is accepted).
codegg_installer_check_version_output() {
    codegg_installer_vo_out="$1"
    codegg_installer_vo_expected="$2"
    case "$codegg_installer_vo_out" in
        codegg\ *) ;;
        *)
            printf 'error: unexpected version output: %s\n' "$codegg_installer_vo_out" >&2
            return 1
            ;;
    esac
    if ! printf '%s' "$codegg_installer_vo_out" | grep -Eq '[0-9]+\.[0-9]+'; then
        printf 'error: version output has no version number: %s\n' "$codegg_installer_vo_out" >&2
        return 1
    fi
    if [ -n "$codegg_installer_vo_expected" ]; then
        case "$codegg_installer_vo_out" in
            *"$codegg_installer_vo_expected"*) ;;
            *)
                printf 'error: version mismatch: expected %s, got: %s\n' "$codegg_installer_vo_expected" "$codegg_installer_vo_out" >&2
                return 1
                ;;
        esac
    fi
}

# Validate `codegg-eggsearch --version` output. The sidecar must identify
# as eggsearch and carry a dotted version; the release-time pin is enforced
# by packaging/verification, while the installer rejects obvious identity
# mismatches without hard-coding a version here.
codegg_installer_check_eggsearch_output() {
    codegg_installer_eo_out="$1"
    case "$codegg_installer_eo_out" in
        *eggsearch* | *Eggsearch* | *EGGSEARCH*) ;;
        *)
            printf 'error: unexpected eggsearch version output: %s\n' "$codegg_installer_eo_out" >&2
            return 1
            ;;
    esac
    if ! printf '%s' "$codegg_installer_eo_out" | grep -Eq '[0-9]+\.[0-9]+'; then
        printf 'error: eggsearch output has no version number: %s\n' "$codegg_installer_eo_out" >&2
        return 1
    fi
}

codegg_installer_unsupported() {
    printf 'error: unsupported OS/architecture: %s / %s\n' "$1" "$2" >&2
    cat >&2 <<'ALTERNATIVES'
CodeGG publishes prebuilt binaries for:
  x86_64-unknown-linux-gnu, aarch64-unknown-linux-gnu,
  x86_64-apple-darwin, aarch64-apple-darwin

Alternatives for other hosts (deliberate choice, never automatic):
  git clone https://github.com/dbowm91/codegg.git
  cd codegg
  cargo install --locked --path .
ALTERNATIVES
    exit 1
}

codegg_install_main() {
    for codegg_installer_arg in "$@"; do
        case "$codegg_installer_arg" in
            -h | --help)
                codegg_installer_usage
                return 0
                ;;
            *)
                printf 'error: unexpected argument: %s\n' "$codegg_installer_arg" >&2
                codegg_installer_usage >&2
                return 1
                ;;
        esac
    done

    # Required tools before any network access or destination mutation.
    codegg_installer_require_commands || exit 1

    # Host mapping before any network access: unsupported hosts fail here.
    codegg_installer_uname_s="$(uname -s)"
    codegg_installer_uname_m="$(uname -m)"
    codegg_installer_target=""
    if ! codegg_installer_target="$(codegg_installer_map_target "$codegg_installer_uname_s" "$codegg_installer_uname_m")"; then
        codegg_installer_unsupported "$codegg_installer_uname_s" "$codegg_installer_uname_m"
    fi

    # Explicit version input is validated before URL construction.
    codegg_installer_version_input="${CODEGG_VERSION:-}"
    codegg_installer_version=""
    if ! codegg_installer_version="$(codegg_installer_normalize_version "$codegg_installer_version_input")"; then
        printf 'error: invalid CODEGG_VERSION (want latest, empty, or e.g. 0.1.1): %s\n' "$codegg_installer_version_input" >&2
        exit 1
    fi

    # Destination: exact configured path only, treated as path data.
    codegg_installer_dest_dir="${CODEGG_INSTALL_DIR:-}"
    if [ -z "$codegg_installer_dest_dir" ]; then
        if [ -z "${HOME:-}" ]; then
            codegg_installer_fail "HOME is unset and CODEGG_INSTALL_DIR is empty; set CODEGG_INSTALL_DIR explicitly"
        fi
        codegg_installer_dest_dir="$HOME/.local/bin"
    fi
    case "$codegg_installer_dest_dir" in
        -*) codegg_installer_fail "CODEGG_INSTALL_DIR must not start with '-'" ;;
    esac
    case "$codegg_installer_dest_dir" in
        *"
"*) codegg_installer_fail "CODEGG_INSTALL_DIR must not contain newlines" ;;
    esac
    mkdir -p -- "$codegg_installer_dest_dir" || codegg_installer_fail "cannot create install directory: $codegg_installer_dest_dir"
    if [ ! -d "$codegg_installer_dest_dir" ]; then
        codegg_installer_fail "install path is not a directory: $codegg_installer_dest_dir"
    fi
    if [ ! -w "$codegg_installer_dest_dir" ]; then
        codegg_installer_fail "install directory is not writable: $codegg_installer_dest_dir (no privilege escalation fallback; choose a user-writable CODEGG_INSTALL_DIR)"
    fi
    # Publish the destination to the cleanup trap so a backup-phase failure
    # can restore the only surviving copy instead of deleting it.
    codegg_installer_dest_dir_global="$codegg_installer_dest_dir"

    codegg_installer_asset=""
    if ! codegg_installer_asset="$(codegg_installer_asset_for_target "$codegg_installer_target")"; then
        codegg_installer_fail "internal error mapping target to asset"
    fi
    codegg_installer_archive_url="$(codegg_installer_download_url "$codegg_installer_asset" "$codegg_installer_version")"
    codegg_installer_manifest_url="$(codegg_installer_download_url "$CODEGG_INSTALLER_CHECKSUM_FILE" "$codegg_installer_version")"
    if [ -z "$codegg_installer_version" ]; then
        codegg_installer_display_tag="latest"
    else
        codegg_installer_display_tag="v$codegg_installer_version"
    fi
    printf 'codegg installer: release=%s target=%s asset=%s\ndestination: %s/(codegg codegg-sandbox-helper codegg-eggsearch)\n' \
        "$codegg_installer_display_tag" "$codegg_installer_target" "$codegg_installer_asset" "$codegg_installer_dest_dir"

    trap codegg_installer_cleanup EXIT HUP INT TERM
    codegg_installer_tmpdir="$(mktemp -d "${TMPDIR:-/tmp}/codegg-install.XXXXXX")" || codegg_installer_fail "cannot create temporary directory"
    case "$codegg_installer_tmpdir" in
        -*) codegg_installer_fail "unsafe temporary directory path" ;;
    esac
    # Note: no `--` after the mode: BSD/macOS chmod rejects it, and the mode
    # operand is fixed so no option-injection hazard exists.
    chmod 700 "$codegg_installer_tmpdir" || codegg_installer_fail "cannot secure temporary directory"
    mkdir -p -- "$codegg_installer_tmpdir/extract" || codegg_installer_fail "cannot create extraction directory"

    codegg_installer_dl_archive="$codegg_installer_tmpdir/$codegg_installer_asset"
    codegg_installer_dl_manifest="$codegg_installer_tmpdir/$CODEGG_INSTALLER_CHECKSUM_FILE"
    printf 'downloading %s\n' "$codegg_installer_manifest_url"
    codegg_installer_download "$CODEGG_INSTALLER_CHECKSUM_FILE" "$codegg_installer_manifest_url" "$codegg_installer_dl_manifest" \
        || codegg_installer_fail "download failed: $codegg_installer_manifest_url"
    printf 'downloading %s\n' "$codegg_installer_archive_url"
    codegg_installer_download "$codegg_installer_asset" "$codegg_installer_archive_url" "$codegg_installer_dl_archive" \
        || codegg_installer_fail "download failed: $codegg_installer_archive_url"

    # Integrity before extraction or execution.
    codegg_installer_verify_checksum "$codegg_installer_dl_archive" "$codegg_installer_dl_manifest" "$codegg_installer_asset" \
        || exit 1
    codegg_installer_check_archive_members "$codegg_installer_dl_archive" || exit 1

    tar -xzf "$codegg_installer_dl_archive" -C "$codegg_installer_tmpdir/extract" \
        || codegg_installer_fail "archive extraction failed"
    # Validate every extracted runfile before touching the destination.
    # shellcheck disable=SC2086
    for codegg_installer_name in $CODEGG_INSTALLER_RUNFILES; do
        codegg_installer_extracted="$codegg_installer_tmpdir/extract/$codegg_installer_name"
        if [ -L "$codegg_installer_extracted" ]; then
            codegg_installer_fail "extracted payload is a symlink: $codegg_installer_name"
        fi
        if [ ! -f "$codegg_installer_extracted" ]; then
            codegg_installer_fail "extracted payload is missing: $codegg_installer_name"
        fi
        chmod 755 "$codegg_installer_extracted" || codegg_installer_fail "cannot set executable mode on extracted payload: $codegg_installer_name"
        if [ ! -x "$codegg_installer_extracted" ]; then
            codegg_installer_fail "extracted payload is not executable: $codegg_installer_name"
        fi
    done
    if [ -e "$codegg_installer_tmpdir/extract/$CODEGG_INSTALLER_NOTICE" ]; then
        if [ -L "$codegg_installer_tmpdir/extract/$CODEGG_INSTALLER_NOTICE" ]; then
            codegg_installer_fail "extracted notice is a symlink"
        fi
        if [ ! -f "$codegg_installer_tmpdir/extract/$CODEGG_INSTALLER_NOTICE" ]; then
            codegg_installer_fail "extracted notice is not a regular file"
        fi
        chmod 644 "$codegg_installer_tmpdir/extract/$CODEGG_INSTALLER_NOTICE" \
            || codegg_installer_fail "cannot set mode on extracted notice"
    fi

    # Pre-install smoke: payloads run only after checksum and payload
    # validation have passed.
    codegg_installer_pre_out=""
    if ! codegg_installer_pre_out="$("$codegg_installer_tmpdir/extract/codegg" --version 2>&1)"; then
        codegg_installer_fail "pre-install version smoke failed; existing runfiles left intact"
    fi
    codegg_installer_check_version_output "$codegg_installer_pre_out" "$codegg_installer_version" \
        || codegg_installer_fail "pre-install version smoke rejected output; existing runfiles left intact"
    codegg_installer_pre_egg=""
    if ! codegg_installer_pre_egg="$("$codegg_installer_tmpdir/extract/codegg-eggsearch" --version 2>&1)"; then
        codegg_installer_fail "pre-install eggsearch smoke failed; existing runfiles left intact"
    fi
    codegg_installer_check_eggsearch_output "$codegg_installer_pre_egg" \
        || codegg_installer_fail "pre-install eggsearch smoke rejected output; existing runfiles left intact"

    # Bundle-level atomic commit: stage every runfile inside the destination
    # filesystem, back up existing entries, then rename staged files over
    # the directory entries. rename(2) replaces a stale symlink entry
    # instead of following it, and existing runfiles are untouched until
    # this point. A historical single-codegg layout simply has no helper
    # or eggsearch backup; fresh-install that path by creating them.
    codegg_installer_stage="$(mktemp "$codegg_installer_dest_dir/.codegg-install.XXXXXX")" \
        || codegg_installer_fail "cannot stage installer output in destination directory"
    cp -- "$codegg_installer_tmpdir/extract/codegg" "$codegg_installer_stage" \
        || codegg_installer_fail "cannot stage verified codegg executable"
    chmod 755 "$codegg_installer_stage" \
        || codegg_installer_fail "cannot set executable mode on staged codegg"
    codegg_installer_stage_helper="$(mktemp "$codegg_installer_dest_dir/.codegg-helper-install.XXXXXX")" \
        || codegg_installer_fail "cannot stage helper output in destination directory"
    cp -- "$codegg_installer_tmpdir/extract/codegg-sandbox-helper" "$codegg_installer_stage_helper" \
        || codegg_installer_fail "cannot stage verified helper executable"
    chmod 755 "$codegg_installer_stage_helper" \
        || codegg_installer_fail "cannot set executable mode on staged helper"
    codegg_installer_stage_egg="$(mktemp "$codegg_installer_dest_dir/.codegg-eggsearch-install.XXXXXX")" \
        || codegg_installer_fail "cannot stage eggsearch output in destination directory"
    cp -- "$codegg_installer_tmpdir/extract/codegg-eggsearch" "$codegg_installer_stage_egg" \
        || codegg_installer_fail "cannot stage verified eggsearch executable"
    chmod 755 "$codegg_installer_stage_egg" \
        || codegg_installer_fail "cannot set executable mode on staged eggsearch"
    codegg_installer_has_notice=0
    if [ -f "$codegg_installer_tmpdir/extract/$CODEGG_INSTALLER_NOTICE" ]; then
        codegg_installer_stage_notice="$(mktemp "$codegg_installer_dest_dir/.codegg-notice-install.XXXXXX")" \
            || codegg_installer_fail "cannot stage notice in destination directory"
        cp -- "$codegg_installer_tmpdir/extract/$CODEGG_INSTALLER_NOTICE" "$codegg_installer_stage_notice" \
            || codegg_installer_fail "cannot stage verified notice"
        chmod 644 "$codegg_installer_stage_notice" \
            || codegg_installer_fail "cannot set mode on staged notice"
        codegg_installer_has_notice=1
    fi
    # Back up existing destination entries (when present) before replacing.
    # Each backup is a rename within the destination directory, so a stale
    # symlink entry is renamed itself, never followed.
    codegg_installer_backup_codegg="$(mktemp -u "$codegg_installer_dest_dir/.codegg-backup.XXXXXX")"
    codegg_installer_backup_helper="$(mktemp -u "$codegg_installer_dest_dir/.codegg-helper-backup.XXXXXX")"
    codegg_installer_backup_egg="$(mktemp -u "$codegg_installer_dest_dir/.codegg-eggsearch-backup.XXXXXX")"
    codegg_installer_backup_notice="$(mktemp -u "$codegg_installer_dest_dir/.codegg-notice-backup.XXXXXX")"
    codegg_installer_have_bkp_codegg=0
    codegg_installer_have_bkp_helper=0
    codegg_installer_have_bkp_egg=0
    codegg_installer_have_bkp_notice=0
    if [ -e "$codegg_installer_dest_dir/codegg" ] || [ -L "$codegg_installer_dest_dir/codegg" ]; then
        mv -- "$codegg_installer_dest_dir/codegg" "$codegg_installer_backup_codegg" \
            || codegg_installer_fail "cannot back up existing codegg"
        codegg_installer_have_bkp_codegg=1
    fi
    if [ -e "$codegg_installer_dest_dir/codegg-sandbox-helper" ] || [ -L "$codegg_installer_dest_dir/codegg-sandbox-helper" ]; then
        mv -- "$codegg_installer_dest_dir/codegg-sandbox-helper" "$codegg_installer_backup_helper" \
            || codegg_installer_fail "cannot back up existing helper"
        codegg_installer_have_bkp_helper=1
    fi
    if [ -e "$codegg_installer_dest_dir/codegg-eggsearch" ] || [ -L "$codegg_installer_dest_dir/codegg-eggsearch" ]; then
        mv -- "$codegg_installer_dest_dir/codegg-eggsearch" "$codegg_installer_backup_egg" \
            || codegg_installer_fail "cannot back up existing eggsearch sidecar"
        codegg_installer_have_bkp_egg=1
    fi
    if [ "$codegg_installer_has_notice" -eq 1 ]; then
        if [ -e "$codegg_installer_dest_dir/$CODEGG_INSTALLER_NOTICE" ] || [ -L "$codegg_installer_dest_dir/$CODEGG_INSTALLER_NOTICE" ]; then
            mv -- "$codegg_installer_dest_dir/$CODEGG_INSTALLER_NOTICE" "$codegg_installer_backup_notice" \
                || codegg_installer_fail "cannot back up existing notice"
            codegg_installer_have_bkp_notice=1
        fi
    fi
    # Commit staged files one by one; on any failure roll back the entries
    # already replaced and leave no staged files behind.
    codegg_installer_rollback() {
        # Bounded best-effort recovery: one restore attempt per entry.
        if [ "$codegg_installer_have_bkp_codegg" -eq 1 ] || [ -e "$codegg_installer_dest_dir/codegg" ] || [ -L "$codegg_installer_dest_dir/codegg" ]; then
            if [ "$codegg_installer_committed_codegg" -eq 1 ]; then
                rm -f -- "$codegg_installer_dest_dir/codegg"
                if [ "$codegg_installer_have_bkp_codegg" -eq 1 ]; then
                    mv -- "$codegg_installer_backup_codegg" "$codegg_installer_dest_dir/codegg" 2>/dev/null || true
                fi
            fi
        fi
        if [ "${codegg_installer_committed_helper:-0}" -eq 1 ]; then
            rm -f -- "$codegg_installer_dest_dir/codegg-sandbox-helper"
            if [ "$codegg_installer_have_bkp_helper" -eq 1 ]; then
                mv -- "$codegg_installer_backup_helper" "$codegg_installer_dest_dir/codegg-sandbox-helper" 2>/dev/null || true
            fi
        else
            if [ "$codegg_installer_have_bkp_helper" -eq 1 ] && [ ! -e "$codegg_installer_dest_dir/codegg-sandbox-helper" ] && [ ! -L "$codegg_installer_dest_dir/codegg-sandbox-helper" ]; then
                mv -- "$codegg_installer_backup_helper" "$codegg_installer_dest_dir/codegg-sandbox-helper" 2>/dev/null || true
            fi
        fi
        if [ "${codegg_installer_committed_egg:-0}" -eq 1 ]; then
            rm -f -- "$codegg_installer_dest_dir/codegg-eggsearch"
            if [ "$codegg_installer_have_bkp_egg" -eq 1 ]; then
                mv -- "$codegg_installer_backup_egg" "$codegg_installer_dest_dir/codegg-eggsearch" 2>/dev/null || true
            fi
        else
            if [ "$codegg_installer_have_bkp_egg" -eq 1 ] && [ ! -e "$codegg_installer_dest_dir/codegg-eggsearch" ] && [ ! -L "$codegg_installer_dest_dir/codegg-eggsearch" ]; then
                mv -- "$codegg_installer_backup_egg" "$codegg_installer_dest_dir/codegg-eggsearch" 2>/dev/null || true
            fi
        fi
        if [ "${codegg_installer_committed_notice:-0}" -eq 1 ]; then
            rm -f -- "$codegg_installer_dest_dir/$CODEGG_INSTALLER_NOTICE"
            if [ "$codegg_installer_have_bkp_notice" -eq 1 ]; then
                mv -- "$codegg_installer_backup_notice" "$codegg_installer_dest_dir/$CODEGG_INSTALLER_NOTICE" 2>/dev/null || true
            fi
        else
            if [ "$codegg_installer_have_bkp_notice" -eq 1 ] && [ ! -e "$codegg_installer_dest_dir/$CODEGG_INSTALLER_NOTICE" ] && [ ! -L "$codegg_installer_dest_dir/$CODEGG_INSTALLER_NOTICE" ]; then
                mv -- "$codegg_installer_backup_notice" "$codegg_installer_dest_dir/$CODEGG_INSTALLER_NOTICE" 2>/dev/null || true
            fi
        fi
        rm -f -- "$codegg_installer_stage" "$codegg_installer_stage_helper" "$codegg_installer_stage_egg"
        if [ -n "$codegg_installer_stage_notice" ]; then
            rm -f -- "$codegg_installer_stage_notice"
        fi
    }
    codegg_installer_committed_codegg=0
    codegg_installer_committed_helper=0
    codegg_installer_committed_egg=0
    codegg_installer_committed_notice=0
    if ! mv -- "$codegg_installer_stage" "$codegg_installer_dest_dir/codegg"; then
        codegg_installer_rollback
        codegg_installer_fail "cannot install codegg to destination; previous runfiles restored where possible"
    fi
    codegg_installer_committed_codegg=1
    codegg_installer_stage=""
    if ! mv -- "$codegg_installer_stage_helper" "$codegg_installer_dest_dir/codegg-sandbox-helper"; then
        codegg_installer_rollback
        codegg_installer_fail "cannot install helper to destination; bundle rolled back where possible"
    fi
    codegg_installer_committed_helper=1
    codegg_installer_stage_helper=""
    if ! mv -- "$codegg_installer_stage_egg" "$codegg_installer_dest_dir/codegg-eggsearch"; then
        codegg_installer_rollback
        codegg_installer_fail "cannot install eggsearch sidecar to destination; bundle rolled back where possible"
    fi
    codegg_installer_committed_egg=1
    codegg_installer_stage_egg=""
    if [ "$codegg_installer_has_notice" -eq 1 ]; then
        if ! mv -- "$codegg_installer_stage_notice" "$codegg_installer_dest_dir/$CODEGG_INSTALLER_NOTICE"; then
            codegg_installer_rollback
            codegg_installer_fail "cannot install notice to destination; bundle rolled back where possible"
        fi
        codegg_installer_committed_notice=1
        codegg_installer_stage_notice=""
    fi
    # Success: drop backups and temp state.
    if [ "$codegg_installer_have_bkp_codegg" -eq 1 ]; then rm -rf -- "$codegg_installer_backup_codegg"; fi
    if [ "$codegg_installer_have_bkp_helper" -eq 1 ]; then rm -rf -- "$codegg_installer_backup_helper"; fi
    if [ "$codegg_installer_have_bkp_egg" -eq 1 ]; then rm -rf -- "$codegg_installer_backup_egg"; fi
    if [ "$codegg_installer_have_bkp_notice" -eq 1 ]; then rm -rf -- "$codegg_installer_backup_notice"; fi
    codegg_installer_backup_codegg=""
    codegg_installer_backup_helper=""
    codegg_installer_backup_egg=""
    codegg_installer_backup_notice=""

    # Post-install smoke of the committed bundle.
    codegg_installer_final_out=""
    if ! codegg_installer_final_out="$("$codegg_installer_dest_dir/codegg" --version 2>&1)"; then
        printf 'error: installed binary smoke failed AFTER replacement: %s/codegg --version exited nonzero.\n' "$codegg_installer_dest_dir" >&2
        printf 'The previous runfiles were already replaced. Re-run the installer or restore from a backup manually.\n' >&2
        exit 1
    fi
    codegg_installer_check_version_output "$codegg_installer_final_out" "$codegg_installer_version" || exit 1
    codegg_installer_final_egg=""
    if ! codegg_installer_final_egg="$("$codegg_installer_dest_dir/codegg-eggsearch" --version 2>&1)"; then
        printf 'error: installed eggsearch smoke failed AFTER replacement: %s/codegg-eggsearch --version exited nonzero.\n' "$codegg_installer_dest_dir" >&2
        exit 1
    fi
    codegg_installer_check_eggsearch_output "$codegg_installer_final_egg" || exit 1
    if [ ! -x "$codegg_installer_dest_dir/codegg-sandbox-helper" ]; then
        printf 'error: installed helper is not executable: %s/codegg-sandbox-helper\n' "$codegg_installer_dest_dir" >&2
        exit 1
    fi

    printf 'installed: %s/codegg\nversion: %s\n' \
        "$codegg_installer_dest_dir" "$codegg_installer_final_out"
    printf 'installed: %s/codegg-sandbox-helper\n' "$codegg_installer_dest_dir"
    printf 'installed: %s/codegg-eggsearch\nversion: %s\n' \
        "$codegg_installer_dest_dir" "$codegg_installer_final_egg"
    case ":$PATH:" in
        *":$codegg_installer_dest_dir:"*) ;;
        *)
            printf 'note: %s is not on PATH. Add it for this shell with:\n' "$codegg_installer_dest_dir"
            printf '  export PATH="%s:$PATH"\n' "$codegg_installer_dest_dir"
            printf 'Consult your shell documentation to persist PATH changes; this installer never edits shell profiles.\n'
            ;;
    esac
    printf 'note: replacing the executable does not restart a running daemon; the new binary takes effect on next launch.\n'

    trap - EXIT HUP INT TERM
    rm -rf -- "$codegg_installer_tmpdir"
    codegg_installer_tmpdir=""
}

if [ "${CODEGG_INSTALL_LIB_ONLY:-0}" = "1" ]; then
    # Sourced by the offline test harness: expose functions without running.
    return 0 2>/dev/null || true
else
    codegg_install_main "$@"
fi
