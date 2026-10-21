use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Instant;

const FILE_COUNT: usize = 4096;
const FILE_BYTES: usize = 24 * 1024;
const WARM_SAMPLES: usize = 100;
const COLD_SAMPLES: usize = 30;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = std::env::args().collect::<Vec<_>>();
    match args.get(1).map(String::as_str) {
        Some("once") => {
            let route = &args[2];
            let repository = Path::new(&args[3]);
            let before = Usage::read()?;
            let count = run(route, repository)?;
            let usage = Usage::read()?.delta(before);
            println!(
                "{count},{},{},{},{}",
                usage.self_cpu_us, usage.child_cpu_us, usage.self_maxrss_kb, usage.child_maxrss_kb
            );
            Ok(())
        }
        Some("bench") => {
            let output_dir = PathBuf::from(&args[2]);
            fs::create_dir_all(&output_dir)?;
            let root = tempfile::tempdir()?;
            for shape in ["fragmented", "packed"] {
                let repository = root.path().join(shape);
                make_fixture(&repository, shape == "packed")?;
                fs::write(
                    repository.join("object-0000.bin"),
                    b"dirty tracked payload\n",
                )?;
                fs::write(repository.join("untracked.txt"), b"untracked payload\n")?;
                let object_bytes = directory_bytes(&repository.join(".git/objects"))?;
                let object_files = file_count(&repository.join(".git/objects"))?;
                let parity = [
                    run("native", &repository)?,
                    run("snapshot", &repository)?,
                    run("gix", &repository)?,
                ];
                if parity != [2, 2, 2] {
                    return Err(format!("status parity failed for {shape}: {parity:?}").into());
                }
                for route in ["native", "snapshot", "gix"] {
                    let csv = output_dir.join(format!("{shape}-{route}.csv"));
                    let mut output = fs::File::create(csv)?;
                    writeln!(output, "shape,route,temperature,sample,elapsed_ns,status_entries,self_cpu_us,child_cpu_us,self_maxrss_kb,child_maxrss_kb,objects_bytes,object_files,cache_drop_advised_bytes")?;
                    // The first call establishes only the warm process cache for the snapshot route.
                    let _ = run(route, &repository)?;
                    for sample in 0..WARM_SAMPLES {
                        let before = Usage::read()?;
                        let started = Instant::now();
                        let count = run(route, &repository)?;
                        let usage = Usage::read()?.delta(before);
                        writeln!(
                            output,
                            "{shape},{route},warm,{sample},{},{count},{},{},{},{},{object_bytes},{object_files}",
                            started.elapsed().as_nanos(),
                            usage.self_cpu_us,
                            usage.child_cpu_us,
                            usage.self_maxrss_kb,
                            usage.child_maxrss_kb,
                        )?;
                        if count > FILE_COUNT {
                            return Err(format!("unexpected status count {count}").into());
                        }
                    }
                    for sample in 0..COLD_SAMPLES {
                        // Advisory only: ask the kernel to discard object pages before
                        // each fresh-process sample. POSIX does not guarantee eviction.
                        let cache_drop_advised_bytes =
                            advise_drop_object_cache(&repository.join(".git/objects"))?;
                        let started = Instant::now();
                        let result = Command::new(std::env::current_exe()?)
                            .args([
                                "once",
                                route,
                                repository.to_str().ok_or("non-UTF8 fixture path")?,
                            ])
                            .stdin(Stdio::null())
                            .output()?;
                        let elapsed = started.elapsed().as_nanos();
                        if !result.status.success() {
                            return Err(format!(
                                "cold {route} sample failed: {}",
                                String::from_utf8_lossy(&result.stderr)
                            )
                            .into());
                        }
                        let encoded = String::from_utf8(result.stdout)?;
                        let mut metrics = encoded.trim().split(',');
                        let count = metrics
                            .next()
                            .ok_or("missing cold status count")?
                            .parse::<usize>()?;
                        let usage = UsageDelta::parse(metrics)?;
                        if count != 2 {
                            return Err(
                                format!("cold {route} parity failed for {shape}: {count}").into()
                            );
                        }
                        writeln!(
                            output,
                            "{shape},{route},cold,{sample},{elapsed},{count},{},{},{},{},{object_bytes},{object_files},{cache_drop_advised_bytes}",
                            usage.self_cpu_us,
                            usage.child_cpu_us,
                            usage.self_maxrss_kb,
                            usage.child_maxrss_kb,
                        )?;
                    }
                    output.flush()?;
                }
            }
            println!("benchmark samples written to {}", output_dir.display());
            Ok(())
        }
        _ => Err(
            "usage: linux-trusted-git-cost-bench bench <output-dir> | once <route> <repo>".into(),
        ),
    }
}

