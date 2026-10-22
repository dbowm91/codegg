//! M005 C006 — native macOS containment qualification for the shared checkout.
//!
//! Scope: `plans/implementation/agent-workstream-m005-writer-containment-corrective/006-restricted-shared-checkout-qualification.md` (Q4 native matrix).
//!
//! Why this file exists. The shared-checkout profile is **platform-gated**: it
//! may only be enabled on a platform where the native *write-confinement*
//! dimension is qualified. That dimension is what makes the profile's central
//! claim meaningful. The claim is that exactly one CodeGG-mediated writer
//! holds the checkout at a time; it would be worthless if a CodeGG-managed
//! process outside that mediation could scribble into the same checkout from
//! the side. Native containment is the mechanism that prevents it, because a
//! confined process can write only inside its own roots.
//!
//! On Linux that mechanism is Landlock's, and it is qualified by
//! `tests/linux_native_scratch_containment.rs`. On macOS it is Seatbelt's,
//! and this file is that qualification.
//!
//! This file is a whole-file `cfg(target_os = "macos")` fixture on purpose: a
//! qualification that cannot run on the target platform must not exist as a
//! *passing* test on another platform, so on Linux and Windows this binary
//! contains no tests at all.
//!
//! A macOS host that cannot obtain Seatbelt **fails** this fixture instead of
//! skipping it. A skipped qualification is indistinguishable from a passed one
//! in a test report, and the plan forbids counting "unavailable" as
//! "qualified".
//!
//! What this fixture claims, and what it does not:
//!
//! * It claims the **write-confinement** dimension on a native macOS host:
//!   a process launched through the real `ManagedProcessService` under a real
//!   `SandboxRequest::Required` can write inside its own roots and cannot
//!   write into a checkout outside them.
//! * It does **not** claim read isolation. Seatbelt's profile is
//!   write-confined, not read-allowlisted, and the fixture asserts that the
//!   obtained report *discloses* that limit rather than letting the
//!   qualification be read as stronger than it is.
//! * It does not claim process, IPC or network isolation, and it says nothing
//!   about Windows, where no filesystem-enforcement backend is registered.

#![cfg(target_os = "macos")]

use std::ffi::OsString;
use std::path::Path;

use codegg::managed_process::{
    EnvironmentPolicy, ManagedProcessRequest, ManagedProcessService, ProcessProvenance,
    SandboxExecutionOutcome, SandboxRequest,
};
use codegg::security::sandbox::{
    platform_sandbox_capability, BackendId, SandboxCapability, SandboxConfig, SandboxMode,
};

/// The native backend this platform qualifies.
const EXPECTED_BACKEND: BackendId = BackendId::SEATBELT;

/// The dimension this platform contributes to the shared-checkout profile.
///
/// The string is matched against the backend's own disclosed limits, so the
/// qualification stays tied to what the backend actually reports rather than
/// to a phrase this file invented.
const WRITE_CONFINEMENT_LIMIT: &str = "write-confined";

/// Obtain the native backend, or fail.
///
/// The failure is deliberate. A macOS host that cannot obtain Seatbelt cannot
/// qualify the shared-checkout profile, and a fixture that quietly returned
/// would report that qualification as satisfied.
fn require_seatbelt() -> SandboxCapability {
    match platform_sandbox_capability() {
        SandboxCapability::Available { backend } => {
            assert_eq!(
                backend, EXPECTED_BACKEND,
                "the native backend on macOS must be {EXPECTED_BACKEND:?}, got {backend:?}"
            );
            platform_sandbox_capability()
        }
        SandboxCapability::Unavailable { reason } => panic!(
            "a macOS host that cannot obtain Seatbelt cannot qualify the shared-checkout \
             profile: {reason}"
        ),
    }
}

/// Quote a path for the shell.
fn sh_quote(path: &Path) -> String {
    format!("'{}'", path.to_string_lossy().replace('\'', r"'\''"))
}

