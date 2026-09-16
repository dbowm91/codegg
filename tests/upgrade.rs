use codegg::upgrade::{current_version, describe_upgrade, installer_invocation, VersionInfo};

#[test]
fn test_current_version() {
    let version = current_version();
    assert!(!version.is_empty());
    assert!(version.contains('.'));
}

#[test]
fn test_version_info_current_only() {
    let info = VersionInfo {
        current: "1.0.0".to_string(),
        latest: None,
        needs_update: false,
    };

    assert_eq!(info.current, "1.0.0");
    assert!(info.latest.is_none());
    assert!(!info.needs_update);
}

#[test]
fn test_version_info_needs_update() {
    let info = VersionInfo {
        current: "1.0.0".to_string(),
        latest: Some("2.0.0".to_string()),
        needs_update: true,
    };

    assert!(info.needs_update);
    assert_eq!(info.latest, Some("2.0.0".to_string()));
}

#[test]
fn test_version_info_up_to_date() {
    let info = VersionInfo {
        current: "2.0.0".to_string(),
        latest: Some("2.0.0".to_string()),
        needs_update: false,
    };

    assert!(!info.needs_update);
}

#[test]
fn test_installer_invocation_pins_supported_env() {
    // Regression: the installer honors CODEGG_VERSION. Exporting any other
    // name (e.g. INSTALL_VERSION) silently installs latest instead.
    let (script_url, env) = installer_invocation("v2.0.0");

    assert_eq!(
        script_url,
        "https://raw.githubusercontent.com/dbowm91/codegg/main/install.sh"
    );
    assert_eq!(env, vec![("CODEGG_VERSION", "v2.0.0".to_string())]);
}

// M005 WP5: deterministic fail-closed disposition for the retired
// in-place execution path. No test hits the network; `describe_upgrade`
// is the pure decision surface behind `upgrade()`.

#[test]
fn test_describe_upgrade_already_current_is_noop() {
    let info = VersionInfo {
        current: "1.0.0".to_string(),
        latest: Some("1.0.0".to_string()),
        needs_update: false,
    };
    let result = describe_upgrade(&info).expect("already-current must succeed");
    assert!(result.contains("Already on latest version"));
    assert!(result.contains("1.0.0"));
}

#[test]
fn test_describe_upgrade_current_only_is_noop() {
    let info = VersionInfo {
        current: "1.0.0".to_string(),
        latest: None,
        needs_update: false,
    };
    let result = describe_upgrade(&info).expect("no-latest without need must succeed");
    assert!(result.contains("Already on latest version"));
}

#[test]
fn test_describe_upgrade_valid_newer_fails_closed_with_manual_guidance() {
    let info = VersionInfo {
        current: "1.0.0".to_string(),
        latest: Some("2.0.0".to_string()),
        needs_update: true,
    };
    let err = describe_upgrade(&info).expect_err("newer candidate must not auto-replace");
    let message = err.to_string();
    assert!(
        message.contains("automatic in-place update is disabled"),
        "unexpected message: {message}"
    );
    assert!(
        message.contains("existing executable left intact"),
        "unexpected message: {message}"
    );
    assert!(message.contains("2.0.0"), "unexpected message: {message}");
    assert!(
        message.contains("CODEGG_VERSION=v2.0.0"),
        "unexpected message: {message}"
    );
    assert!(
        message.contains("https://raw.githubusercontent.com/dbowm91/codegg/main/install.sh"),
        "unexpected message: {message}"
    );
    // Fail-closed also covers the WP5 replacement-adjacent cases by
    // construction: no candidate bytes are acquired and no replacement is
    // attempted, so checksum mismatch, wrong program/version identity,
    // unwritable destination, interrupted download, replacement failure,
    // and unsupported target all leave the existing executable intact.
}

#[test]
fn test_describe_upgrade_missing_latest_fails_closed() {
    let info = VersionInfo {
        current: "1.0.0".to_string(),
        latest: None,
        needs_update: true,
    };
    let err = describe_upgrade(&info).expect_err("missing tag must fail closed");
    assert!(err.to_string().contains("no latest version found"));
}

#[test]
fn test_describe_upgrade_invalid_semver_fails_closed() {
    let info = VersionInfo {
        current: "1.0.0".to_string(),
        latest: Some("not-a-version".to_string()),
        needs_update: true,
    };
    let err = describe_upgrade(&info).expect_err("invalid semver must fail closed");
    assert!(err.to_string().contains("invalid semver version"));
}

#[test]
fn test_describe_upgrade_never_reports_automatic_success_for_newer() {
    // Guards against reintroducing an automatic "Upgraded to X" path that
    // would imply executable replacement happened.
    for latest in ["2.0.0", "1.0.1", "10.0.0"] {
        let info = VersionInfo {
            current: "1.0.0".to_string(),
            latest: Some(latest.to_string()),
            needs_update: true,
        };
        let err = describe_upgrade(&info).expect_err("must remain fail-closed");
        assert!(
            !err.to_string().contains("Upgraded to"),
            "must not claim automatic replacement for {latest}"
        );
    }
}
