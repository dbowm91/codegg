//! Opt-in compiler for the Linux protected workspace mount view.
//!
//! The legacy Landlock policy remains the default. This module produces a
//! bounded Bubblewrap argv only for launch descriptions that explicitly ask
//! for the protected mount view. Host paths are opened by the helper and
//! passed with Bubblewrap's fd-based bind options; no workspace path is
//! re-resolved by Bubblewrap after policy compilation.

use std::collections::BTreeSet;
use std::path::{Component, Path, PathBuf};

use super::SandboxLaunchSpec;

const MAX_MOUNTS: usize = 512;
const MAX_PATH_BYTES: usize = 4096;
const MAX_REPORTED_SCOPE_BYTES: usize = 8192;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum MountAccess {
    ReadOnly,
    ReadWrite,
}

#[derive(Clone, Debug)]
struct BindMount {
    source: PathBuf,
    destination: PathBuf,
    access: MountAccess,
}

/// Compile policy roots into a deterministic, fail-closed mount command.
/// `open_path` is supplied by the one-shot helper so it can use `openat2`
/// with `RESOLVE_NO_SYMLINKS`; the returned descriptor stays alive until
/// Bubblewrap has completed setup.
pub fn compile(
    spec: &SandboxLaunchSpec,
    cwd: &Path,
    mut open_path: impl FnMut(&Path) -> Result<i32, String>,
) -> Result<(Vec<String>, Vec<i32>), String> {
    if !spec.linux_protected_mount_view {
        return Err("protected mount view was not requested".into());
    }
    verify_path_identities(spec)?;

    let mut read_roots = BTreeSet::new();
    for root in &spec.read_paths {
        let root = canonical_existing(root)?;
        if is_host_runtime_root(&root) {
            continue;
        }
        if root == Path::new("/") || root == Path::new("/home") {
            return Err("broad host root is not a valid mount grant".into());
        }
        read_roots.insert(root);
    }

    // `/etc` is never exposed as a tree. These files are enough for ordinary
    // libc name lookup and timezone behavior without making host credentials
    // or host Git configuration visible to the child.
    let mut explicit_etc = Vec::new();
    for name in [
        "resolv.conf",
        "hosts",
        "nsswitch.conf",
        "gai.conf",
        "localtime",
        "ld.so.cache",
        "ssl/certs/ca-certificates.crt",
        "pki/tls/certs/ca-bundle.crt",
    ] {
        let path = PathBuf::from("/etc").join(name);
        match std::fs::metadata(&path) {
            Ok(metadata) if metadata.is_file() => {
                explicit_etc.push((canonical_existing(&path)?, path));
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(format!(
                    "cannot inspect explicit runtime config file: {error}"
                ));
            }
        }
    }

    let mut write_roots = BTreeSet::new();
    for root in &spec.write_paths {
        if root.starts_with("/dev/") {
            continue;
        }
        let canonical = canonical_existing(root)?;
        if matches!(
            canonical.to_str(),
            Some(
                "/" | "/tmp"
                    | "/home"
                    | "/usr"
                    | "/var"
                    | "/opt"
                    | "/etc"
                    | "/run"
                    | "/proc"
                    | "/sys"
                    | "/dev"
            )
        ) {
            return Err("broad host directory is not a valid writable lease".into());
        }
        let metadata = std::fs::metadata(&canonical)
            .map_err(|error| format!("cannot inspect writable root: {error}"))?;
        if !metadata.is_dir() {
            return Err("protected mount view only accepts directory write roots".into());
        }
        write_roots.insert(canonical);
    }

    let mut read_only_metadata = BTreeSet::new();
    let mut future_git_paths = BTreeSet::new();
    let expected_git = spec
        .protected_readonly_paths
        .iter()
        .cloned()
        .collect::<BTreeSet<_>>();
    for root in &write_roots {
        let discovered = discover_git_metadata(root)?;
        let captured_root = spec
            .path_identities
            .iter()
            .any(|identity| identity.path == *root);
        for metadata_path in discovered {
            if captured_root && !expected_git.contains(&metadata_path) {
                return Err("Git metadata changed after the workspace lease was captured".into());
            }
            if !captured_root && metadata_path == root.join(".git") && metadata_path.exists() {
                return Err("unleased Git metadata appeared in a launch scratch root".into());
            }
            match std::fs::symlink_metadata(&metadata_path) {
                Ok(_) => {
                    let canonical = canonical_existing(&metadata_path)?;
                    if captured_root
                        && !spec
                            .path_identities
                            .iter()
                            .any(|identity| identity.path == canonical)
                    {
                        return Err(
                            "Git metadata appeared or changed after the workspace lease was captured"
                                .into(),
                        );
                    }
                    read_only_metadata.insert(canonical);
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    future_git_paths.insert(metadata_path);
                }
                Err(error) => {
                    return Err(format!(
                        "cannot inspect protected Git metadata {}: {error}",
                        metadata_path.display()
                    ));
                }
            }
        }
    }
    for protected in &spec.protected_readonly_paths {
        if std::fs::symlink_metadata(protected).is_ok() {
            let canonical = canonical_existing(protected)?;
            if !read_only_metadata.contains(&canonical) {
                return Err("captured Git metadata is not attached to a writable lease".into());
            }
        }
    }
    validate_no_nested_mounts(
        read_roots
            .iter()
            .chain(&write_roots)
            .chain(&read_only_metadata),
    )?;
    for metadata_root in &read_only_metadata {
        validate_protected_tree(metadata_root)?;
    }

    // Read grants that are also writable become one writable mount. Nested
    // read-only metadata is overlaid afterward. A read grant inside an
    // unrelated writable root is rejected because mount ordering would
    // otherwise make its effective authority depend on aliases.
    for read in &read_roots {
        if write_roots.iter().any(|write| read.starts_with(write))
            && !write_roots.contains(read)
            && !read_only_metadata
                .iter()
                .any(|protected| read.starts_with(protected))
        {
            return Err("read root nested beneath writable root is ambiguous".into());
        }
    }

    let mut mounts = Vec::new();
    for read in &read_roots {
        let access = if write_roots.contains(read) {
            MountAccess::ReadWrite
        } else {
            MountAccess::ReadOnly
        };
        mounts.push(BindMount {
            source: read.clone(),
            destination: read.clone(),
            access,
        });
    }
    for write in &write_roots {
        if !mounts.iter().any(|mount| mount.destination == *write) {
            mounts.push(BindMount {
                source: write.clone(),
                destination: write.clone(),
                access: MountAccess::ReadWrite,
            });
        }
    }
    for metadata in read_only_metadata {
        mounts.push(BindMount {
            source: metadata.clone(),
            destination: metadata,
            access: MountAccess::ReadOnly,
        });
    }
    for (source, destination) in explicit_etc {
        mounts.push(BindMount {
            source,
            destination,
            access: MountAccess::ReadOnly,
        });
    }
    // Several Linux toolchains are exposed through symlinks such as
    // /lib64 -> /usr/lib64. Canonical grant paths alone omit those names,
    // which can make an otherwise visible ELF binary fail to find its
    // interpreter. Recreate only aliases whose resolved source is already
    // covered by an explicit read-only runtime grant.
    for alias in ["/bin", "/sbin", "/lib", "/lib64"] {
        let alias = PathBuf::from(alias);
        let is_symlink = match std::fs::symlink_metadata(&alias) {
            Ok(metadata) => metadata.file_type().is_symlink(),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => false,
            Err(error) => return Err(format!("cannot inspect runtime path alias: {error}")),
        };
        if !is_symlink {
            continue;
        }
        let source = canonical_existing(&alias)?;
        if !read_roots.iter().any(|root| source.starts_with(root)) {
            continue;
        }
        if !mounts.iter().any(|mount| mount.destination == alias) {
            mounts.push(BindMount {
                source,
                destination: alias,
                access: MountAccess::ReadOnly,
            });
        }
    }
    mounts.sort_by(|left, right| {
        left.destination
            .components()
            .count()
            .cmp(&right.destination.components().count())
            .then_with(|| left.destination.cmp(&right.destination))
    });
    if mounts.len() > MAX_MOUNTS {
        return Err("protected mount view exceeds its mount-count bound".into());
    }
    enforcement_scope(spec)?;

    let mut hidden = BTreeSet::new();
    for path in &spec.deny_paths {
        if !denial_may_overlap_grant(path, &read_roots, &write_roots) {
            // The mount root is otherwise empty, so denies outside all
            // granted roots need no host-path lookup. This also avoids
            // probing unrelated credential paths that the daemon user may
            // not be allowed to stat (for example /root/.ssh on CI).
            continue;
        }
        let path = canonical_or_future(path)?;
        if write_roots.iter().any(|write| write.starts_with(&path))
            || read_roots.iter().any(|read| read.starts_with(&path))
        {
            return Err("protected path covers a granted mount root".into());
        }
        if write_roots.iter().any(|write| path.starts_with(write))
            || read_roots.iter().any(|read| path.starts_with(read))
        {
            hidden.insert(path);
        }
    }
    hidden.extend(future_git_paths);
    let hidden_all = hidden;
    let hidden = hidden_all
        .iter()
        .filter(|path| {
            !hidden_all
                .iter()
                .any(|parent| parent != *path && path.starts_with(parent))
        })
        .cloned()
        .collect::<BTreeSet<_>>();
    if mounts.len().saturating_add(hidden.len()) > MAX_MOUNTS {
        return Err("protected mount view exceeds its mount-operation bound".into());
    }

    // Mount destinations are constructed in a fresh tmpfs root. Directory
    // creation is explicit and parent-first; no host root is ever bind-mounted.
    let mut args = vec![
        "--unshare-user".to_string(),
        "--die-with-parent".to_string(),
        "--tmpfs".to_string(),
        "/".to_string(),
        "--dev".to_string(),
        "/dev".to_string(),
        "--tmpfs".to_string(),
        "/tmp".to_string(),
    ];

    let mut directories = BTreeSet::new();
    for mount in &mounts {
        add_parent_directories(&mount.destination, &mut directories)?;
        if std::fs::metadata(&mount.source)
            .map_err(|error| format!("cannot inspect mount source type: {error}"))?
            .is_dir()
        {
            directories.insert(mount.destination.clone());
        }
    }
    for path in &hidden {
        add_parent_directories(path, &mut directories)?;
        if !std::fs::metadata(path).is_ok_and(|metadata| metadata.is_file()) {
            directories.insert(path.clone());
        }
    }
    add_parent_directories(cwd, &mut directories)?;
    directories.insert(cwd.to_path_buf());
    if directories.len() > 4096 {
        return Err("protected mount view exceeds its directory-creation bound".into());
    }
    directories.remove(Path::new("/"));
    directories.remove(Path::new("/dev"));
    directories.remove(Path::new("/tmp"));
    for directory in directories {
        args.push("--dir".into());
        args.push(path_arg(&directory)?);
    }

    let mut fds = Vec::new();
    for mount in &mounts {
        let fd = open_path(&mount.source)?;
        fds.push(fd);
        args.push(match mount.access {
            MountAccess::ReadOnly => "--ro-bind-fd".into(),
            MountAccess::ReadWrite => "--bind-fd".into(),
        });
        args.push(fd.to_string());
        args.push(path_arg(&mount.destination)?);
    }

    for path in hidden {
        if std::fs::metadata(&path).is_ok_and(|metadata| metadata.is_file()) {
            // `/dev/null` supplies an empty file, never the protected file's
            // contents. Bubblewrap copies it into the view and mounts it ro.
            let empty_fd = open_path(Path::new("/dev/null"))?;
            fds.push(empty_fd);
            args.push("--ro-bind-data".into());
            args.push(empty_fd.to_string());
            args.push(path_arg(&path)?);
        } else {
            args.push("--tmpfs".into());
            args.push(path_arg(&path)?);
            args.push("--remount-ro".into());
            args.push(path_arg(&path)?);
        }
    }

    args.push("--chdir".into());
    args.push(path_arg(cwd)?);

    Ok((args, fds))
}

