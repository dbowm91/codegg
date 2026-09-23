use codegg::upgrade::{
    current_version, describe_manual_fresh_install, installer_invocation, VersionInfo,
};

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

// Pure fallback formatting. The supported Linux/macOS native update flow is
// covered separately by deterministic transport/archive fixtures.

#[test]
fn test_describe_upgrade_already_current_is_noop() {
    let info = VersionInfo {
        current: "1.0.0".to_string(),
        latest: Some("1.0.0".to_string()),
        needs_update: false,
    };
    let result = describe_manual_fresh_install(&info).expect("already-current must succeed");
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
    let result = describe_manual_fresh_install(&info).expect("no-latest without need must succeed");
    assert!(result.contains("Already on latest version"));
}

#[test]
fn test_unsupported_target_guidance_pins_manual_fresh_install() {
    let info = VersionInfo {
        current: "1.0.0".to_string(),
        latest: Some("2.0.0".to_string()),
        needs_update: true,
    };
    let err =
        describe_manual_fresh_install(&info).expect_err("newer release needs install guidance");
    let message = err.to_string();
    assert!(
        message.contains("native in-place update is unavailable"),
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
}

#[test]
fn test_describe_upgrade_missing_latest_fails_closed() {
    let info = VersionInfo {
        current: "1.0.0".to_string(),
        latest: None,
        needs_update: true,
    };
    let err = describe_manual_fresh_install(&info).expect_err("missing tag must fail closed");
    assert!(err.to_string().contains("no latest version found"));
}

#[test]
fn test_describe_upgrade_invalid_semver_fails_closed() {
    let info = VersionInfo {
        current: "1.0.0".to_string(),
        latest: Some("not-a-version".to_string()),
        needs_update: true,
    };
    let err = describe_manual_fresh_install(&info).expect_err("invalid semver must fail closed");
    assert!(err.to_string().contains("invalid semver version"));
}
