use codegg::upgrade::{current_version, installer_invocation, VersionInfo};

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
