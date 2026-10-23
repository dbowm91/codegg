//! Bounded, read-only detection of a repository's hostile Git configuration.
//!
//! A Git *read* can execute code. `git status` consults the clean/smudge
//! filter machinery, and a repository that declares
//! `filter.<name>.process` selects a **long-running filter process** whenever
//! an attribute in the worktree names that filter. The same is true for a
//! user-level `core.attributesFile`, which can select a filter without the
//! repository having to name it.
//!
//! The trusted read route in [`crate::process`] neutralises a *list* of
//! repository-controlled config surfaces by injecting `-c key=value` overrides
//! that beat every config file, and by scrubbing `GIT_*` environment
//! variables. That list is not general: an override beats the keys it names
//! and nothing else. Two gaps follow, and this module exists to close them:
//!
//! 1. **The content of an included file is still parsed by Git.** A
//!    `include.path` or `includeIf.<condition>.path` directive names another
//!    config file whose keys Git honours. A dangerous key hidden inside such a
//!    file applies, and the override list does not name it.
//! 2. **The override list is a property of the publisher's private gitdir,
//!    not of a live read of the repository's own configuration.** The
//!    mediation must be derived from the repository's own config files,
//!    whatever they contain.
//!
//! This module is a **detector**, not an enforcer: it reads the repository's
//! configuration, follows the include graph, and reports which dangerous
//! surfaces the repository declares. The caller decides what to do with the
//! report.
//!
//! ## What this module guarantees
//!
//! * **It never executes anything.** The configuration is parsed here, in
//!   Rust. It does not shell out to `git config` — that command would honour
//!   includes and could consult helpers — and it never runs a filter, driver,
//!   pager, editor, or helper of any kind.
//! * **It never leaks a value.** The report carries *surface names*, not
//!   values. A `credential.<url>.helper` value can contain a password and an
//!   `url.<base>.insteadOf` base can contain a URL with credentials, so the
//!   summary only ever names the key **template** (`credential.<url>.helper`)
//!   and never the resolved subsection or the value.
//! * **It never panics on a hostile file.** A malformed configuration is
//!   reported as [`HostileConfigSurface::InspectionTruncated`]: the file
//!   could not be vouched for, so the inspection is incomplete.
//! * **It is bounded.** See [`MAX_INCLUDE_DEPTH`], [`MAX_CONFIG_FILES`],
//!   [`MAX_CONFIG_FILE_BYTES`] and [`MAX_TOTAL_CONFIG_BYTES`]. A bound that
//!   is exceeded stops the walk and is reported; it never turns into an
//!   unbounded read.
//!
//! ## What this module does not guarantee
//!
//! The detector is a **pre-flight check on one snapshot**, not a guarantee
//! about the bytes Git will read later. Two residuals are worth naming:
//!
//! * **Time-of-check to time-of-use.** The repository's configuration can
//!   change between the inspection and the Git invocation it was meant to
//!   guard. A detector cannot close that window on its own; the caller has to
//!   treat the report as advisory or re-check.
//! * **Includes outside the bounded root set.** An `include.path` that
//!   resolves outside the bounded root set is reported as
//!   [`HostileConfigSurface::OutOfBoundsInclude`]. Its content is still
//!   inspected when it is an ordinary regular file, because refusing to look
//!   is the hole this detector exists to close; a target that is not a
//!   regular file is never opened, and the report is truncated.
//!
//! The report therefore distinguishes "the repository declares no dangerous
//! surface" from "the repository's configuration could not be fully
//! inspected". [`HostileConfigReport::is_clean`] is true only for the former.

use std::collections::BTreeSet;
use std::fmt;
use std::fs;
use std::io;
use std::io::Read;
use std::path::{Path, PathBuf};

/// Maximum number of include levels that are followed.
///
/// A legitimate configuration nests at most a handful of levels deep
/// (system → global → repository → worktree is already three). The bound
/// stops a hostile repository from building an include graph that never
/// terminates. Exceeding it stops the walk and sets the truncated flag.
pub const MAX_INCLUDE_DEPTH: usize = 4;

/// Maximum number of configuration files that may be read in one inspection.
///
/// The include graph can also *fan out*: one file may name many others, so
/// the number of files is bounded independently of the depth. Exceeding the
/// bound stops the walk and sets the truncated flag.
pub const MAX_CONFIG_FILES: usize = 32;

/// Maximum size of a single configuration file.
///
/// A configuration file is a few kilobytes in practice. The size is checked
/// **before** the file is read, so an oversized file is refused rather than
/// buffered, and the read itself is capped at the same bound in case the
/// file grows between the check and the read.
pub const MAX_CONFIG_FILE_BYTES: u64 = 1024 * 1024;

/// Maximum number of bytes read across every inspected configuration file.
///
/// The per-file bound alone would still permit `MAX_CONFIG_FILES *
/// MAX_CONFIG_FILE_BYTES` bytes of reading. This aggregate bound keeps the
/// total work of one inspection small and predictable. Exceeding it stops the
/// walk and sets the truncated flag.
pub const MAX_TOTAL_CONFIG_BYTES: u64 = 8 * 1024 * 1024;

/// A repository-controlled configuration surface that can cause code
/// execution during a Git read.
///
/// The vocabulary is closed: every variant names a *kind* of surface, never a
/// value and never a resolved subsection, so a report built from it cannot
/// carry a secret.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum HostileConfigSurface {
    /// `filter.<name>.process`, `filter.<name>.clean` or
    /// `filter.<name>.smudge` — the filter commands Git runs to convert
    /// content to and from its object form.
    ///
    /// This is the surface that matters most for a *read*. `git status` and
    /// `git diff` are on the trusted read route, and both compare the working
    /// tree against the index *through the clean filter*, so a repository
    /// that declares a filter command and ships an attribute that selects it
    /// gets that command executed by an ordinary status read. The
    /// `process` form is worse still: it is a long-running filter process
    /// that Git starts and speaks a protocol to.
    FilterCommand,
    /// `core.attributesFile` — a user-level attributes file that can select
    /// a filter or a diff driver.
    GlobalAttributesFile,
    /// `core.alternateRefsCommand` — a repository-configured command.
    AlternateRefsCommand,
    /// `core.sshCommand` — a repository-configured ssh command.
    SshCommand,
    /// `credential.<url>.helper` — the URL-scoped credential helper form,
    /// which `credential.helper=` does not clear.
    ///
    /// The plain `credential.helper` key *is* cleared by the trusted route's
    /// `-c credential.helper=` override. The URL-scoped form is a different
    /// key and survives that override, so it is reported separately.
    UrlScopedCredentialHelper,
    /// `url.<base>.insteadOf` — URL rewriting.
    UrlInsteadOf,
    /// `merge.<name>.driver` — a repository-configured merge driver.
    MergeDriver,
    /// `diff.<name>.command` — a repository-configured diff driver.
    DiffCommand,
    /// A repository-configured `core.pager` / `core.editor` /
    /// `sequence.editor`.
    PagerOrEditor,
    /// An `include.path` / `includeIf.<condition>.path` directive named a file
    /// outside the bounded root set.
    ///
    /// The content of an ordinary regular file is still inspected — the
    /// trusted route's `-c` overrides beat the keys they *name*, so a filter
    /// command that lives only in an included file is reachable from a read,
    /// and refusing to look would be exactly the hole this detector exists to
    /// close. The surface is reported because a configuration that reaches
    /// outside the repository is itself worth naming.
    ///
    /// When the target is **not** an ordinary regular file — a fifo, a
    /// device or a socket — it is not opened at all, and the report is also
    /// truncated.
    OutOfBoundsInclude,
    /// The config was not fully inspected because a bound was exceeded.
    ///
    /// A malformed configuration file also produces this surface: the file
    /// could not be vouched for, so the inspection is incomplete. The
    /// truncated flag of the report is set at the same time.
    InspectionTruncated,
}