/// Exact path scope represented by the compiled mount policy. The serialized
/// execution result is bounded so a large policy cannot inflate audit output.
pub fn enforcement_scope(spec: &SandboxLaunchSpec) -> Result<Vec<String>, String> {
    let writes = spec
        .write_paths
        .iter()
        .filter(|path| !path.starts_with("/dev/"))
        .cloned()
        .collect::<BTreeSet<_>>();
    let reads = spec
        .read_paths
        .iter()
        .filter(|path| !is_host_runtime_root(path) && !writes.contains(*path))
        .cloned()
        .collect::<BTreeSet<_>>();
    let mut protected = spec
        .protected_readonly_paths
        .iter()
        .cloned()
        .collect::<BTreeSet<_>>();
    for write in &writes {
        if !spec
            .path_identities
            .iter()
            .any(|identity| identity.path == *write)
        {
            protected.insert(write.join(".git"));
        }
    }
    let mut denied = BTreeSet::new();
    for deny in &spec.deny_paths {
        if !denial_may_overlap_grant(deny, &reads, &writes) {
            continue;
        }
        let deny = canonical_or_future(deny)?;
        if reads
            .iter()
            .chain(&writes)
            .any(|root| root.starts_with(deny.as_path()) || deny.starts_with(root.as_path()))
        {
            denied.insert(deny);
        }
    }
    let explicit_etc = [
        "/etc/resolv.conf",
        "/etc/hosts",
        "/etc/nsswitch.conf",
        "/etc/gai.conf",
        "/etc/localtime",
        "/etc/ld.so.cache",
        "/etc/ssl/certs/ca-certificates.crt",
        "/etc/pki/tls/certs/ca-bundle.crt",
    ]
    .iter()
    .filter(|path| std::fs::metadata(path).is_ok_and(|metadata| metadata.is_file()))
    .map(PathBuf::from)
    .collect::<BTreeSet<_>>();
    let runtime_aliases = ["/bin", "/sbin", "/lib", "/lib64"]
        .iter()
        .filter_map(|path| {
            let alias = Path::new(path);
            let is_symlink = std::fs::symlink_metadata(alias)
                .is_ok_and(|metadata| metadata.file_type().is_symlink());
            let source = alias.canonicalize().ok()?;
            (is_symlink && reads.iter().any(|root| source.starts_with(root)))
                .then(|| alias.to_path_buf())
        })
        .collect::<BTreeSet<_>>();

    let scope = vec![
        "fresh mount root; no host-root bind; host /proc, /run, and /sys omitted".into(),
        format!("read-only grant roots: {}", join_paths(&reads)),
        format!(
            "runtime compatibility aliases: {}",
            join_paths(&runtime_aliases)
        ),
        format!("writable grant roots: {}", join_paths(&writes)),
        format!("read-only Git metadata roots: {}", join_paths(&protected)),
        format!("hidden denied descendants: {}", join_paths(&denied)),
        format!(
            "minimal device view: /dev; explicit system files: {}",
            join_paths(&explicit_etc)
        ),
        "unlisted host paths are absent from the fresh mount root".into(),
    ];
    let total = scope.iter().map(String::len).sum::<usize>();
    if total > MAX_REPORTED_SCOPE_BYTES {
        return Err("protected mount scope exceeds its reporting bound".into());
    }
    Ok(scope)
}