#[derive(Debug)]
struct Usage {
    self_user_us: i64,
    self_system_us: i64,
    self_maxrss_kb: i64,
    child_user_us: i64,
    child_system_us: i64,
    child_maxrss_kb: i64,
}

#[derive(Debug)]
struct UsageDelta {
    self_cpu_us: i64,
    child_cpu_us: i64,
    self_maxrss_kb: i64,
    child_maxrss_kb: i64,
}

impl Usage {
    fn read() -> Result<Self, Box<dyn std::error::Error>> {
        fn get(which: libc::c_int) -> Result<libc::rusage, Box<dyn std::error::Error>> {
            let mut usage = std::mem::MaybeUninit::<libc::rusage>::uninit();
            // SAFETY: getrusage initializes the supplied value on success.
            if unsafe { libc::getrusage(which, usage.as_mut_ptr()) } != 0 {
                return Err(std::io::Error::last_os_error().into());
            }
            // SAFETY: the preceding successful call initialized the value.
            Ok(unsafe { usage.assume_init() })
        }
        fn micros(time: libc::timeval) -> i64 {
            time.tv_sec as i64 * 1_000_000 + time.tv_usec as i64
        }
        let own = get(libc::RUSAGE_SELF)?;
        let children = get(libc::RUSAGE_CHILDREN)?;
        Ok(Self {
            self_user_us: micros(own.ru_utime),
            self_system_us: micros(own.ru_stime),
            self_maxrss_kb: own.ru_maxrss as i64,
            child_user_us: micros(children.ru_utime),
            child_system_us: micros(children.ru_stime),
            child_maxrss_kb: children.ru_maxrss as i64,
        })
    }

    fn delta(self, before: Self) -> UsageDelta {
        UsageDelta {
            self_cpu_us: self.self_user_us + self.self_system_us
                - before.self_user_us
                - before.self_system_us,
            child_cpu_us: self.child_user_us + self.child_system_us
                - before.child_user_us
                - before.child_system_us,
            self_maxrss_kb: self.self_maxrss_kb,
            child_maxrss_kb: self.child_maxrss_kb,
        }
    }
}

impl UsageDelta {
    fn parse<'a>(
        mut values: impl Iterator<Item = &'a str>,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        Ok(Self {
            self_cpu_us: values.next().ok_or("missing self CPU")?.parse()?,
            child_cpu_us: values.next().ok_or("missing child CPU")?.parse()?,
            self_maxrss_kb: values.next().ok_or("missing self RSS")?.parse()?,
            child_maxrss_kb: values.next().ok_or("missing child RSS")?.parse()?,
        })
    }
}

fn run(route: &str, repository: &Path) -> Result<usize, Box<dyn std::error::Error>> {
    match route {
        "native" => {
            let result = git(
                repository,
                &["status", "--porcelain=v2", "-z", "--untracked-files=all"],
            )?;
            Ok(result
                .stdout
                .split(|byte| *byte == 0)
                .filter(|entry| !entry.is_empty())
                .count())
        }
        "snapshot" => {
            let argv = [
                "git".to_owned(),
                "status".to_owned(),
                "--porcelain=v2".to_owned(),
                "-z".to_owned(),
                "--untracked-files=all".to_owned(),
            ];
            let mut command = egggit::GitEnvPolicy::default()
                .apply_sync(&argv, repository)
                .map_err(|error| format!("trusted snapshot policy refused: {error:?}"))?;
            let output = command.output()?;
            if !output.status.success() {
                return Err(format!(
                    "snapshot Git failed: {}",
                    String::from_utf8_lossy(&output.stderr)
                )
                .into());
            }
            Ok(output
                .stdout
                .split(|byte| *byte == 0)
                .filter(|entry| !entry.is_empty())
                .count())
        }
        "gix" => {
            #[cfg(feature = "with-gix")]
            {
                // Isolated disables system/user/environment config and includes. The
                // only remaining config source is local repository configuration.
                let repository = gix::open_opts(repository, gix::open::Options::isolated())?;
                let changes = repository
                    .status(gix::progress::Discard)?
                    .into_index_worktree_iter(Vec::new())?
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(changes.len())
            }
            #[cfg(not(feature = "with-gix"))]
            {
                Err("rebuild with --features with-gix to compare the gix route".into())
            }
        }
        other => Err(format!("unknown route {other}").into()),
    }
}