impl HostileConfigSurface {
    /// A stable machine name for the surface.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::FilterCommand => "filter_command",
            Self::GlobalAttributesFile => "global_attributes_file",
            Self::AlternateRefsCommand => "alternate_refs_command",
            Self::SshCommand => "ssh_command",
            Self::UrlScopedCredentialHelper => "url_scoped_credential_helper",
            Self::UrlInsteadOf => "url_instead_of",
            Self::MergeDriver => "merge_driver",
            Self::DiffCommand => "diff_command",
            Self::PagerOrEditor => "pager_or_editor",
            Self::OutOfBoundsInclude => "out_of_bounds_include",
            Self::InspectionTruncated => "inspection_truncated",
        }
    }

    /// The config **key template** this surface corresponds to.
    ///
    /// The template is a compile-time constant that names the key shape
    /// only: the concrete subsection and the value are never part of it, so
    /// a template can never carry a password, a URL credential, or any other
    /// repository-controlled text. This is what makes the report's summary
    /// safe to log.
    pub fn key_template(&self) -> &'static str {
        match self {
            Self::FilterCommand => "filter.<name>.{process|clean|smudge}",
            Self::GlobalAttributesFile => "core.attributesFile",
            Self::AlternateRefsCommand => "core.alternateRefsCommand",
            Self::SshCommand => "core.sshCommand",
            Self::UrlScopedCredentialHelper => "credential.<url>.helper",
            Self::UrlInsteadOf => "url.<base>.insteadOf",
            Self::MergeDriver => "merge.<name>.driver",
            Self::DiffCommand => "diff.<name>.command",
            Self::PagerOrEditor => "core.pager/core.editor/sequence.editor",
            Self::OutOfBoundsInclude => "include.path/includeIf.<condition>.path",
            Self::InspectionTruncated => "",
        }
    }
}

impl fmt::Display for HostileConfigSurface {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// The result of inspecting a repository's configuration surface.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostileConfigReport {
    /// The distinct dangerous surfaces found, sorted deterministically.
    ///
    /// [`HostileConfigSurface::InspectionTruncated`] is present exactly when
    /// [`HostileConfigReport::truncated`] is true, so a consumer that only
    /// reads this list still sees that the inspection was incomplete.
    pub surfaces: Vec<HostileConfigSurface>,
    /// Whether the inspection hit a bound and therefore may be incomplete.
    pub truncated: bool,
    /// A human-readable, secret-free summary. Never include file contents.
    ///
    /// The summary names key **templates** and counts. It never contains a
    /// configuration value, a resolved subsection, or a file path.
    pub summary: String,
}

impl HostileConfigReport {
    /// Whether the repository's configuration was fully inspected and
    /// declares no dangerous surface.
    ///
    /// `false` means *either* a dangerous surface was found *or* the
    /// inspection was incomplete. Use [`HostileConfigReport::has_surface`] to
    /// tell those apart.
    pub fn is_clean(&self) -> bool {
        self.surfaces.is_empty() && !self.truncated
    }

    /// Whether the report carries the given surface.
    pub fn has_surface(&self, surface: HostileConfigSurface) -> bool {
        self.surfaces.contains(&surface)
    }
}

/// Inspect the repository at `repository_root` and report the dangerous
/// configuration surfaces it declares.
///
/// The inspection is read-only and never executes anything. It reads the
/// repository's own `config` and, when present, `config.worktree`, follows
/// `include.path` and `includeIf.<condition>.path` directives within the
/// documented bounds, and classifies the keys it finds.
///
/// A repository that cannot be located at all yields an empty report with
/// [`HostileConfigReport::truncated`] set to `false`: there is no
/// configuration to be hostile.
pub fn inspect_repository_config_surface(repository_root: &Path) -> HostileConfigReport {
    let Some(layout) = RepositoryLayout::discover(repository_root) else {
        return HostileConfigReport {
            surfaces: Vec::new(),
            truncated: false,
            summary: "no Git repository configuration found; nothing to inspect".to_string(),
        };
    };

    let mut inspector = Inspector::new(&layout);
    for config_file in &layout.config_files {
        inspector.inspect_file(config_file, 0, &[]);
    }
    inspector.finish()
}

// ---------------------------------------------------------------------------
// Repository layout
// ---------------------------------------------------------------------------

/// The configuration files that make up a repository's own configuration.
struct RepositoryLayout {
    /// The canonical repository root (the worktree).
    root: PathBuf,
    /// The canonical per-worktree git directory.
    git_dir: PathBuf,
    /// The canonical common directory (shared by all worktrees).
    common_dir: PathBuf,
    /// The configuration files to inspect, canonical and de-duplicated.
    config_files: Vec<PathBuf>,
}

impl RepositoryLayout {
    /// Locate the repository's own configuration files without running Git.
    ///
    /// The git directory is found from the `.git` entry of the root: a
    /// directory for an ordinary repository, a `gitdir:` pointer file for a
    /// linked worktree or a submodule. A bare repository is recognised by
    /// its own `HEAD` and `objects`.
    fn discover(repository_root: &Path) -> Option<Self> {
        let root = fs::canonicalize(repository_root).ok()?;
        let dot_git = root.join(".git");
        let git_dir = if dot_git.is_dir() {
            dot_git
        } else if dot_git.is_file() {
            let contents = fs::read_to_string(&dot_git).ok()?;
            let target = contents.lines().find_map(|line| {
                let rest = line.trim().strip_prefix("gitdir:")?;
                Some(rest.trim().to_string())
            })?;
            if target.is_empty() {
                return None;
            }
            let candidate = PathBuf::from(&target);
            if candidate.is_absolute() {
                candidate
            } else {
                root.join(candidate)
            }
        } else if root.join("HEAD").is_file() && root.join("objects").is_dir() {
            // A bare repository: the root itself is the git directory.
            root.clone()
        } else {
            return None;
        };
        let git_dir = fs::canonicalize(&git_dir).ok()?;
        let common_dir = read_common_dir(&git_dir);

        // `config` lives in the common directory; `config.worktree` lives in
        // the per-worktree git directory and holds the worktree-local
        // overrides. A linked worktree has no git-directory-local `config`.
        let mut config_files = Vec::new();
        for candidate in [
            common_dir.join("config"),
            git_dir.join("config"),
            git_dir.join("config.worktree"),
        ] {
            let Ok(canonical) = fs::canonicalize(&candidate) else {
                continue;
            };
            if !config_files.contains(&canonical) {
                config_files.push(canonical);
            }
        }
        if config_files.is_empty() {
            return None;
        }

        Some(Self {
            root,
            git_dir,
            common_dir,
            config_files,
        })
    }

