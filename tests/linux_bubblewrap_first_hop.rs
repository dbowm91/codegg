#![cfg(target_os = "linux")]

//! Opt-in native first-hop feasibility fixture. This does not register or call
//! a production sandbox backend and does not claim that its status pipe is the
//! production helper protocol.

use std::fs;
use std::os::fd::AsRawFd;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::thread;
use std::time::{Duration, Instant};

use codegg::security::sandbox::linux_launch::{
    Demand, LinuxLaunchAdapter, LinuxLaunchPlan, LinuxLaunchRequest, NetworkIntent, ProcessScope,
    ProductionLinuxLaunchAdapter,
};
use codegg::security::sandbox::SandboxLaunchSpec;

const OPT_IN: &str = "CODEGG_RUN_LINUX_BWRAP_FIRST_HOP";

fn trusted_bubblewrap() -> Result<PathBuf, String> {
    for candidate in ["/usr/bin/bwrap", "/bin/bwrap", "/usr/local/bin/bwrap"] {
        let path = Path::new(candidate);
        let Ok(canonical) = fs::canonicalize(path) else {
            continue;
        };
        let metadata = fs::metadata(&canonical).map_err(|error| error.to_string())?;
        let secure_ancestors = canonical.ancestors().skip(1).all(|ancestor| {
            fs::metadata(ancestor)
                .is_ok_and(|metadata| metadata.uid() == 0 && metadata.mode() & 0o022 == 0)
        });
        if metadata.is_file()
            && metadata.uid() == 0
            && metadata.mode() & 0o022 == 0
            && metadata.mode() & 0o111 != 0
            && metadata.mode() & 0o4000 == 0
            && secure_ancestors
        {
            return Ok(canonical);
        }
        return Err(format!(
            "{} is not a trusted root-owned, non-writable helper",
            canonical.display()
        ));
    }
    Err("no trusted Bubblewrap executable at a fixed system path".into())
}

fn launcher(helper: &Path, scratch: &Path, status_fd: i32) -> Command {
    let mut command = Command::new(helper);
    command
        .arg("--unshare-user")
        .arg("--unshare-pid")
        .arg("--unshare-net")
        .arg("--unshare-ipc")
        .arg("--die-with-parent")
        .arg("--new-session")
        .args(["--ro-bind", "/usr/bin", "/usr/bin"])
        .args(["--ro-bind", "/usr/lib", "/usr/lib"])
        .args(["--ro-bind", "/lib", "/lib"])
        .args(["--ro-bind", "/lib64", "/lib64"])
        .args(["--proc", "/proc"])
        .args(["--dev", "/dev"])
        .arg("--bind")
        .arg(scratch)
        .arg("/fixture")
        .args(["--chdir", "/fixture", "--"])
        .arg("/usr/bin/setpriv")
        .args(["--no-new-privs", "--"])
        .arg("/usr/bin/dash")
        .arg("-c")
        .arg(format!(
            "for n in user mnt pid net ipc; do /usr/bin/stat -Lc '%i' /proc/self/ns/$n; done > /fixture/namespaces; \\
             if test -e /proc/self/fd/{status_fd}; then printf present > /fixture/status-fd; else printf absent > /fixture/status-fd; fi; \\
             /usr/bin/grep '^NoNewPrivs:' /proc/self/status > /fixture/no-new-privs; \\
             printf ran > /fixture/target.marker"
        ));
    command
}

fn namespace_ids(path: &Path) -> Result<Vec<u64>, String> {
    fs::read_to_string(path)
        .map_err(|error| error.to_string())?
        .lines()
        .map(|line| line.parse::<u64>().map_err(|error| error.to_string()))
        .collect()
}