fn git(
    repository: &Path,
    args: &[&str],
) -> Result<std::process::Output, Box<dyn std::error::Error>> {
    let git = executable("git")?;
    let output = Command::new(git)
        .args(args)
        .current_dir(repository)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_OPTIONAL_LOCKS", "0")
        .output()?;
    if !output.status.success() {
        return Err(format!(
            "git {:?} failed: {}",
            args,
            String::from_utf8_lossy(&output.stderr)
        )
        .into());
    }
    Ok(output)
}

fn make_fixture(repository: &Path, packed: bool) -> Result<(), Box<dyn std::error::Error>> {
    fs::create_dir_all(repository)?;
    git(repository, &["init", "--quiet"])?;
    git(repository, &["config", "gc.auto", "0"])?;
    let mut state = 0x8f31_2c49_d7a6_b5e1u64;
    for index in 0..FILE_COUNT {
        let path = repository.join(format!("object-{index:04}.bin"));
        let mut bytes = Vec::with_capacity(FILE_BYTES);
        for _ in 0..FILE_BYTES {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            bytes.push(state as u8);
        }
        fs::write(path, bytes)?;
    }
    git(repository, &["add", "--all"])?;
    let commit = Command::new(executable("git")?)
        .args([
            "-c",
            "user.name=Cost benchmark",
            "-c",
            "user.email=cost@example.invalid",
            "commit",
            "--quiet",
            "-m",
            "fixture",
        ])
        .current_dir(repository)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .output()?;
    if !commit.status.success() {
        return Err(format!(
            "fixture commit failed: {}",
            String::from_utf8_lossy(&commit.stderr)
        )
        .into());
    }
    if packed {
        git(repository, &["repack", "-ad"])?;
        git(repository, &["prune-packed"])?;
    }
    Ok(())
}

fn executable(name: &str) -> Result<PathBuf, Box<dyn std::error::Error>> {
    for directory in std::env::split_paths(&std::env::var_os("PATH").ok_or("PATH is unset")?) {
        let candidate = directory.join(name);
        if candidate.is_file() {
            return Ok(candidate.canonicalize()?);
        }
    }
    Err(format!("{name} is not installed on PATH").into())
}

fn directory_bytes(path: &Path) -> Result<u64, Box<dyn std::error::Error>> {
    let mut bytes = 0;
    for entry in walkdir(path)? {
        bytes += entry.metadata()?.len();
    }
    Ok(bytes)
}

fn file_count(path: &Path) -> Result<usize, Box<dyn std::error::Error>> {
    Ok(walkdir(path)?.len())
}

fn advise_drop_object_cache(path: &Path) -> Result<u64, Box<dyn std::error::Error>> {
    let mut advised_bytes = 0;
    for entry in walkdir(path)? {
        let file = fs::File::open(entry.path())?;
        let length = entry.metadata()?.len();
        // SAFETY: posix_fadvise only receives a valid open descriptor and byte range.
        // POSIX_FADV_DONTNEED is advisory; the benchmark records the request, not proof
        // that the kernel evicted these pages.
        let result = unsafe {
            libc::posix_fadvise(
                std::os::fd::AsRawFd::as_raw_fd(&file),
                0,
                0,
                libc::POSIX_FADV_DONTNEED,
            )
        };
        if result != 0 {
            return Err(std::io::Error::from_raw_os_error(result).into());
        }
        advised_bytes += length;
    }
    Ok(advised_bytes)
}

fn walkdir(path: &Path) -> Result<Vec<fs::DirEntry>, Box<dyn std::error::Error>> {
    let mut pending = vec![path.to_path_buf()];
    let mut files = Vec::new();
    while let Some(directory) = pending.pop() {
        for entry in fs::read_dir(directory)? {
            let entry = entry?;
            if entry.file_type()?.is_dir() {
                pending.push(entry.path());
            } else {
                files.push(entry);
            }
        }
    }
    Ok(files)
}
