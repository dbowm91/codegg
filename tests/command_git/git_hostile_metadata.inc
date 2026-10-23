//! C003: hostile Git configuration metadata.
//!
//! A Git *read* can execute code. `git status` consults the clean/smudge
//! filter machinery, and a repository that declares `filter.<name>.process`
//! (or `filter.<name>.clean`) selects a filter whenever an attribute in the
//! worktree names it. The same is true for a user-level
//! `core.attributesFile`, which can select a filter without the repository
//! having to name it.
//!
//! The trusted read route neutralises a *list* of repository-controlled config
//! surfaces by injecting `-c key=value` overrides that beat every config file.
//! That list is not general: an override beats the keys it names and nothing
//! else, so a dangerous key hidden inside a file reached through `include.path`
//! still applies. `egggit::config_surface` closes that gap with a bounded,
//! read-only detector that follows the include graph and reports which
//! dangerous surfaces the repository declares.
//!
//! The suite covers:
//!
//! * the include-path gap — a filter declared **only** in an included file;
//! * the host-positive effect control — the sentinel command really can
//!   produce the sentinel, so the sentinel-absence assertions are not vacuous;
//! * the allow case — an ordinary repository reports no surface;
//! * the credential-helper asymmetry — the URL-scoped form is a finding, the
//!   plain form is not;
//! * the secret guarantee — a report never carries a configuration value.

use super::common;

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use egggit::config_surface::{
    inspect_repository_config_surface, HostileConfigReport, HostileConfigSurface,
};

// ── Fixtures ────────────────────────────────────────────────────────────────

fn git_binary() -> PathBuf {
    codegg::git_publication::resolve_git_binary().expect("git is available on PATH")
}

