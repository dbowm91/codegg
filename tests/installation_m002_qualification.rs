//! Self-contained-installation M002 qualification.
//!
//! Covers the plan regression guards in one narrow integration target:
//! - absent eggsearch command resolves the managed sibling while an
//!   explicit override stays explicit;
//! - packaged-path resolution works with `PATH` stripped of eggsearch;
//! - trusted sandbox helper resolves from an installed layout (no PATH
//!   fallback) and ignores inherited helper env;
//! - deterministic tools are in-process eggsact (no executable, curated
//!   palette, pinned 1.2.5 path);
//! - prebuilt docs never reintroduce "install eggsearch separately";
//! - ten-tool bundled surface expectation;
//! - sandbox helper qualification through the real trusted resolver plus
//!   the status-channel enforcement signal;
//! - installation report is secret-free and well-formed.

use std::path::PathBuf;

#[cfg(unix)]
fn make_layout(helper: bool, egg: bool) -> tempfile::TempDir {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().expect("installation fixture");
    let exe = dir.path().join("codegg");
    std::fs::write(&exe, b"codegg").expect("exe fixture");
    if helper {
        let h = dir.path().join(codegg::install::sandbox_helper_name());
        std::fs::write(&h, b"helper").expect("helper fixture");
        std::fs::set_permissions(&h, std::fs::Permissions::from_mode(0o755)).expect("helper perms");
    }
    if egg {
        let e = dir.path().join(codegg::install::eggsearch_sidecar_name());
        std::fs::write(&e, b"egg").expect("egg fixture");
        std::fs::set_permissions(&e, std::fs::Permissions::from_mode(0o755)).expect("egg perms");
    }
    dir
}

#[test]
fn absent_command_resolves_managed_while_explicit_stays_explicit() {
    #[cfg(not(unix))]
    return;
    #[cfg(unix)]
    {
        let dir = make_layout(true, true);
        let exe = dir.path().join("codegg");
        let absent = codegg::config::schema::EggsearchConfig::default();
        assert!(!absent.has_explicit_command());
        let resolved = codegg::install::resolve_eggsearch_command_for(&absent, &exe);
        match resolved {
            codegg::install::EggsearchResolution::ManagedSibling { path } => {
                assert!(path.ends_with(codegg::install::eggsearch_sidecar_name()));
            }
            other => panic!("absent must resolve managed, got {other:?}"),
        }
        let explicit = codegg::config::schema::EggsearchConfig {
            command: Some("custom-eggsearch".to_string()),
            ..Default::default()
        };
        assert!(explicit.has_explicit_command());
        assert_eq!(explicit.explicit_command(), Some("custom-eggsearch"));
        let resolved = codegg::install::resolve_eggsearch_command_for(&explicit, &exe);
        assert_eq!(
            resolved,
            codegg::install::EggsearchResolution::ExplicitCommand {
                command: "custom-eggsearch".to_string(),
            }
        );
    }
}

#[test]
fn packaged_path_works_with_path_stripped_of_eggsearch() {
    #[cfg(not(unix))]
    return;
    #[cfg(unix)]
    {
        let dir = make_layout(true, true);
        let exe = dir.path().join("codegg");
        // Strip PATH to an empty temp dir: no eggsearch, no cargo.
        let empty = tempfile::tempdir().expect("empty path");
        let old = std::env::var_os("PATH");
        // SAFETY: single-threaded test manipulation of process PATH for the
        // resolution call below, which deliberately ignores PATH.
        unsafe {
            std::env::set_var("PATH", empty.path());
        }
        let absent = codegg::config::schema::EggsearchConfig::default();
        let resolved = codegg::install::resolve_eggsearch_command_for(&absent, &exe);
        if let Some(old) = old {
            // SAFETY: restoring the caller PATH.
            unsafe {
                std::env::set_var("PATH", old);
            }
        } else {
            // SAFETY: restoring absence.
            unsafe {
                std::env::remove_var("PATH");
            }
        }
        match resolved {
            codegg::install::EggsearchResolution::ManagedSibling { path } => {
                assert!(path.starts_with(dir.path().canonicalize().unwrap()));
            }
            other => panic!("stripped PATH must still resolve managed, got {other:?}"),
        }
    }
}