/// The native backend is present and is Seatbelt.
///
/// This is the platform half of the Q4 matrix: the profile is enabled on a
/// platform only where an actual supported backend exists, and macOS's is
/// Seatbelt.
#[test]
fn macos_obtains_the_seatbelt_backend() {
    let capability = require_seatbelt();
    assert!(capability.is_available());
    assert_eq!(capability.backend_id(), EXPECTED_BACKEND);
    assert!(
        capability.reason().is_none(),
        "an available platform must not carry an unavailability reason"
    );
}

/// A confined process can write inside its own roots and cannot write into a
/// checkout outside them.
///
/// The launch goes through the real `ManagedProcessService` with a real
/// `SandboxRequest::Required`, so the assertion is about the *observed*
/// behaviour of a child the product actually launched, and about the
/// *obtained* enforcement report — never about a launch spec this file wrote
/// for itself.
#[tokio::test(flavor = "current_thread")]
async fn a_confined_process_cannot_write_into_a_checkout_outside_its_roots() {
    let capability = require_seatbelt();

    // The process's own roots.
    let own_root = tempfile::tempdir().expect("own root");
    // Stands in for a group's shared checkout: a real directory the confined
    // process must not be able to write into.
    let shared_checkout = tempfile::tempdir().expect("shared checkout");

    let inside = own_root.path().join("inside.txt");
    let outside = shared_checkout.path().join("outside.txt");

    // The redirect is the observation: when the write is denied the shell
    // cannot even create the file, so the success marker never prints.
    let script = format!(
        "echo inside > {inside} 2>/dev/null && echo INSIDE_OK; \
         echo outside > {outside} 2>/dev/null && echo OUTSIDE_OK || echo OUTSIDE_DENIED",
        inside = sh_quote(&inside),
        outside = sh_quote(&outside),
    );

    let config = SandboxConfig::new()
        .with_enabled(true)
        .with_mode(SandboxMode::WorkspaceWrite)
        .with_allowed_paths(vec![own_root.path().to_string_lossy().into_owned()]);
    let spec = config
        .launch_spec(
            "sh",
            &["-c".to_string(), script.clone()],
            Some(own_root.path()),
        )
        .expect("production launch spec");

    let mut request = ManagedProcessRequest::new(
        vec![
            OsString::from("sh"),
            OsString::from("-c"),
            OsString::from(script),
        ],
        own_root.path().to_path_buf(),
        ProcessProvenance::new("job-macos-shared-checkout", "attempt-macos-shared-checkout"),
    );
    request.sandbox = SandboxRequest::Required(spec);
    request.environment_policy = EnvironmentPolicy::sanitized();

    let result = ManagedProcessService::run(request)
        .await
        .expect("the contained launch must succeed");

    // The obtained status, not the requested one.
    let SandboxExecutionOutcome::Enforced {
        backend,
        guarantees,
        limits,
        ..
    } = &result.sandbox
    else {
        panic!(
            "the launch must report obtained containment, got {:?}",
            result.sandbox
        );
    };
    assert_eq!(*backend, EXPECTED_BACKEND);
    assert!(capability.is_available());
    assert!(
        guarantees
            .iter()
            .any(|guarantee| guarantee.contains("writes confined")),
        "the obtained guarantees must record the write confinement, got {guarantees:?}"
    );
    assert!(
        limits
            .iter()
            .any(|limit| limit.contains(WRITE_CONFINEMENT_LIMIT)),
        "the obtained report must disclose that containment is write-confined rather than \
         read-allowlisted, got {limits:?}"
    );

    let stdout = result.stdout.to_string_lossy();
    assert!(
        stdout.contains("INSIDE_OK"),
        "the confined child must be able to write inside its own root: {stdout:?}"
    );
    assert!(
        stdout.contains("OUTSIDE_DENIED"),
        "the confined child must not be able to write into a checkout outside its roots: \
         {stdout:?}"
    );
    assert!(
        inside.exists(),
        "the own root must hold the write the child was allowed to make"
    );
    assert!(
        !outside.exists(),
        "the checkout outside the child's roots must be untouched"
    );
}