    /// The roots an `include.path` may resolve inside.
    fn include_roots(&self) -> Vec<PathBuf> {
        let mut roots = vec![
            self.root.clone(),
            self.git_dir.clone(),
            self.common_dir.clone(),
        ];
        roots.sort();
        roots.dedup();
        roots
    }
}

/// Read the `commondir` pointer of a git directory.
fn read_common_dir(git_dir: &Path) -> PathBuf {
    let Ok(contents) = fs::read_to_string(git_dir.join("commondir")) else {
        return git_dir.to_path_buf();
    };
    let target = contents.lines().next().unwrap_or("").trim();
    if target.is_empty() {
        return git_dir.to_path_buf();
    }
    let candidate = PathBuf::from(target);
    let path = if candidate.is_absolute() {
        candidate
    } else {
        git_dir.join(candidate)
    };
    fs::canonicalize(&path).unwrap_or_else(|_| git_dir.to_path_buf())
}

// ---------------------------------------------------------------------------
// The inspection
// ---------------------------------------------------------------------------

struct Inspector {
    /// The roots an include may resolve inside.
    roots: Vec<PathBuf>,
    /// The canonical paths already inspected, so a file included twice is
    /// read once.
    inspected: BTreeSet<PathBuf>,
    /// The distinct surfaces found.
    surfaces: BTreeSet<HostileConfigSurface>,
    /// How many files were read.
    files_inspected: usize,
    /// How many includes were resolved and followed.
    includes_followed: usize,
    /// How many bytes were read in total.
    bytes_read: u64,
    /// Whether a bound was hit or a file could not be vouched for.
    truncated: bool,
}

impl Inspector {
    fn new(layout: &RepositoryLayout) -> Self {
        Self {
            roots: layout.include_roots(),
            inspected: BTreeSet::new(),
            surfaces: BTreeSet::new(),
            files_inspected: 0,
            includes_followed: 0,
            bytes_read: 0,
            truncated: false,
        }
    }

    /// Read one configuration file and recurse into the includes it names.
    ///
    /// `chain` is the sequence of files currently being read, so an include
    /// cycle is detected as a cycle rather than followed until a bound is
    /// hit.
    fn inspect_file(&mut self, path: &Path, depth: usize, chain: &[PathBuf]) {
        if depth > MAX_INCLUDE_DEPTH {
            self.truncated = true;
            return;
        }
        if self.files_inspected >= MAX_CONFIG_FILES {
            self.truncated = true;
            return;
        }
        let Ok(metadata) = fs::metadata(path) else {
            self.truncated = true;
            return;
        };
        if !metadata.is_file() {
            self.truncated = true;
            return;
        }
        // The size is checked before the file is read so an oversized file
        // is refused rather than buffered.
        if metadata.len() > MAX_CONFIG_FILE_BYTES {
            self.truncated = true;
            return;
        }
        let Ok(bytes) = read_bounded(path, MAX_CONFIG_FILE_BYTES) else {
            self.truncated = true;
            return;
        };
        self.files_inspected += 1;
        self.bytes_read += bytes.len() as u64;
        self.inspected.insert(path.to_path_buf());
        if self.bytes_read > MAX_TOTAL_CONFIG_BYTES {
            self.truncated = true;
            return;
        }

        let text = String::from_utf8_lossy(&bytes);
        let parsed = parse_config_text(&text);
        if parsed.malformed {
            // The file could not be vouched for: the inspection is
            // incomplete, whatever else was found in it.
            self.truncated = true;
        }
        for entry in &parsed.entries {
            if let Some(surface) = classify_entry(entry) {
                self.surfaces.insert(surface);
            }
        }
        for include in &parsed.includes {
            self.follow_include(include, path, depth, chain);
        }
    }

    /// Resolve and follow one include directive.
    ///
    /// `including_file` is the file that names the include: Git resolves a
    /// relative include against the directory of that file.
    fn follow_include(
        &mut self,
        include: &ParsedInclude,
        including_file: &Path,
        depth: usize,
        chain: &[PathBuf],
    ) {
        let Some(resolved) = resolve_include_path(&include.path, Some(including_file)) else {
            self.truncated = true;
            return;
        };
        let canonical = match fs::canonicalize(&resolved) {
            Ok(path) => path,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                // The file does not exist, so there is nothing in it to be
                // hostile. Git would fail to read it too.
                return;
            }
            Err(_) => {
                // The path could not be resolved safely. Refuse it rather
                // than guess.
                self.truncated = true;
                return;
            }
        };
        if !self.is_within_roots(&canonical) {
            // The repository asked Git to read a file outside the bounded
            // root set. The content is still inspected when it is an ordinary
            // regular file, because "we did not look" is exactly the state
            // that lets a dangerous key hide: the trusted route's `-c`
            // overrides beat the keys they *name*, so a `filter` command
            // that lives only in an included file is reachable from a read.
            //
            // The surface is still reported, because a configuration that
            // reaches outside the repository is itself worth naming. What is
            // NOT done is refusing to look: the walk continues, bounded.
            match fs::metadata(&canonical) {
                Ok(metadata) if metadata.is_file() => {
                    self.surfaces
                        .insert(HostileConfigSurface::OutOfBoundsInclude);
                }
                _ => {
                    // Not an ordinary file — a fifo, a device or a socket.
                    // Opening it could block, so it is refused outright and
                    // the inspection is declared incomplete.
                    self.surfaces
                        .insert(HostileConfigSurface::OutOfBoundsInclude);
                    self.truncated = true;
                    return;
                }
            }
        }
        if chain.contains(&canonical) {
            // An include cycle. The walk stops here instead of following it.
            self.truncated = true;
            return;
        }
        if self.inspected.contains(&canonical) {
            // Already read: the content was inspected once.
            return;
        }
        let child_depth = depth + 1;
        if child_depth > MAX_INCLUDE_DEPTH {
            self.truncated = true;
            return;
        }
        let mut next_chain = chain.to_vec();
        next_chain.push(canonical.clone());
        self.includes_followed += 1;
        self.inspect_file(&canonical, child_depth, &next_chain);
    }

    fn is_within_roots(&self, path: &Path) -> bool {
        self.roots.iter().any(|root| path.starts_with(root))
    }

    fn finish(self) -> HostileConfigReport {
        let mut surfaces: Vec<HostileConfigSurface> = self.surfaces.into_iter().collect();
        if self.truncated && !surfaces.contains(&HostileConfigSurface::InspectionTruncated) {
            surfaces.push(HostileConfigSurface::InspectionTruncated);
        }
        surfaces.sort();
        let summary = build_summary(
            &surfaces,
            self.files_inspected,
            self.includes_followed,
            self.truncated,
        );
        HostileConfigReport {
            surfaces,
            truncated: self.truncated,
            summary,
        }
    }
}