#[test]
fn trusted_helper_resolves_from_installed_layout_without_path_fallback() {
    #[cfg(not(unix))]
    return;
    #[cfg(unix)]
    {
        let dir = make_layout(true, true);
        let exe = dir.path().join("codegg");
        let helper = codegg::install::trusted_sandbox_helper_path_for(&exe)
            .expect("installed helper must resolve");
        assert!(helper.ends_with(codegg::install::sandbox_helper_name()));
        // Inherited helper-specific env must not steer resolution.
        let decoy_dir = tempfile::tempdir().expect("decoy");
        let decoy = decoy_dir
            .path()
            .join(codegg::install::sandbox_helper_name());
        std::fs::write(&decoy, b"decoy").expect("decoy");
        // SAFETY: scoped env mutation for the resolution call below.
        unsafe {
            std::env::set_var("CODEGG_SANDBOX_HELPER", &decoy);
        }
        let again = codegg::install::trusted_sandbox_helper_path_for(&exe)
            .expect("env must not steer helper");
        // SAFETY: cleanup.
        unsafe {
            std::env::remove_var("CODEGG_SANDBOX_HELPER");
        }
        assert_eq!(helper, again);
        // Missing runfile is a distinct installation error, not an
        // "unsupported kernel" signal.
        let bare = make_layout(false, false);
        let bare_exe = bare.path().join("codegg");
        let err = codegg::install::trusted_sandbox_helper_path_for(&bare_exe)
            .expect_err("missing helper must fail");
        assert!(
            err.contains("could not be resolved") || err.contains("Missing"),
            "missing helper error must name resolution, got: {err}"
        );
    }
}

#[test]
fn eggsact_is_inprocess_curated_and_pinned() {
    // Pinned library baseline, not an executable contract.
    assert_eq!(codegg::install::PINNED_EGGSACT_VERSION, "1.2.5");
    let lock = std::fs::read_to_string("Cargo.lock").expect("Cargo.lock readable");
    assert!(
        lock.contains("name = \"eggsact\"\nversion = \"1.2.5\""),
        "lockfile must resolve eggsact 1.2.5"
    );
    // No production code shells out to an eggsact executable.
    let mut hits = Vec::new();
    for entry in walkdir::WalkDir::new("src")
        .into_iter()
        .filter_map(Result::ok)
    {
        if entry.path().extension().and_then(|e| e.to_str()) != Some("rs") {
            continue;
        }
        let content = std::fs::read_to_string(entry.path()).unwrap_or_default();
        for (idx, line) in content.lines().enumerate() {
            let lower = line.to_lowercase();
            if lower.contains("eggsact")
                && (lower.contains("command::new")
                    || lower.contains(" экз")
                    || lower.contains("process::command")
                    || lower.contains("tokio::process"))
            {
                hits.push(format!("{}:{}: {line}", entry.path().display(), idx + 1));
            }
            // Direct spawn of a bare `eggsact` binary would be a sidecar
            // regression; the managed contract has no eggsact executable.
            if line.contains("\"eggsact\"") && line.contains("Command") {
                hits.push(format!("{}:{}: {line}", entry.path().display(), idx + 1));
            }
        }
    }
    assert!(
        hits.is_empty(),
        "no process spawn of an eggsact executable is permitted, got: {hits:?}"
    );
    // Curated palette stays small (8 always-visible + 5 deferred = 13),
    // never the full upstream ~86-tool surface.
    let runtime = std::sync::Arc::new(
        codegg::eggsact::adapter::EggsactRuntime::new(
            codegg::eggsact::adapter::EggsactConfig::default(),
        )
        .expect("in-process runtime builds"),
    );
    assert!(runtime.has_tool("text_equal"));
    let (visible, deferred) = codegg::tool::deterministic::build_eggsact_tools(runtime);
    let total = visible.len() + deferred.len();
    assert!(
        (13..86).contains(&total),
        "curated palette must stay bounded (got {total})"
    );
    // One deterministic tool executes in-process with no `eggsact` binary.
    let rt = codegg::eggsact::adapter::EggsactRuntime::new(
        codegg::eggsact::adapter::EggsactConfig::default(),
    )
    .expect("runtime");
    let out = rt
        .call_json("text_equal", serde_json::json!({"a": "x", "b": "x"}))
        .expect("in-process call works");
    assert!(out.success);
    // Clean-host invariant: no `eggsact` executable is required on PATH.
    // (The test itself does not depend on one; absence is the expected
    // clean state, presence elsewhere must not matter.)
    let contract = codegg::install::eggsact_contract_line();
    assert!(contract.contains("in-process"));
    assert!(contract.contains("1.2.5"));
    assert!(contract.contains("no executable"));
}