#[test]
fn opt_in_real_child_namespaces_and_fail_closed_setup() {
    if std::env::var(OPT_IN).as_deref() != Ok("1") {
        eprintln!("skipped: set {OPT_IN}=1 to run the native Linux namespace fixture");
        return;
    }

    let helper = trusted_bubblewrap().expect("trusted fixed-path Bubblewrap helper");
    let scratch = tempfile::tempdir().expect("private fixture scratch");
    let marker = scratch.path().join("target.marker");
    let spec = SandboxLaunchSpec {
        linux_protected_mount_view: false,
        protected_readonly_paths: Vec::new(),
        path_identities: Vec::new(),
        target: PathBuf::from("/usr/bin/dash"),
        args: Vec::new(),
        read_paths: vec![PathBuf::from("/usr/bin"), PathBuf::from("/usr/lib")],
        write_paths: vec![scratch.path().to_owned()],
        deny_paths: vec![scratch.path().join("protected")],
    };
    let mut request = LinuxLaunchRequest::legacy_filesystem(&spec);
    request.network.demand = Demand::Required;
    request.network.requested = NetworkIntent::OfflineRequired;
    request.network.parent_ceiling = NetworkIntent::OfflineRequired;
    request.process.demand = Demand::Required;
    request.process.requested = ProcessScope::Namespaced;
    request.process.parent_ceiling = ProcessScope::Namespaced;
    request.validate_parent_ceiling().unwrap();
    assert_eq!(
        ProductionLinuxLaunchAdapter.plan(&request).unwrap(),
        LinuxLaunchPlan::EnhancedUnavailable {
            dimensions: vec!["network", "process"]
        },
        "production must not select the test-only Bubblewrap fixture"
    );

    // The private pipe belongs to the test parent and is deliberately not
    // passed through std::process::Command. The child checks that the chosen
    // descriptor is absent; the parent then writes the only status record.
    let (status_reader, mut status_writer) = std::io::pipe().expect("private status pipe");
    let status_writer_fd = status_writer.as_raw_fd();
    let helper_version = Command::new(&helper)
        .arg("--version")
        .output()
        .expect("read Bubblewrap version");
    assert!(helper_version.status.success());
    eprintln!(
        "helper={} uid={} mode={:o} version={}",
        helper.display(),
        fs::metadata(&helper).unwrap().uid(),
        fs::metadata(&helper).unwrap().mode() & 0o7777,
        String::from_utf8_lossy(&helper_version.stdout).trim()
    );

    let mut child = launcher(&helper, scratch.path(), status_writer_fd)
        .spawn()
        .expect("start trusted Bubblewrap helper");
    let started = Instant::now();
    let result = loop {
        if let Some(status) = child.try_wait().expect("poll native child") {
            break status;
        }
        if started.elapsed() >= Duration::from_secs(10) {
            let _ = child.kill();
            let _ = child.wait();
            panic!("native namespace child exceeded the 10 second bound");
        }
        thread::sleep(Duration::from_millis(20));
    };
    if !result.success() {
        assert!(!marker.exists(), "target ran after namespace setup failed");
        eprintln!("native namespace setup unavailable: {result}");
        return;
    }

    assert_eq!(fs::read_to_string(&marker).unwrap(), "ran");
    assert_eq!(
        fs::read_to_string(scratch.path().join("status-fd")).unwrap(),
        "absent"
    );
    assert!(fs::read_to_string(scratch.path().join("no-new-privs"))
        .unwrap()
        .trim_end()
        .ends_with("1"));
    let actual = namespace_ids(&scratch.path().join("namespaces")).unwrap();
    let names = ["user", "mnt", "pid", "net", "ipc"];
    assert_eq!(actual.len(), names.len());
    let mut host_ids = Vec::with_capacity(names.len());
    for (name, child_id) in names.iter().zip(actual.iter().copied()) {
        let host_id = fs::metadata(format!("/proc/self/ns/{name}")).unwrap().ino();
        assert_ne!(
            child_id, host_id,
            "child did not enter a distinct {name} namespace"
        );
        host_ids.push(host_id);
    }
    eprintln!("namespace names={names:?} host_ids={host_ids:?} child_ids={actual:?}");

    use std::io::{Read, Write};
    status_writer
        .write_all(b"parent-observed=setup-and-exec\n")
        .unwrap();
    drop(status_writer);
    let mut receipt = String::new();
    let mut reader = status_reader;
    reader.read_to_string(&mut receipt).unwrap();
    assert_eq!(receipt, "parent-observed=setup-and-exec\n");

    let failed_marker = scratch.path().join("failed-target.marker");
    let failed = Command::new(&helper)
        .args([
            "--unshare-user",
            "--unshare-pid",
            "--die-with-parent",
            "--ro-bind",
            "/path/that/does/not/exist",
            "/fixture",
            "--",
            "/usr/bin/dash",
            "-c",
            "printf ran > /fixture/failed-target.marker",
        ])
        .output()
        .expect("run negative setup fixture");
    assert!(
        !failed.status.success(),
        "invalid required mount unexpectedly succeeded"
    );
    assert!(
        !failed_marker.exists(),
        "target ran after required mount setup failed"
    );
}