/// Read at most `limit` bytes from `path`, refusing a file that is larger.
fn read_bounded(path: &Path, limit: u64) -> io::Result<Vec<u8>> {
    let file = fs::File::open(path)?;
    let mut reader = file.take(limit + 1);
    let mut bytes = Vec::new();
    io::Read::read_to_end(&mut reader, &mut bytes)?;
    if bytes.len() as u64 > limit {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "configuration file exceeds the read bound",
        ));
    }
    Ok(bytes)
}

/// Build the human-readable, secret-free summary.
fn build_summary(
    surfaces: &[HostileConfigSurface],
    files_inspected: usize,
    includes_followed: usize,
    truncated: bool,
) -> String {
    let mut summary = String::new();
    if surfaces.is_empty() {
        summary.push_str("no hostile Git configuration surface detected");
    } else {
        let templates: Vec<&str> = surfaces
            .iter()
            .map(HostileConfigSurface::key_template)
            .filter(|template| !template.is_empty())
            .collect();
        summary.push_str("hostile Git configuration surface detected: ");
        summary.push_str(&templates.join(", "));
    }
    summary.push_str(&format!(
        "; inspected {files_inspected} configuration file(s)"
    ));
    if includes_followed > 0 {
        summary.push_str(&format!("; followed {includes_followed} include(s)"));
    }
    summary.push_str(if truncated {
        "; inspection incomplete (a bound was reached or content could not be parsed)"
    } else {
        "; inspection complete"
    });
    summary
}

/// Resolve an `include.path` value to a filesystem path.
///
/// Git resolves a relative include against the directory of the file that
/// names it, and expands a leading `~` against the user's home directory.
/// The resolved path is *not* checked against the bounded roots here; that
/// check happens after symlink resolution.
fn resolve_include_path(raw: &str, including_file: Option<&Path>) -> Option<PathBuf> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    if trimmed == "~" {
        return home_directory();
    }
    if let Some(rest) = trimmed.strip_prefix("~/") {
        return home_directory().map(|home| home.join(rest));
    }
    let candidate = Path::new(trimmed);
    if candidate.is_absolute() {
        return Some(candidate.to_path_buf());
    }
    let base = including_file?.parent()?;
    Some(base.join(candidate))
}

/// The user's home directory, used to expand a leading `~` in an include.
fn home_directory() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .filter(|path| !path.as_os_str().is_empty())
}

// ---------------------------------------------------------------------------
// Configuration parsing
// ---------------------------------------------------------------------------

/// One parsed configuration entry.
///
/// The section and the key are lower-cased, matching Git's own treatment:
/// section and variable names are case-insensitive while a subsection is
/// case-sensitive. The subsection is kept verbatim because it is the part
/// that can carry a URL with credentials — and it is never rendered.
#[derive(Clone, PartialEq, Eq)]
struct ConfigEntry {
    section: String,
    subsection: Option<String>,
    key: String,
    value: String,
}

impl ConfigEntry {
    /// The canonical `section.subsection.key` name, for internal matching.
    ///
    /// Only the tests need this name: the production path never renders it,
    /// because the subsection can carry a URL with credentials.
    #[cfg(test)]
    fn canonical_name(&self) -> String {
        match &self.subsection {
            Some(subsection) => format!("{}.{}.{}", self.section, subsection, self.key),
            // Git accepts a key before any section header and stores it
            // without a section.
            None if self.section.is_empty() => self.key.clone(),
            None => format!("{}.{}", self.section, self.key),
        }
    }
}

impl fmt::Debug for ConfigEntry {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        // The value is never rendered, and neither is the subsection: a
        // `credential.<url>.helper` subsection can contain a URL with a
        // password. Even an internal debug rendering stays secret-free.
        let name = match &self.subsection {
            Some(_) => format!("{}.<subsection>.{}", self.section, self.key),
            None => format!("{}.{}", self.section, self.key),
        };
        formatter
            .debug_struct("ConfigEntry")
            .field("name", &name)
            .field("value", &"<redacted>")
            .finish()
    }
}

/// An include directive and the path it names.
#[derive(Debug, Clone, PartialEq, Eq)]
struct ParsedInclude {
    path: String,
}

/// The result of parsing one configuration file.
#[derive(Debug, Default)]
struct ParsedConfig {
    /// Every entry that is not an include directive.
    entries: Vec<ConfigEntry>,
    /// Every include directive, with the path it names.
    includes: Vec<ParsedInclude>,
    /// Whether the file could not be parsed in full.
    malformed: bool,
}

/// Parse the text of a Git configuration file.
///
/// The grammar implemented here is Git's own, verified against `git config`
/// for the cases that matter to a detector:
///
/// * `[section]`, `[section "subsection"]` and `[section.subsection]`
///   headers; the section and the key are lower-cased, the subsection is
///   kept verbatim.
/// * `key = value`, `key` (which means `true`), quoted values, and the
///   escape sequences `\n`, `\t`, `\b`, `\\` and `\"`.
/// * Whole-line comments introduced by `#` or `;`, and — as Git does — an
///   inline `#` or `;` that ends an unquoted value.
/// * Line continuations: a line ending in an odd number of backslashes
///   continues onto the next line; a comment line never continues.
///
/// A file that does not parse is reported as malformed rather than being
/// partially trusted.
fn parse_config_text(text: &str) -> ParsedConfig {
    let mut parsed = ParsedConfig::default();
    let mut section: Option<(String, Option<String>)> = None;

    for logical in logical_lines(text) {
        let trimmed = logical.trim_start();
        if trimmed.is_empty() {
            continue;
        }
        let first = trimmed.as_bytes()[0];
        if first == b'#' || first == b';' {
            continue;
        }
        if first == b'[' {
            match parse_section_header(trimmed) {
                Ok(header) => section = Some(header),
                Err(()) => {
                    parsed.malformed = true;
                    break;
                }
            }
            continue;
        }
        let (key, value) = match parse_key_value(trimmed) {
            Ok(pair) => pair,
            Err(()) => {
                parsed.malformed = true;
                break;
            }
        };
        let (section_name, subsection) = match &section {
            Some((name, subsection)) => (name.clone(), subsection.clone()),
            // Git accepts a key before any section header; it is stored
            // without a section.
            None => (String::new(), None),
        };
        let entry = ConfigEntry {
            section: section_name,
            subsection,
            key,
            value: value.clone(),
        };
        if is_include_directive(&entry) {
            parsed.includes.push(ParsedInclude { path: value });
        } else {
            parsed.entries.push(entry);
        }
    }
    parsed
}