#[test]
fn prebuilt_docs_do_not_require_separate_eggsearch_install() {
    let readme = std::fs::read_to_string("README.md").expect("README readable");
    // The supported prebuilt quick start must not tell users to install
    // eggsearch separately. Source-build notes may describe the sidecar
    // override, but the prebuilt path owns the bundle.
    let prebuilt_start = readme
        .find("### Prebuilt installer")
        .expect("prebuilt section exists");
    let source_start = readme
        .find("### From source")
        .expect("source section exists");
    let prebuilt = &readme[prebuilt_start..source_start];
    let lower = prebuilt.to_lowercase();
    assert!(
        !lower.contains("install eggsearch separately") && !lower.contains("install eggsearch"),
        "prebuilt quick start must not require a separate eggsearch install"
    );
    assert!(
        prebuilt.contains("codegg-eggsearch"),
        "prebuilt section must name the managed sidecar"
    );
}

#[test]
fn ten_tool_bundled_surface_is_complete_contract() {
    assert_eq!(
        codegg::search_backend::bootstrap::EGGSEARCH_BUNDLED_COMPLETE_SURFACE.len(),
        10
    );
    for required in ["web_search", "web_fetch"] {
        assert!(
            codegg::search_backend::bootstrap::EGGSEARCH_BUNDLED_COMPLETE_SURFACE
                .contains(&required)
        );
    }
    for recommended in [
        "batch_fetch",
        "repo_search",
        "repo_fetch",
        "repo_map",
        "security_search",
        "research_search",
        "build_evidence_bundle",
    ] {
        assert!(
            codegg::search_backend::bootstrap::EGGSEARCH_BUNDLED_COMPLETE_SURFACE
                .contains(&recommended)
        );
    }
    assert!(
        codegg::search_backend::bootstrap::EGGSEARCH_BUNDLED_COMPLETE_SURFACE
            .contains(&"provider_status")
    );
    // Coverage helper marks a fully populated report complete.
    let report = codegg::search_backend::bootstrap::BootstrapReport {
        tools: codegg::search_backend::bootstrap::EGGSEARCH_BUNDLED_COMPLETE_SURFACE
            .iter()
            .map(|s| s.to_string())
            .collect(),
        ..Default::default()
    };
    assert!(report.missing_bundled_tools().is_empty());
    assert_eq!(report.tool_coverage_status(), "complete");
}