fn denial_may_overlap_grant(
    deny: &Path,
    read_roots: &BTreeSet<PathBuf>,
    write_roots: &BTreeSet<PathBuf>,
) -> bool {
    let overlaps = |candidate: &Path| {
        read_roots
            .iter()
            .chain(write_roots)
            .any(|root| candidate.starts_with(root) || root.starts_with(candidate))
    };

    if overlaps(deny) {
        return true;
    }

    // A trusted deny may use an existing symlink alias for a granted path.
    // Resolve that case when possible, while leaving unrelated inaccessible
    // host paths unprobed because they are absent from the fresh mount root.
    std::fs::canonicalize(deny).is_ok_and(|canonical| overlaps(&canonical))
}

fn join_paths(paths: &BTreeSet<PathBuf>) -> String {
    paths
        .iter()
        .map(|path| path.to_string_lossy())
        .collect::<Vec<_>>()
        .join(", ")
}

fn canonical_existing(path: &Path) -> Result<PathBuf, String> {
    let canonical = std::fs::canonicalize(path)
        .map_err(|error| format!("required mount path cannot be resolved: {error}"))?;
    validate_path(&canonical)?;
    Ok(canonical)
}

fn canonical_or_future(path: &Path) -> Result<PathBuf, String> {
    match std::fs::symlink_metadata(path) {
        Ok(metadata) => {
            if metadata.file_type().is_symlink() {
                return canonical_existing(path).map_err(|_| {
                    "protected path is a dangling symlink and cannot be safely mounted".into()
                });
            }
            return canonical_existing(path);
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(format!("cannot inspect protected path: {error}")),
    }
    let mut suffix = Vec::new();
    let mut current = path;
    while !current.exists() {
        let component = current
            .file_name()
            .ok_or_else(|| "protected path has no resolvable parent".to_string())?;
        suffix.push(component.to_os_string());
        current = current
            .parent()
            .ok_or_else(|| "protected path has no resolvable parent".to_string())?;
    }
    let mut result = canonical_existing(current)?;
    for component in suffix.into_iter().rev() {
        result.push(component);
    }
    validate_path(&result)?;
    Ok(result)
}

fn validate_path(path: &Path) -> Result<(), String> {
    if !path.is_absolute()
        || path.as_os_str().len() > MAX_PATH_BYTES
        || path
            .components()
            .any(|component| matches!(component, Component::ParentDir | Component::CurDir))
    {
        return Err("mount path is not a bounded canonical absolute path".into());
    }
    Ok(())
}

fn path_arg(path: &Path) -> Result<String, String> {
    validate_path(path)?;
    path.to_str()
        .map(str::to_owned)
        .ok_or_else(|| "mount path is not valid UTF-8".into())
}

fn is_host_runtime_root(path: &Path) -> bool {
    matches!(
        path.to_str(),
        Some("/dev" | "/proc" | "/sys" | "/run" | "/var/run" | "/etc" | "/opt")
    )
}

fn validate_no_nested_mounts<'a>(roots: impl Iterator<Item = &'a PathBuf>) -> Result<(), String> {
    let mountinfo = std::fs::read_to_string("/proc/self/mountinfo")
        .map_err(|error| format!("cannot inspect mount aliases: {error}"))?;
    let roots = roots.cloned().collect::<Vec<_>>();
    for line in mountinfo.lines() {
        let Some(mountpoint) = line.split_whitespace().nth(4) else {
            return Err("malformed mount table entry".into());
        };
        let mountpoint = mountpoint
            .replace("\\040", " ")
            .replace("\\011", "\t")
            .replace("\\012", "\n")
            .replace("\\134", "\\");
        let mountpoint = PathBuf::from(mountpoint);
        if roots
            .iter()
            .any(|root| mountpoint != *root && mountpoint.starts_with(root))
        {
            return Err("nested mount beneath a granted root is not supported".into());
        }
    }
    Ok(())
}