/// Split the file into logical lines, applying Git's continuation rule.
///
/// A physical line that ends in an odd number of backslashes continues onto
/// the next physical line. A comment line never continues: Git ends a
/// comment at the newline and reads the next line afresh, so a pending
/// value is closed when a comment line is reached.
fn logical_lines(text: &str) -> Vec<String> {
    let mut lines = Vec::new();
    let mut pending = String::new();
    for physical in text.split('\n') {
        let physical = physical.strip_suffix('\r').unwrap_or(physical);
        let trimmed = physical.trim_start();
        let is_comment = matches!(trimmed.as_bytes().first(), Some(b'#') | Some(b';'));
        if is_comment {
            if !pending.is_empty() {
                lines.push(std::mem::take(&mut pending));
            }
            continue;
        }
        if ends_with_continuation(physical) {
            // The trailing backslash is the continuation marker; the escape
            // processing of the value sees the backslashes before it.
            pending.push_str(&physical[..physical.len() - 1]);
            continue;
        }
        pending.push_str(physical);
        lines.push(std::mem::take(&mut pending));
    }
    if !pending.is_empty() {
        lines.push(pending);
    }
    lines
}

/// Whether a physical line ends in an odd number of backslashes.
fn ends_with_continuation(line: &str) -> bool {
    let trailing = line.chars().rev().take_while(|ch| *ch == '\\').count();
    trailing % 2 == 1
}

/// Parse a `[section]`, `[section "subsection"]` or `[section.subsection]`
/// header.
fn parse_section_header(line: &str) -> Result<(String, Option<String>), ()> {
    let rest = &line[1..];
    let Some(end) = rest.find(']') else {
        return Err(());
    };
    let inner = rest[..end].trim();
    if inner.is_empty() {
        return Err(());
    }
    let name_end = inner
        .find(|ch: char| !(ch.is_ascii_alphanumeric() || ch == '-'))
        .unwrap_or(inner.len());
    let section = &inner[..name_end];
    if section.is_empty() {
        return Err(());
    }
    let after = inner[name_end..].trim_start();
    if after.is_empty() {
        return Ok((section.to_ascii_lowercase(), None));
    }
    if let Some(tail) = after.strip_prefix('"') {
        let (subsection, consumed) = parse_quoted(tail)?;
        if !tail[consumed..].trim().is_empty() {
            return Err(());
        }
        return Ok((section.to_ascii_lowercase(), Some(subsection)));
    }
    if let Some(tail) = after.strip_prefix('.') {
        // The dot form names the subsection as everything after the first
        // dot.
        return Ok((section.to_ascii_lowercase(), Some(tail.to_string())));
    }
    Err(())
}

/// Parse a `key = value` pair, or a bare key (which means `true`).
fn parse_key_value(line: &str) -> Result<(String, String), ()> {
    let name_end = line
        .find(|ch: char| !(ch.is_ascii_alphanumeric() || ch == '-'))
        .unwrap_or(line.len());
    let key = &line[..name_end];
    if key.is_empty() {
        return Err(());
    }
    let rest = line[name_end..].trim_start();
    if rest.is_empty() {
        return Ok((key.to_ascii_lowercase(), "true".to_string()));
    }
    let Some(after_equals) = rest.strip_prefix('=') else {
        return Err(());
    };
    let value = parse_value(after_equals)?;
    Ok((key.to_ascii_lowercase(), value))
}

/// Parse a configuration value.
fn parse_value(text: &str) -> Result<String, ()> {
    let trimmed = text.trim_start();
    if let Some(tail) = trimmed.strip_prefix('"') {
        let (value, _consumed) = parse_quoted(tail)?;
        return Ok(value);
    }
    let mut value = String::new();
    let mut chars = trimmed.chars();
    while let Some(ch) = chars.next() {
        match ch {
            '#' | ';' => break,
            '\\' => {
                let Some(escaped) = chars.next() else {
                    return Err(());
                };
                value.push(unescape(escaped)?);
            }
            other => value.push(other),
        }
    }
    while value.ends_with(' ') || value.ends_with('\t') {
        value.pop();
    }
    Ok(value)
}

/// Parse a quoted string, returning the value and the bytes consumed.
fn parse_quoted(text: &str) -> Result<(String, usize), ()> {
    let mut value = String::new();
    let mut chars = text.char_indices();
    while let Some((index, ch)) = chars.next() {
        match ch {
            '\\' => {
                let Some((_, escaped)) = chars.next() else {
                    return Err(());
                };
                value.push(unescape(escaped)?);
            }
            '"' => return Ok((value, index + 1)),
            other => value.push(other),
        }
    }
    Err(())
}

/// Apply one of Git's escape sequences.
fn unescape(ch: char) -> Result<char, ()> {
    match ch {
        'n' => Ok('\n'),
        't' => Ok('\t'),
        'b' => Ok('\u{8}'),
        '\\' => Ok('\\'),
        '"' => Ok('"'),
        _ => Err(()),
    }
}

// ---------------------------------------------------------------------------
// Classification
// ---------------------------------------------------------------------------

/// Whether an entry is an include directive.
///
/// Git follows exactly two forms, and this detector follows the same two:
///
/// * `include.path` — a plain include, which has **no** subsection.
/// * `includeIf.<condition>.path` — the condition is the subsection.
///
/// Any other shape is a different key, and Git does not follow it. In
/// particular `[include "x"] path` and `[includeIf] path` name keys Git
/// ignores, so treating them as directives would report a surface that
/// cannot be reached.
///
/// A *conditional* include is followed unconditionally. Git evaluates the
/// condition (`onbranch:`, `gitdir:`, `gitdir/i:`, `hasconfig:…`) before it
/// reads the file, and this detector cannot evaluate those conditions
/// faithfully. Reporting a surface Git would not have read is the safe
/// direction; missing one is not.
fn is_include_directive(entry: &ConfigEntry) -> bool {
    matches!(
        (
            entry.section.as_str(),
            entry.subsection.is_some(),
            entry.key.as_str(),
        ),
        ("include", false, "path") | ("includeif", true, "path")
    )
}