#[test]
fn sandbox_helper_qualification_through_trusted_resolver() {
    // Real installed-layout helper (target/<profile>/codegg-sandbox-helper
    // in test builds) resolves through the strict sibling rule.
    let resolved = match codegg::security::sandbox::sandbox_helper_path() {
        Ok(p) => p,
        Err(e) => {
            // No built helper in this checkout: the missing-runfile signal
            // itself must be distinct from an unsupported-kernel signal.
            let probe = codegg::security::sandbox::probe_landlock()
                .err()
                .unwrap_or_else(|| "available".to_string());
            assert!(
                e.contains("could not be resolved")
                    || e.contains("metadata")
                    || e.contains("regular file")
                    || e.contains("executable")
                    || e.contains("installation directory"),
                "missing helper must report installation cause, got: {e} (probe: {probe})"
            );
            return;
        }
    };
    assert!(resolved.is_file());
    // Status channel: Enforced setup decodes as enforcement (the exact
    // signal the helper emits after Landlock restriction on Linux).
    let frame = codegg::security::sandbox::encode_sandbox_status(
        codegg::security::sandbox::SandboxLaunchOutcome::Enforced { abi: 1 },
    )
    .expect("enforced frame encodes");
    let decoded =
        codegg::security::sandbox::decode_sandbox_status(&frame).expect("enforced frame decodes");
    assert_eq!(
        decoded,
        codegg::security::sandbox::SandboxLaunchOutcome::Enforced { abi: 1 }
    );
    // Platform distinction: unsupported kernel/platform is Unavailable
    // (fail-closed), never confused with a missing runfile above and never
    // silently FullHost.
    let enforcement = codegg::security::sandbox::resolve_sandbox_enforcement(
        codegg_core::approval::SandboxProfile::WorkspaceWrite,
    );
    if codegg::security::sandbox::SandboxConfig::is_available() {
        assert!(enforcement.is_enforced());
    } else {
        assert!(!enforcement.is_enforced());
        assert!(!enforcement.is_full_host());
        let reason = codegg::security::sandbox::probe_landlock()
            .expect_err("unavailable host must name a reason");
        assert!(!reason.is_empty());
    }
}

#[test]
fn installation_report_is_secret_free_and_well_formed() {
    let report = codegg::install::InstallationReport::describe();
    assert!(!report.codegg_version.is_empty());
    assert!(!report.installation_dir.is_empty());
    assert_eq!(
        report.eggsearch_sidecar_name,
        codegg::install::eggsearch_sidecar_name()
    );
    assert_eq!(
        report.sandbox_helper_name,
        codegg::install::sandbox_helper_name()
    );
    let lines = report.summary_lines();
    let joined = lines.join("\n").to_lowercase();
    assert!(joined.contains("codegg executable"));
    assert!(joined.contains("installation directory"));
    assert!(joined.contains("sandbox helper"));
    assert!(joined.contains("eggsact"));
    for secret in [
        "api_key",
        "master_key",
        "encryption_key",
        "authorization",
        "bearer ",
    ] {
        assert!(
            !joined.contains(secret),
            "installation report must never leak secrets (hit {secret:?})"
        );
    }
    // Missing sidecar hint (when applicable in this checkout) must name the
    // bundle reinstall, never a bare eggsearch install.
    if !matches!(
        report.eggsearch_state,
        codegg::install::SiblingState::Present { .. }
    ) {
        assert!(joined.contains("reinstall the prebuilt"));
    }
}

#[test]
fn packaged_installation_layout_smoke_without_external_helpers() {
    // Simulates the packaged layout: temp dir acts as the install root with
    // all three runfiles as executable files. Resolution must succeed
    // without consulting PATH or helper env.
    #[cfg(not(unix))]
    return;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().expect("package fixture");
        for name in [
            "codegg",
            &codegg::install::eggsearch_sidecar_name(),
            &codegg::install::sandbox_helper_name(),
        ] {
            let p = dir.path().join(name);
            std::fs::write(&p, b"runfile").expect("runfile");
            std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).expect("perms");
        }
        // No eggsact executable may exist for the smoke to be valid.
        let path_dirs: Vec<PathBuf> = std::env::var_os("PATH")
            .map(|v| std::env::split_paths(&v).collect())
            .unwrap_or_default();
        let mut eggsearch_on_path = Vec::new();
        let mut eggsact_on_path = Vec::new();
        for d in path_dirs {
            let a = d.join("eggsearch");
            let b = d.join("eggsact");
            if a.is_file() {
                eggsearch_on_path.push(a);
            }
            if b.is_file() {
                eggsact_on_path.push(b);
            }
        }
        // The smoke does not fail when such binaries happen to exist on a
        // dev host; it proves the packaged layout itself resolves without
        // them by using the explicit-layout seam.
        let exe = dir.path().join("codegg");
        let egg =
            codegg::install::managed_eggsearch_path_for(&exe).expect("packaged eggsearch resolves");
        let helper = codegg::install::trusted_sandbox_helper_path_for(&exe)
            .expect("packaged helper resolves");
        assert!(egg.is_file());
        assert!(helper.is_file());
        let _ = (eggsearch_on_path, eggsact_on_path);
    }
}