fn validate_protected_tree(root: &Path) -> Result<(), String> {
    use std::os::unix::fs::MetadataExt;

    let root_metadata = std::fs::symlink_metadata(root)
        .map_err(|error| format!("cannot inspect protected Git metadata root: {error}"))?;
    if root_metadata.file_type().is_symlink() {
        return Err("symlinked protected Git metadata root is unsupported".into());
    }
    if root_metadata.is_file() {
        if root_metadata.nlink() > 1 {
            return Err("hardlinked protected Git metadata cannot be mounted safely".into());
        }
        return Ok(());
    }
    if !root_metadata.is_dir() {
        return Err("unsupported protected Git metadata root type".into());
    }

    let mut pending = vec![root.to_path_buf()];
    let mut visited = 0usize;
    while let Some(directory) = pending.pop() {
        for entry in std::fs::read_dir(&directory).map_err(|error| {
            format!(
                "cannot inspect protected Git metadata at {}: {error}",
                directory.display()
            )
        })? {
            let entry =
                entry.map_err(|error| format!("cannot inspect Git metadata entry: {error}"))?;
            visited += 1;
            if visited > 100_000 {
                return Err("protected Git metadata scan exceeded its entry bound".into());
            }
            let metadata = std::fs::symlink_metadata(entry.path())
                .map_err(|error| format!("cannot inspect Git metadata identity: {error}"))?;
            if metadata.is_file() && metadata.nlink() > 1 {
                return Err("hardlinked protected Git metadata cannot be mounted safely".into());
            }
            if metadata.is_dir() {
                pending.push(entry.path());
            }
        }
    }
    Ok(())
}