/// Classify an entry into a dangerous surface.
///
/// The section and the key are matched case-insensitively, exactly as Git
/// matches them. A subsection is required where the surface is *defined* by
/// the URL-scoped, base-scoped or named form of the key: the plain
/// `credential.helper` is cleared by the trusted route's
/// `-c credential.helper=` override and is therefore not a finding, while
/// `credential.<url>.helper` is a different key and is.
fn classify_entry(entry: &ConfigEntry) -> Option<HostileConfigSurface> {
    let named = entry
        .subsection
        .as_deref()
        .is_some_and(|subsection| !subsection.is_empty());
    match (entry.section.as_str(), named, entry.key.as_str()) {
        // `filter.<name>.process` starts a long-running filter process, and
        // `filter.<name>.clean` / `filter.<name>.smudge` are commands Git
        // runs to convert content to and from its object form. All three are
        // reachable from a **read**: `git status` and `git diff` compare the
        // working tree against the index through the clean filter, so a
        // repository that declares one gets its command executed by an
        // ordinary status read.
        ("filter", _, "process") | ("filter", _, "clean") | ("filter", _, "smudge") => {
            Some(HostileConfigSurface::FilterCommand)
        }
        ("core", _, "attributesfile") => Some(HostileConfigSurface::GlobalAttributesFile),
        ("core", _, "alternaterefscommand") => Some(HostileConfigSurface::AlternateRefsCommand),
        ("core", _, "sshcommand") => Some(HostileConfigSurface::SshCommand),
        ("core", _, "pager") | ("core", _, "editor") | ("sequence", _, "editor") => {
            Some(HostileConfigSurface::PagerOrEditor)
        }
        ("credential", true, "helper") => Some(HostileConfigSurface::UrlScopedCredentialHelper),
        ("url", true, "insteadof") => Some(HostileConfigSurface::UrlInsteadOf),
        ("merge", true, "driver") => Some(HostileConfigSurface::MergeDriver),
        ("diff", true, "command") => Some(HostileConfigSurface::DiffCommand),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    /// Build a repository whose git directory holds the given files.
    fn repository(files: &[(&str, &str)]) -> tempfile::TempDir {
        let dir = tempfile::tempdir().expect("tempdir");
        let git_dir = dir.path().join(".git");
        fs::create_dir_all(&git_dir).expect("git dir");
        for (name, contents) in files {
            fs::write(git_dir.join(name), contents).expect("write config");
        }
        dir
    }

    #[test]
    fn detects_filter_process_surface() {
        let dir = repository(&[("config", "[filter \"x\"]\n\tprocess = /bin/sh -c evil\n")]);
        let report = inspect_repository_config_surface(dir.path());
        assert!(report.has_surface(HostileConfigSurface::FilterCommand));
        assert!(!report.truncated);
        assert!(!report.is_clean());
    }

    #[test]
    fn detects_the_clean_and_smudge_filter_forms() {
        // `git status` and `git diff` compare the working tree against the
        // index *through the clean filter*, so the clean form is reachable
        // from an ordinary read just as the process form is. The smudge form
        // is the inverse and belongs to the same family.
        for key in ["clean", "smudge"] {
            let dir = repository(&[(
                "config",
                &format!("[filter \"x\"]\n\t{key} = /bin/sh -c evil\n"),
            )]);
            let report = inspect_repository_config_surface(dir.path());
            assert!(
                report.has_surface(HostileConfigSurface::FilterCommand),
                "filter.<name>.{key} must be reported as a filter command surface"
            );
        }
    }

    #[test]
    fn a_filter_section_without_a_command_is_not_a_surface() {
        // The section alone declares nothing executable. Reporting it would
        // refuse every repository that merely names a filter.
        let dir = repository(&[("config", "[filter \"x\"]\n\trequired = true\n")]);
        let report = inspect_repository_config_surface(dir.path());
        assert!(!report.has_surface(HostileConfigSurface::FilterCommand));
        assert!(report.is_clean());
    }

    #[test]
    fn detects_core_attributes_file_surface() {
        let dir = repository(&[("config", "[core]\n\tattributesFile = /tmp/attrs\n")]);
        let report = inspect_repository_config_surface(dir.path());
        assert!(report.has_surface(HostileConfigSurface::GlobalAttributesFile));
    }

    #[test]
    fn detects_url_scoped_credential_helper() {
        let dir = repository(&[(
            "config",
            "[credential \"https://example.com\"]\n\thelper = store --password=hunter2\n",
        )]);
        let report = inspect_repository_config_surface(dir.path());
        assert!(report.has_surface(HostileConfigSurface::UrlScopedCredentialHelper));
    }

    #[test]
    fn plain_credential_helper_is_not_a_surface() {
        // `credential.helper` is cleared by the trusted route's
        // `-c credential.helper=` override, so only the URL-scoped form is
        // a finding.
        let dir = repository(&[("config", "[credential]\n\thelper = store\n")]);
        let report = inspect_repository_config_surface(dir.path());
        assert!(!report.has_surface(HostileConfigSurface::UrlScopedCredentialHelper));
        assert!(report.is_clean());
    }

    #[test]
    fn detects_dangerous_key_inside_an_include_path_file() {
        let dir = repository(&[
            ("config", "[include]\n\tpath = extra.cfg\n"),
            ("extra.cfg", "[filter \"y\"]\n\tprocess = /bin/sh\n"),
        ]);
        let report = inspect_repository_config_surface(dir.path());
        assert!(report.has_surface(HostileConfigSurface::FilterCommand));
        assert!(!report.truncated);
    }

    #[test]
    fn detects_dangerous_key_inside_an_include_if_path_file() {
        let dir = repository(&[
            (
                "config",
                "[includeIf \"onbranch:main\"]\n\tpath = branch.cfg\n",
            ),
            ("branch.cfg", "[core]\n\tattributesFile = /tmp/attrs\n"),
        ]);
        let report = inspect_repository_config_surface(dir.path());
        assert!(report.has_surface(HostileConfigSurface::GlobalAttributesFile));
        assert!(!report.truncated);
    }

    #[test]
    fn include_cycle_terminates_and_is_truncated() {
        let dir = repository(&[
            ("config", "[include]\n\tpath = a.cfg\n"),
            ("a.cfg", "[include]\n\tpath = b.cfg\n"),
            ("b.cfg", "[include]\n\tpath = a.cfg\n"),
        ]);
        let report = inspect_repository_config_surface(dir.path());
        assert!(report.truncated);
        assert!(report.has_surface(HostileConfigSurface::InspectionTruncated));
    }

    #[test]
    fn include_depth_bound_is_enforced() {
        // A chain deeper than `MAX_INCLUDE_DEPTH`.
        let mut files: Vec<(String, String)> = vec![(
            "config".to_string(),
            "[include]\n\tpath = level0.cfg\n".to_string(),
        )];
        for index in 0..(MAX_INCLUDE_DEPTH + 3) {
            files.push((
                format!("level{index}.cfg"),
                format!("[include]\n\tpath = level{}.cfg\n", index + 1),
            ));
        }
        files.push((
            format!("level{}.cfg", MAX_INCLUDE_DEPTH + 3),
            "[filter \"deep\"]\n\tprocess = /bin/sh\n".to_string(),
        ));
        let borrowed: Vec<(&str, &str)> = files
            .iter()
            .map(|(name, contents)| (name.as_str(), contents.as_str()))
            .collect();
        let dir = repository(&borrowed);
        let report = inspect_repository_config_surface(dir.path());
        assert!(report.truncated);
        // The key beyond the depth bound was never reached.
        assert!(!report.has_surface(HostileConfigSurface::FilterCommand));
    }

    #[test]
    fn file_count_bound_is_enforced() {
        let mut files: Vec<(String, String)> = Vec::new();
        let mut includes = String::new();
        for index in 0..(MAX_CONFIG_FILES + 8) {
            files.push((
                format!("f{index}.cfg"),
                "[core]\n\tautocrlf = input\n".to_string(),
            ));
            includes.push_str(&format!("\tpath = f{index}.cfg\n"));
        }
        files.push(("config".to_string(), format!("[include]\n{includes}")));
        let borrowed: Vec<(&str, &str)> = files
            .iter()
            .map(|(name, contents)| (name.as_str(), contents.as_str()))
            .collect();
        let dir = repository(&borrowed);
        let report = inspect_repository_config_surface(dir.path());
        assert!(report.truncated);
        assert!(report.has_surface(HostileConfigSurface::InspectionTruncated));
    }

    #[test]
    fn oversized_config_file_is_refused_and_truncated() {
        let mut contents = String::from("[core]\n\tautocrlf = input\n");
        while contents.len() as u64 <= MAX_CONFIG_FILE_BYTES {
            contents.push_str("# padding padding padding padding padding padding\n");
        }
        let dir = repository(&[("config", &contents)]);
        let report = inspect_repository_config_surface(dir.path());
        assert!(report.truncated);
        assert!(report.has_surface(HostileConfigSurface::InspectionTruncated));
    }

    #[test]
    fn malformed_config_is_reported_without_panicking() {
        let dir = repository(&[("config", "[core\n\tkey = value\n")]);
        let report = inspect_repository_config_surface(dir.path());
        assert!(report.truncated);
        assert!(report.has_surface(HostileConfigSurface::InspectionTruncated));
    }

    #[test]
    fn malformed_config_after_a_dangerous_key_still_reports_the_key() {
        let dir = repository(&[("config", "[filter \"x\"]\n\tprocess = /bin/sh\n[core\n")]);
        let report = inspect_repository_config_surface(dir.path());
        assert!(report.has_surface(HostileConfigSurface::FilterCommand));
        assert!(report.truncated);
    }

    #[test]
    fn benign_config_reports_no_surfaces() {
        let dir = repository(&[(
            "config",
            "[user]\n\tname = Someone\n\temail = someone@example.com\n[core]\n\tautocrlf = input\n",
        )]);
        let report = inspect_repository_config_surface(dir.path());
        assert!(
            report.surfaces.is_empty(),
            "unexpected surfaces: {report:?}"
        );
        assert!(!report.truncated);
        assert!(report.is_clean());
    }

    #[test]
    fn report_never_contains_a_config_value() {
        let secret = "s3cr3t-passw0rd";
        let dir = repository(&[(
            "config",
            &format!(
                "[credential \"https://user:{secret}@example.com\"]\n\thelper = store --password={secret}\n\
                 [url \"https://token:{secret}@example.com/\"]\n\tinsteadOf = https://example.com/\n"
            ),
        )]);
        let report = inspect_repository_config_surface(dir.path());
        assert!(report.has_surface(HostileConfigSurface::UrlScopedCredentialHelper));
        assert!(report.has_surface(HostileConfigSurface::UrlInsteadOf));
        assert!(!report.summary.contains(secret), "summary leaked a value");
        assert!(
            !format!("{report:?}").contains(secret),
            "debug leaked a value"
        );
        // The key template is the only thing the summary may name.
        assert!(report.summary.contains("credential.<url>.helper"));
    }

    #[test]
    fn detects_url_instead_of_merge_driver_and_diff_command() {
        let dir = repository(&[(
            "config",
            "[url \"https://example.com/\"]\n\tinsteadOf = https://other.example.com/\n\
             [merge \"evil\"]\n\tdriver = /bin/sh\n\
             [diff \"evil\"]\n\tcommand = /bin/sh\n",
        )]);
        let report = inspect_repository_config_surface(dir.path());
        assert!(report.has_surface(HostileConfigSurface::UrlInsteadOf));
        assert!(report.has_surface(HostileConfigSurface::MergeDriver));
        assert!(report.has_surface(HostileConfigSurface::DiffCommand));
    }

    #[test]
    fn detects_ssh_command_alternate_refs_command_and_editors() {
        let dir = repository(&[(
            "config",
            "[core]\n\tsshCommand = /bin/sh\n\talternateRefsCommand = /bin/sh\n\
             [sequence]\n\teditor = /bin/sh\n",
        )]);
        let report = inspect_repository_config_surface(dir.path());
        assert!(report.has_surface(HostileConfigSurface::SshCommand));
        assert!(report.has_surface(HostileConfigSurface::AlternateRefsCommand));
        assert!(report.has_surface(HostileConfigSurface::PagerOrEditor));
    }

    #[test]
    fn worktree_config_is_inspected() {
        let dir = repository(&[
            ("config", "[user]\n\tname = Someone\n"),
            ("config.worktree", "[filter \"w\"]\n\tprocess = /bin/sh\n"),
        ]);
        let report = inspect_repository_config_surface(dir.path());
        assert!(report.has_surface(HostileConfigSurface::FilterCommand));
    }

    #[test]
    fn include_outside_the_repository_is_recorded_and_still_inspected() {
        // Refusing to look would be the hole this detector exists to close:
        // the trusted route's `-c` overrides beat the keys they *name*, so a
        // filter command that lives only in an included file is still
        // reachable from a read.
        let outside = tempfile::tempdir().expect("outside tempdir");
        let hostile = outside.path().join("hostile.cfg");
        fs::write(&hostile, "[filter \"x\"]\n\tprocess = /bin/sh\n").expect("write");
        let dir = repository(&[(
            "config",
            &format!("[include]\n\tpath = {}\n", hostile.display()),
        )]);
        let report = inspect_repository_config_surface(dir.path());
        assert!(report.has_surface(HostileConfigSurface::OutOfBoundsInclude));
        // The out-of-bounds content WAS inspected, so the inspection is
        // complete and what it declares is known.
        assert!(!report.truncated);
        assert!(report.has_surface(HostileConfigSurface::FilterCommand));
    }

    #[test]
    fn an_include_that_is_not_a_regular_file_is_not_opened() {
        // A fifo would block the detector forever, and Git would block on it
        // too. The inspection is declared incomplete instead.
        let outside = tempfile::tempdir().expect("outside tempdir");
        let fifo = outside.path().join("pipe");
        #[cfg(unix)]
        {
            let mut command = std::process::Command::new("mkfifo");
            command.arg(&fifo);
            assert!(command.status().expect("mkfifo").success());
        }
        let dir = repository(&[(
            "config",
            &format!("[include]\n\tpath = {}\n", fifo.display()),
        )]);
        let report = inspect_repository_config_surface(dir.path());
        assert!(report.has_surface(HostileConfigSurface::OutOfBoundsInclude));
        assert!(report.truncated);
        assert!(report.has_surface(HostileConfigSurface::InspectionTruncated));
    }

    #[test]
    fn missing_include_file_is_not_a_finding() {
        let dir = repository(&[("config", "[include]\n\tpath = absent.cfg\n")]);
        let report = inspect_repository_config_surface(dir.path());
        assert!(!report.has_surface(HostileConfigSurface::OutOfBoundsInclude));
        assert!(!report.truncated);
        assert!(report.is_clean());
    }

    #[test]
    fn repository_without_a_git_directory_reports_nothing() {
        let dir = tempfile::tempdir().expect("tempdir");
        let report = inspect_repository_config_surface(dir.path());
        assert!(report.surfaces.is_empty());
        assert!(!report.truncated);
    }

    #[test]
    fn parses_sections_subsections_and_keys() {
        let parsed = parse_config_text(
            "[Core]\n\tKey = value\n[filter \"Drv\"]\n\tprocess = /bin/sh\n\
             [a.b.c]\n\tk = v\n[bare]\n\tflag\n",
        );
        assert!(!parsed.malformed);
        let names: Vec<String> = parsed
            .entries
            .iter()
            .map(ConfigEntry::canonical_name)
            .collect();
        assert!(names.contains(&"core.key".to_string()));
        assert!(names.contains(&"filter.Drv.process".to_string()));
        assert!(names.contains(&"a.b.c.k".to_string()));
        assert!(names.contains(&"bare.flag".to_string()));
        let flag = parsed
            .entries
            .iter()
            .find(|entry| entry.canonical_name() == "bare.flag")
            .expect("bare flag");
        assert_eq!(flag.value, "true");
    }

    #[test]
    fn parses_quoted_values_escapes_and_inline_comments() {
        let parsed = parse_config_text(
            "[include]\n\tpath = \"/tmp/a b.cfg\"\n[core]\n\tkey = value # trailing\n",
        );
        assert!(!parsed.malformed);
        let include = parsed.includes.first().expect("include");
        assert_eq!(include.path, "/tmp/a b.cfg");
        let key = parsed
            .entries
            .iter()
            .find(|entry| entry.canonical_name() == "core.key")
            .expect("key");
        assert_eq!(key.value, "value");
    }

    #[test]
    fn parses_line_continuations() {
        let parsed = parse_config_text("[core]\n\tkey = a\\\nb\n");
        assert!(!parsed.malformed);
        let key = parsed
            .entries
            .iter()
            .find(|entry| entry.canonical_name() == "core.key")
            .expect("key");
        assert_eq!(key.value, "ab");
    }

    #[test]
    fn comment_line_does_not_swallow_the_next_line() {
        // Git ends a comment at the newline: the key on the next line is a
        // real key, and a detector that missed it would miss a hostile one.
        let parsed = parse_config_text("# comment \\\nkey = value\n");
        assert!(!parsed.malformed);
        let key = parsed
            .entries
            .iter()
            .find(|entry| entry.canonical_name() == "key")
            .expect("key");
        assert_eq!(key.value, "value");
    }

    #[test]
    fn unknown_escape_is_malformed() {
        let parsed = parse_config_text("[core]\n\tkey = a\\qb\n");
        assert!(parsed.malformed);
    }

    #[test]
    fn resolves_include_paths_against_the_including_file() {
        let including = Path::new("/repo/.git/config");
        assert_eq!(
            resolve_include_path("extra.cfg", Some(including)),
            Some(PathBuf::from("/repo/.git/extra.cfg"))
        );
        assert_eq!(
            resolve_include_path("/abs/extra.cfg", Some(including)),
            Some(PathBuf::from("/abs/extra.cfg"))
        );
        assert_eq!(resolve_include_path("", Some(including)), None);
    }

    #[test]
    fn expands_a_leading_tilde_in_an_include_path() {
        let home = home_directory().expect("home directory");
        assert_eq!(
            resolve_include_path("~/cfg", Some(Path::new("/repo/.git/config"))),
            Some(home.join("cfg"))
        );
    }

    #[test]
    fn only_gits_include_forms_are_treated_as_directives() {
        // Git follows `include.path` (no subsection) and
        // `includeIf.<condition>.path` (the condition is the subsection).
        // These two shapes name keys Git ignores, so following them would
        // report a surface that cannot be reached.
        let dir = repository(&[
            ("config", "[include \"x\"]\n\tpath = inc.cfg\n"),
            ("inc.cfg", "[filter \"y\"]\n\tprocess = /bin/sh\n"),
        ]);
        let report = inspect_repository_config_surface(dir.path());
        assert!(!report.has_surface(HostileConfigSurface::FilterCommand));

        let dir = repository(&[
            ("config", "[includeIf]\n\tpath = inc.cfg\n"),
            ("inc.cfg", "[filter \"y\"]\n\tprocess = /bin/sh\n"),
        ]);
        let report = inspect_repository_config_surface(dir.path());
        assert!(!report.has_surface(HostileConfigSurface::FilterCommand));
    }

    #[test]
    fn a_conditional_include_is_followed_even_when_its_condition_would_not_hold() {
        // Git evaluates an `includeIf` condition before reading the file.
        // The detector cannot evaluate `onbranch:` faithfully, so it reads
        // the file anyway: reporting a surface Git would not have read is
        // the safe direction.
        let dir = repository(&[
            (
                "config",
                "[includeIf \"onbranch:a-branch-that-does-not-exist\"]\n\tpath = branch.cfg\n",
            ),
            ("branch.cfg", "[filter \"y\"]\n\tprocess = /bin/sh\n"),
        ]);
        let report = inspect_repository_config_surface(dir.path());
        assert!(report.has_surface(HostileConfigSurface::FilterCommand));
    }

    #[test]
    fn surfaces_are_sorted_deterministically() {
        let dir = repository(&[(
            "config",
            "[diff \"d\"]\n\tcommand = /bin/sh\n[filter \"f\"]\n\tprocess = /bin/sh\n\
             [core]\n\tattributesFile = /tmp/a\n",
        )]);
        let report = inspect_repository_config_surface(dir.path());
        let mut sorted = report.surfaces.clone();
        sorted.sort();
        assert_eq!(report.surfaces, sorted);
        assert_eq!(report.surfaces[0], HostileConfigSurface::FilterCommand);
    }
}