/// Run a plain `git` command, outside the trusted read route.
fn run_git(dir: &Path, args: &[&str]) -> String {
    let output = Command::new(git_binary())
        .args(args)
        .current_dir(dir)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .output()
        .expect("git runs");
    assert!(
        output.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

/// A repository whose git directory holds the given configuration files.
///
/// The files are written verbatim, so a fixture can declare exactly the
/// configuration surface under test and nothing else.
fn repository_with_config(files: &[(&str, &str)]) -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("tempdir");
    let git_dir = dir.path().join(".git");
    fs::create_dir_all(&git_dir).expect("git dir");
    for (name, contents) in files {
        fs::write(git_dir.join(name), contents).expect("write config");
    }
    dir
}

/// The shell command a filter would run, and which writes the sentinel file.
///
/// Git runs a filter command through the shell, so the value is a shell
/// command. The redirection form is used rather than `touch` so the command
/// needs nothing but a POSIX shell.
fn sentinel_command(sentinel: &Path) -> String {
    format!(": > '{}'", sentinel.display())
}

/// A repository whose `.git/config` includes `hostile.cfg`, and whose
/// included file declares the given `[filter "x"]` key.
fn repository_with_included_filter(key: &str, value: &str) -> tempfile::TempDir {
    repository_with_config(&[
        ("config", "[include]\n\tpath = hostile.cfg\n"),
        (
            "hostile.cfg",
            &format!("[filter \"x\"]\n\t{key} = {value}\n"),
        ),
    ])
}

// ── Suite C: hostile metadata ───────────────────────────────────────────────

#[test]
fn a_filter_process_declared_only_through_include_path_is_detected() {
    // The include-path gap: the dangerous key lives in a file the repository
    // config only *reaches*. An override list that names `filter.x.process`
    // does not beat a key the repository never put in the file the override
    // applies to — the detector has to follow the include graph.
    // The sentinel lives in its own scratch root, outside the fixture's
    // repository, so its presence cannot be confused with repository content.
    let scratch = tempfile::tempdir().expect("tempdir");
    let sentinel = scratch.path().join("filter-sentinel");
    let command = sentinel_command(&sentinel);

    let repo = repository_with_included_filter("process", &command);

    let report = inspect_repository_config_surface(repo.path());

    assert!(
        report.has_surface(HostileConfigSurface::FilterCommand),
        "the include-path gap is not closed: {report:?}"
    );
    assert!(
        !report.truncated,
        "the inspection must be complete: {report:?}"
    );

    // The detector never executes anything.
    assert!(
        !sentinel.exists(),
        "the detector executed the filter command it only reported"
    );
}

#[test]
fn a_filter_clean_command_declared_only_through_include_path_is_detected() {
    // The regression the prior review proved: `git diff` executed a `filter`
    // declared only in a file reached through `include.path`. The `clean`
    // form is the same code-execution surface as the `process` form — Git
    // runs it while refreshing the index during a read — and it is declared
    // the same way, so the detector must report it the same way.
    let scratch = tempfile::tempdir().expect("tempdir");
    let sentinel = scratch.path().join("filter-sentinel");
    let command = sentinel_command(&sentinel);

    let repo = repository_with_included_filter("clean", &command);

    let report = inspect_repository_config_surface(repo.path());

    assert!(
        report.has_surface(HostileConfigSurface::FilterCommand),
        "a filter clean command reached only through include.path was not reported: {report:?}"
    );
    assert!(
        !report.truncated,
        "the inspection must be complete: {report:?}"
    );
    assert!(
        !sentinel.exists(),
        "the detector executed the filter command it only reported"
    );
}

#[test]
fn the_filter_sentinel_command_produces_the_sentinel_when_run_directly() {
    // The host-positive effect control. A sentinel-absence assertion is only
    // meaningful if the command could produce the sentinel; if it cannot in
    // this environment, every sentinel-absence assertion in this suite is
    // vacuous. This test therefore fails loudly rather than skipping.
    let dir = tempfile::tempdir().expect("tempdir");
    let sentinel = dir.path().join("filter-sentinel");
    let command = sentinel_command(&sentinel);

    let output = Command::new("/bin/sh")
        .args(["-c", &command])
        .output()
        .expect("the filter command runs");
    assert!(
        output.status.success(),
        "the filter command failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        sentinel.exists(),
        "the filter command did not produce the sentinel; the sentinel-absence \
         assertions in this suite would be vacuous"
    );
}

#[test]
fn an_ordinary_repository_reports_no_hostile_surface() {
    // The allow case. A repository Git itself created, with a benign
    // configuration, must report nothing.
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    run_git(root, &["init", "--quiet", "--initial-branch=main"]);
    run_git(root, &["config", "user.name", "Someone"]);
    run_git(root, &["config", "user.email", "someone@example.com"]);
    run_git(root, &["config", "core.autocrlf", "input"]);

    let report = inspect_repository_config_surface(root);

    assert!(
        report.surfaces.is_empty(),
        "an ordinary repository must report no surface: {report:?}"
    );
    assert!(
        !report.truncated,
        "the inspection must be complete: {report:?}"
    );
    assert!(report.is_clean());
}

#[test]
fn a_url_scoped_credential_helper_is_detected_and_the_plain_form_is_not() {
    // The URL-scoped form is a different key from `credential.helper`, so it
    // survives the trusted route's `-c credential.helper=` override and is a
    // finding.
    let scoped = repository_with_config(&[(
        "config",
        "[credential \"https://example.com\"]\n\thelper = store --file=/tmp/creds\n",
    )]);
    let report = inspect_repository_config_surface(scoped.path());
    assert!(
        report.has_surface(HostileConfigSurface::UrlScopedCredentialHelper),
        "the URL-scoped credential helper was not reported: {report:?}"
    );
    assert!(!report.is_clean());

    // The plain form *is* cleared by that override, so it is not a finding.
    let plain = repository_with_config(&[("config", "[credential]\n\thelper = store\n")]);
    let report = inspect_repository_config_surface(plain.path());
    assert!(
        !report.has_surface(HostileConfigSurface::UrlScopedCredentialHelper),
        "the plain credential helper must not be reported: {report:?}"
    );
    assert!(
        report.surfaces.is_empty(),
        "the plain credential helper must not be reported: {report:?}"
    );
    assert!(report.is_clean());
}

#[test]
fn a_credential_secret_never_appears_in_the_report() {
    // A `credential.<url>.helper` value can carry a password and the
    // subsection can carry a URL with credentials. The report carries surface
    // names and key *templates* only, so it cannot leak either.
    let secret = common::secret_scan::unique_sentinel("credential_helper");
    let repo = repository_with_config(&[(
        "config",
        &format!(
            "[credential \"https://user:{secret}@example.com\"]\n\thelper = store --password={secret}\n"
        ),
    )]);

    let report: HostileConfigReport = inspect_repository_config_surface(repo.path());

    assert!(
        report.has_surface(HostileConfigSurface::UrlScopedCredentialHelper),
        "the URL-scoped credential helper was not reported: {report:?}"
    );
    assert!(
        !report.summary.contains(&secret),
        "the summary leaked the secret: {}",
        report.summary
    );
    assert!(
        !format!("{report:?}").contains(&secret),
        "the debug rendering leaked the secret: {report:?}"
    );
    // The report names the key template only.
    assert!(
        report.summary.contains("credential.<url>.helper"),
        "the summary must name the key template: {}",
        report.summary
    );
}