fn add_parent_directories(path: &Path, directories: &mut BTreeSet<PathBuf>) -> Result<(), String> {
    validate_path(path)?;
    let mut parents = path
        .parent()
        .into_iter()
        .flat_map(Path::ancestors)
        .filter(|parent| *parent != Path::new("/"))
        .map(Path::to_path_buf)
        .collect::<Vec<_>>();
    parents.reverse();
    directories.extend(parents);
    Ok(())
}

fn discover_git_metadata(workspace: &Path) -> Result<Vec<PathBuf>, String> {
    let mut protected = Vec::new();
    let mut pending = vec![workspace.to_path_buf()];
    let mut visited = 0usize;
    while let Some(directory) = pending.pop() {
        visited += 1;
        if visited > 100_000 {
            return Err("workspace Git metadata scan exceeded its entry bound".into());
        }
        let git_entry = directory.join(".git");
        let git_metadata = match std::fs::symlink_metadata(&git_entry) {
            Ok(metadata) => Some(metadata),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(error) => return Err(format!("cannot inspect Git metadata path: {error}")),
        };
        let has_git_entry = git_metadata.is_some();
        if let Some(metadata) = git_metadata {
            if metadata.file_type().is_symlink() {
                return Err("symlinked .git metadata path is not supported".into());
            }
            protected.push(git_entry.clone());
            if metadata.is_dir() {
                protected.push(git_entry.clone());
            } else if !metadata.is_file() {
                return Err("unsupported .git metadata path type".into());
            } else {
                let pointer = std::fs::read_to_string(&git_entry)
                    .map_err(|error| format!("cannot read linked-worktree pointer: {error}"))?;
                if pointer.len() > 4096 {
                    return Err("linked-worktree pointer exceeds its size bound".into());
                }
                let gitdir = pointer
                    .lines()
                    .find_map(|line| line.strip_prefix("gitdir: "))
                    .ok_or_else(|| "malformed linked-worktree .git pointer".to_string())?;
                let gitdir = PathBuf::from(gitdir);
                let gitdir = if gitdir.is_absolute() {
                    gitdir
                } else {
                    directory.join(gitdir)
                };
                let gitdir = canonical_existing(&gitdir)?;
                protected.push(gitdir.clone());
                let common_file = gitdir.join("commondir");
                if common_file.exists() {
                    let common = std::fs::read_to_string(&common_file)
                        .map_err(|error| format!("cannot read Git common-dir pointer: {error}"))?;
                    if common.len() > 4096 {
                        return Err("Git common-dir pointer exceeds its size bound".into());
                    }
                    let value = common.trim();
                    if value.is_empty() {
                        return Err("Git common-dir pointer is empty".into());
                    }
                    let common = PathBuf::from(value);
                    let common = if common.is_absolute() {
                        common
                    } else {
                        gitdir.join(common)
                    };
                    protected.push(canonical_existing(&common)?);
                }
            }
        }

        let entries = std::fs::read_dir(&directory)
            .map_err(|error| format!("cannot inspect workspace Git metadata: {error}"))?;
        for entry in entries {
            let entry =
                entry.map_err(|error| format!("cannot inspect workspace entry: {error}"))?;
            visited += 1;
            if visited > 100_000 {
                return Err("workspace Git metadata scan exceeded its entry bound".into());
            }
            if entry.file_name() == ".git" {
                continue;
            }
            let file_type = entry
                .file_type()
                .map_err(|error| format!("cannot inspect workspace entry type: {error}"))?;
            if file_type.is_dir() {
                pending.push(entry.path());
            }
        }
        // Only the leased workspace's root .git path is protected in the
        // absent case. Existing nested .git pointers (submodules) are found
        // by the bounded walk above.
        if directory == workspace && !has_git_entry {
            protected.push(git_entry);
        }
    }
    protected.sort();
    protected.dedup();
    Ok(protected)
}

pub fn git_metadata_paths(write_roots: &[PathBuf]) -> Result<Vec<PathBuf>, String> {
    let mut paths = BTreeSet::new();
    for root in write_roots {
        if root.starts_with("/dev/") {
            continue;
        }
        let metadata = std::fs::metadata(root)
            .map_err(|error| format!("cannot inspect workspace lease root: {error}"))?;
        if metadata.is_dir() {
            paths.extend(discover_git_metadata(root)?);
        }
    }
    Ok(paths.into_iter().collect())
}

fn verify_path_identities(spec: &SandboxLaunchSpec) -> Result<(), String> {
    use std::os::unix::fs::MetadataExt;

    for identity in &spec.path_identities {
        let canonical = canonical_existing(&identity.path)?;
        if canonical != identity.path {
            return Err("sandbox grant path changed its canonical identity".into());
        }
        let metadata = std::fs::metadata(&canonical)
            .map_err(|error| format!("cannot verify sandbox grant identity: {error}"))?;
        if metadata.dev() != identity.device || metadata.ino() != identity.inode {
            return Err("sandbox grant path was replaced after authorization".into());
        }
    }

    let captured = spec
        .path_identities
        .iter()
        .map(|identity| identity.path.as_path())
        .collect::<BTreeSet<_>>();
    for root in spec.read_paths.iter().chain(&spec.write_paths) {
        if root.starts_with("/dev/") || is_launch_scratch(root) {
            continue;
        }
        let canonical = canonical_existing(root)?;
        if !captured.contains(canonical.as_path()) {
            return Err("sandbox grant has no captured path identity".into());
        }
    }
    Ok(())
}

fn is_launch_scratch(path: &Path) -> bool {
    path.parent() == Some(Path::new("/tmp"))
        && path
            .file_name()
            .is_some_and(|name| name.to_string_lossy().starts_with("codegg-run-"))
}
