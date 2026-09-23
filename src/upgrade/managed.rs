use std::collections::HashSet;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

use super::INSTALLER_SCRIPT_URL;
use eggup_acquisition::{
    AcquisitionError, AcquisitionRequest, AcquisitionTransport, ArtifactEvidence, CancelFlag,
    FetchLimits, FetchOutcome, MetadataBytes,
};
use eggup_core::{
    run_bounded, AbsentPolicy, ArtifactMember, ArtifactSet, CandidateValidator, CommandSpec,
    CommitOwnership, InstallPlan, IntegrityRequirement, MemberId, Ownership, OwnershipVerifier,
    PermissionsIntent, ProductId, ReleaseId, TransactionDisposition, VerifiedTransaction,
};
use futures_util::StreamExt;
use sha2::{Digest, Sha256};

const MAX_ARCHIVE_BYTES: u64 = 256 * 1024 * 1024;
const MAX_RUNFILE_BYTES: u64 = 128 * 1024 * 1024;
const MAX_NOTICE_BYTES: u64 = 1024 * 1024;
const CHECKSUM_LIMIT: usize = 64 * 1024;
const PINNED_EGGSEARCH_VERSION: &str = "0.3.9";
const SUPPORTED_TARGETS: [&str; 4] = [
    "x86_64-unknown-linux-gnu",
    "aarch64-unknown-linux-gnu",
    "x86_64-apple-darwin",
    "aarch64-apple-darwin",
];

pub(super) fn supported_target() -> Option<&'static str> {
    let target = match (std::env::consts::OS, std::env::consts::ARCH) {
        ("linux", "x86_64") => Some("x86_64-unknown-linux-gnu"),
        ("linux", "aarch64") => Some("aarch64-unknown-linux-gnu"),
        ("macos", "x86_64") => Some("x86_64-apple-darwin"),
        ("macos", "aarch64") => Some("aarch64-apple-darwin"),
        _ => None,
    }?;
    SUPPORTED_TARGETS.contains(&target).then_some(target)
}

fn archive_asset_name(target: &str) -> Result<String, String> {
    if !SUPPORTED_TARGETS.contains(&target) {
        return Err("unsupported CodeGG archive target".into());
    }
    Ok(format!("codegg-{target}.tar.gz"))
}

pub(super) fn update(latest: &str) -> Result<String, String> {
    let target = supported_target().ok_or_else(|| {
        format!(
            "in-place upgrade is supported only for packaged Linux and macOS targets. For a manual fresh installation only, run: CODEGG_VERSION=v{latest} curl -fsSL {INSTALLER_SCRIPT_URL} | sh"
        )
    })?;
    let version =
        semver::Version::parse(latest).map_err(|_| "release version is invalid".to_string())?;
    let tag = format!("v{version}");
    let archive_name = archive_asset_name(target)?;
    let base = format!("https://github.com/dbowm91/codegg/releases/download/{tag}");
    let transport =
        CodeggTransport::new().map_err(|_| "could not initialize upgrade transport".to_string())?;
    let cancel = CancelFlag::new();
    let work = tempfile::tempdir()
        .map_err(|_| "could not create private upgrade workspace".to_string())?;
    let checksums = fetch_metadata(
        &transport,
        &format!("{base}/checksums.txt"),
        CHECKSUM_LIMIT,
        &cancel,
    )?;
    let expected_digest = checksum_for_archive(&checksums, &archive_name)?;
    let archive_path = work.path().join(&archive_name);
    fetch_artifact(
        &transport,
        &format!("{base}/{archive_name}"),
        &archive_path,
        MAX_ARCHIVE_BYTES,
        &cancel,
    )?;
    if sha256_file(&archive_path)? != expected_digest {
        return Err("release archive SHA-256 mismatch; installation was not changed".into());
    }
    let extracted = work.path().join("extracted");
    fs::create_dir(&extracted)
        .map_err(|_| "could not create private extraction directory".to_string())?;
    extract_bundle(&archive_path, &extracted)?;

    let exe =
        std::env::current_exe().map_err(|_| "could not resolve running executable".to_string())?;
    let install_root = exe
        .parent()
        .ok_or("running executable has no parent directory")?;
    let install_root =
        fs::canonicalize(install_root).map_err(|_| "could not resolve installation directory")?;
    let current_exe = fs::canonicalize(&exe).map_err(|_| "could not resolve running executable")?;
    let members = build_members(&extracted, &install_root)?;
    let release = ReleaseId::new(version.to_string()).map_err(|_| "invalid release identifier")?;
    let plan = InstallPlan::new(
        ProductId::new("codegg").map_err(|_| "invalid product identifier")?,
        release,
        &install_root,
        ArtifactSet::new(members).map_err(|e| format!("invalid runfile set: {e}"))?,
    )
    .map_err(|e| format!("invalid install plan: {e}"))?;
    let prepared = plan
        .prepare()
        .map_err(|e| format!("could not stage update: {e}"))?;
    let verified = prepared
        .verify_integrity()
        .map_err(|e| format!("staged integrity verification failed: {e}"))?;
    let validator = CodeggValidator {
        version: version.to_string(),
    };
    let validated = verified
        .validate(&validator)
        .map_err(|e| format!("candidate validation failed: {e}"))?;
    let owner = CodeggOwnership {
        root: install_root.clone(),
        current_exe,
        current_version: env!("CARGO_PKG_VERSION").to_string(),
    };
    let receipt = validated
        .commit(CommitOwnership::new(&owner, AbsentPolicy::AllowCreate))
        .map_err(|e| format!("transaction could not start: {e}"))?;
    match receipt.disposition() {
        TransactionDisposition::Committed => Ok(format!("Updated CodeGG to {version}")),
        TransactionDisposition::RolledBack => Err(format!(
            "update rolled back; prior runfiles were restored{}",
            receipt
                .failure()
                .map(|f| format!(" ({:?}: {})", f.phase(), f.detail()))
                .unwrap_or_default()
        )),
        TransactionDisposition::RecoveryRequired => Err(format!(
            "update requires manual recovery; retained evidence: {}{}",
            receipt
                .recovery_path()
                .map(|p| p.display().to_string())
                .unwrap_or_else(|| "(path unavailable)".into()),
            receipt
                .failure()
                .map(|f| format!(" ({:?}: {})", f.phase(), f.detail()))
                .unwrap_or_default()
        )),
    }
}

fn fetch_metadata<T: AcquisitionTransport>(
    transport: &T,
    url: &str,
    max: usize,
    cancel: &CancelFlag,
) -> Result<Vec<u8>, String> {
    let request = AcquisitionRequest::new(url).map_err(|_| "invalid release metadata URL")?;
    let limits = FetchLimits::new(max, None, Duration::from_secs(10), Duration::from_secs(30))
        .map_err(|_| "invalid release metadata limits")?;
    match transport.fetch_metadata(&request, limits, cancel) {
        Ok(FetchOutcome::Success(bytes)) => Ok(bytes.bytes().to_vec()),
        Ok(FetchOutcome::NotFound) => Err("release metadata asset was not found".into()),
        Err(error) => Err(format!("release metadata acquisition failed: {error}")),
    }
}

fn fetch_artifact<T: AcquisitionTransport>(
    transport: &T,
    url: &str,
    dest: &Path,
    max: u64,
    cancel: &CancelFlag,
) -> Result<(), String> {
    let request = AcquisitionRequest::new(url).map_err(|_| "invalid release artifact URL")?;
    let limits = FetchLimits::new(
        1024,
        Some(max),
        Duration::from_secs(10),
        Duration::from_secs(180),
    )
    .map_err(|_| "invalid release artifact limits")?;
    match transport.fetch_artifact(&request, dest, limits, cancel) {
        Ok(FetchOutcome::Success(_)) => Ok(()),
        Ok(FetchOutcome::NotFound) => Err("release archive was not found".into()),
        Err(error) => Err(format!("release archive acquisition failed: {error}")),
    }
}

fn checksum_for_archive(manifest: &[u8], expected_name: &str) -> Result<[u8; 32], String> {
    let text = std::str::from_utf8(manifest).map_err(|_| "checksum manifest is not UTF-8")?;
    let mut found = None;
    for line in text.lines() {
        let mut fields = line.split_ascii_whitespace();
        let digest = fields.next().ok_or("malformed checksum entry")?;
        let name = fields.next().ok_or("malformed checksum entry")?;
        if fields.next().is_some()
            || digest.len() != 64
            || !digest.bytes().all(|byte| byte.is_ascii_hexdigit())
            || name.contains('/')
            || name.contains('\\')
            || name.chars().any(char::is_control)
        {
            return Err("malformed checksum entry".into());
        }
        if name == expected_name {
            if found.is_some() {
                return Err("duplicate archive checksum entry".into());
            }
            let mut value = [0; 32];
            for (index, pair) in digest.as_bytes().as_chunks::<2>().0.iter().enumerate() {
                value[index] = (hex_nibble(pair[0])? << 4) | hex_nibble(pair[1])?;
            }
            found = Some(value);
        }
    }
    found.ok_or_else(|| "archive checksum entry is missing".into())
}

fn hex_nibble(value: u8) -> Result<u8, String> {
    match value {
        b'0'..=b'9' => Ok(value - b'0'),
        b'a'..=b'f' => Ok(value - b'a' + 10),
        b'A'..=b'F' => Ok(value - b'A' + 10),
        _ => Err("malformed checksum digest".into()),
    }
}

fn sha256_file(path: &Path) -> Result<[u8; 32], String> {
    let mut file = File::open(path).map_err(|_| "could not read downloaded archive")?;
    let mut digest = Sha256::new();
    let mut buffer = [0; 32 * 1024];
    loop {
        let count = file
            .read(&mut buffer)
            .map_err(|_| "could not hash downloaded archive")?;
        if count == 0 {
            break;
        }
        digest.update(&buffer[..count]);
    }
    Ok(digest.finalize().into())
}

fn extract_bundle(archive_path: &Path, destination: &Path) -> Result<(), String> {
    let gzip = flate2::read::GzDecoder::new(
        File::open(archive_path).map_err(|_| "could not open archive")?,
    );
    let mut archive = tar::Archive::new(gzip);
    let mut seen = HashSet::new();
    let mut folded = HashSet::new();
    let mut total = 0_u64;
    let entries = archive.entries().map_err(|_| "invalid tar archive")?;
    for entry in entries {
        let mut entry = entry.map_err(|_| "invalid tar member")?;
        if !entry.header().entry_type().is_file() {
            return Err("archive contains a link or special member".into());
        }
        let component = {
            let raw_path = entry.path_bytes();
            let raw_path =
                std::str::from_utf8(&raw_path).map_err(|_| "non-UTF-8 archive member")?;
            validate_archive_basename(raw_path)?.to_owned()
        };
        let max = match component.as_str() {
            "codegg" | "codegg-sandbox-helper" | "codegg-eggsearch" => MAX_RUNFILE_BYTES,
            "THIRD-PARTY-NOTICES.txt" => MAX_NOTICE_BYTES,
            _ => return Err("archive contains an undeclared member".into()),
        };
        if !seen.insert(component.clone()) || !folded.insert(component.to_ascii_lowercase()) {
            return Err("archive contains duplicate or case-colliding members".into());
        }
        let size = entry.header().size().map_err(|_| "invalid member size")?;
        if size > max {
            return Err("archive member exceeds size bound".into());
        }
        total = total.checked_add(size).ok_or("archive size overflow")?;
        if total > MAX_ARCHIVE_BYTES {
            return Err("expanded archive exceeds size bound".into());
        }
        let out = destination.join(&component);
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&out)
            .map_err(|_| "could not create extracted member")?;
        std::io::copy(&mut entry, &mut file).map_err(|_| "could not extract archive member")?;
        if file
            .metadata()
            .map_err(|_| "could not inspect extracted member")?
            .len()
            != size
        {
            return Err("archive member size mismatch".into());
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = if component == "THIRD-PARTY-NOTICES.txt" {
                0o600
            } else {
                0o700
            };
            fs::set_permissions(&out, fs::Permissions::from_mode(mode))
                .map_err(|_| "could not set extracted permissions")?;
        }
    }
    for required in ["codegg", "codegg-sandbox-helper", "codegg-eggsearch"] {
        if !seen.contains(required) {
            return Err(format!("archive is missing required member {required}"));
        }
    }
    Ok(())
}

fn validate_archive_basename(path: &str) -> Result<&str, String> {
    if path.contains('\\') || path.chars().any(char::is_control) {
        return Err("unsafe archive member name".into());
    }
    let mut components = path.split('/');
    let Some(name) = components.next() else {
        return Err("archive member path is empty".into());
    };
    if name.is_empty() || name == "." || name == ".." || components.next().is_some() {
        return Err("archive member path is not a top-level basename".into());
    }
    Ok(name)
}

fn build_members(extracted: &Path, install_root: &Path) -> Result<Vec<ArtifactMember>, String> {
    let mut names = vec!["codegg", "codegg-sandbox-helper", "codegg-eggsearch"];
    // Do not replace an existing notice whose ownership cannot be proven.
    if extracted.join("THIRD-PARTY-NOTICES.txt").exists()
        && !install_root.join("THIRD-PARTY-NOTICES.txt").exists()
    {
        names.push("THIRD-PARTY-NOTICES.txt");
    }
    names
        .into_iter()
        .map(|name| {
            let source = extracted.join(name);
            let digest = sha256_file(&source)?;
            let id = MemberId::new(name).map_err(|_| "invalid member id")?;
            let permissions = if name == "THIRD-PARTY-NOTICES.txt" {
                PermissionsIntent::Preserve
            } else {
                PermissionsIntent::Executable
            };
            Ok(ArtifactMember::new(id, source, name)
                .map_err(|e| format!("invalid member: {e}"))?
                .with_integrity(IntegrityRequirement::Sha256(digest))
                .with_permissions(permissions))
        })
        .collect()
}

struct CodeggValidator {
    version: String,
}

impl CandidateValidator for CodeggValidator {
    fn validate(&self, transaction: &VerifiedTransaction) -> eggup_core::Result<()> {
        for (name, identity, expected) in [
            ("codegg", "codegg", self.version.as_str()),
            ("codegg-eggsearch", "eggsearch", PINNED_EGGSEARCH_VERSION),
        ] {
            let path = transaction.staged_path(&MemberId::new(name)?)?;
            let output = run_bounded(
                &CommandSpec::new(path)
                    .arg("--version")
                    .timeout(Duration::from_secs(5))
                    .max_output_bytes(4096),
            )?;
            let text = String::from_utf8_lossy(output.stdout());
            if !output.success() || !version_probe_matches(&text, identity, expected) {
                return Err(eggup_core::Error::VerificationFailed(format!(
                    "{name} identity/version check failed"
                )));
            }
        }
        let helper = transaction.staged_path(&MemberId::new("codegg-sandbox-helper")?)?;
        let output = run_bounded(
            &CommandSpec::new(helper)
                .timeout(Duration::from_secs(3))
                .max_output_bytes(4096),
        )?;
        let text = String::from_utf8_lossy(output.stderr());
        if output.success()
            || !["sandbox", "protocol", "unavailable", "spec", "status-fd"]
                .iter()
                .any(|part| text.to_ascii_lowercase().contains(part))
        {
            return Err(eggup_core::Error::VerificationFailed(
                "sandbox helper safe identity probe failed".into(),
            ));
        }
        Ok(())
    }
}

struct CodeggOwnership {
    root: PathBuf,
    current_exe: PathBuf,
    current_version: String,
}

impl OwnershipVerifier for CodeggOwnership {
    fn verify(&self, member: &MemberId, destination: &Path) -> Ownership {
        let full = self.root.join(destination);
        let metadata = match fs::symlink_metadata(&full) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return if member.as_str() == "codegg" {
                    Ownership::Unknown
                } else {
                    Ownership::Absent
                };
            }
            Err(_) => return Ownership::Unknown,
            Ok(metadata) if metadata.is_file() && !metadata.file_type().is_symlink() => metadata,
            Ok(_) => return Ownership::Foreign,
        };
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            if metadata.nlink() != 1 {
                return Ownership::Foreign;
            }
        }
        let canonical = match fs::canonicalize(&full) {
            Ok(path) => path,
            Err(_) => return Ownership::Unknown,
        };
        if member.as_str() == "codegg" {
            return if canonical == self.current_exe
                && probe_version(&full, "codegg", &self.current_version)
            {
                Ownership::Owned
            } else {
                Ownership::Unknown
            };
        }
        if member.as_str() == "codegg-eggsearch" {
            return if probe_version(&full, "eggsearch", PINNED_EGGSEARCH_VERSION) {
                Ownership::Owned
            } else {
                Ownership::Unknown
            };
        }
        if member.as_str() == "codegg-sandbox-helper" {
            return if probe_helper(&full) {
                Ownership::Owned
            } else {
                Ownership::Unknown
            };
        }
        Ownership::Unknown
    }
}

fn probe_version(path: &Path, identity: &str, version: &str) -> bool {
    let Ok(output) = run_bounded(
        &CommandSpec::new(path)
            .arg("--version")
            .timeout(Duration::from_secs(3))
            .max_output_bytes(4096),
    ) else {
        return false;
    };
    let text = String::from_utf8_lossy(output.stdout());
    output.success() && version_probe_matches(&text, identity, version)
}

fn version_probe_matches(output: &str, identity: &str, expected: &str) -> bool {
    let first = output.lines().next().unwrap_or("").trim();
    let tokens = first
        .split(|character: char| !character.is_ascii_alphanumeric() && character != '.')
        .filter(|token| !token.is_empty())
        .collect::<Vec<_>>();
    tokens.len() == 2
        && tokens[0].eq_ignore_ascii_case(identity)
        && tokens[1].trim_start_matches(['v', 'V']) == expected
}

fn probe_helper(path: &Path) -> bool {
    let Ok(output) = run_bounded(
        &CommandSpec::new(path)
            .timeout(Duration::from_secs(3))
            .max_output_bytes(4096),
    ) else {
        return false;
    };
    let text = String::from_utf8_lossy(output.stderr()).to_ascii_lowercase();
    !output.success()
        && ["sandbox", "protocol", "unavailable", "spec", "status-fd"]
            .iter()
            .any(|part| text.contains(part))
}

struct CodeggTransport {
    client: eggfetch_core::Client,
}

impl CodeggTransport {
    fn new() -> Result<Self, AcquisitionError> {
        let client = crate::http_client::ordinary_http_client_builder(
            eggfetch_core::Timeout::from_secs(180),
        )
        .build();
        Ok(Self { client })
    }

    fn block_on<F: std::future::Future>(&self, future: F) -> F::Output {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("upgrade runtime")
            .block_on(future)
    }
}

impl AcquisitionTransport for CodeggTransport {
    fn fetch_metadata(
        &self,
        request: &AcquisitionRequest,
        limits: FetchLimits,
        cancel: &CancelFlag,
    ) -> Result<FetchOutcome<MetadataBytes>, AcquisitionError> {
        limits.validate()?;
        if cancel.is_cancelled() {
            return Err(AcquisitionError::Cancelled);
        }
        let timeout = eggfetch_core::Timeout::builder()
            .connect(limits.connect_timeout)
            .total(limits.total_timeout)
            .build();
        self.block_on(async {
            let mut response = self
                .client
                .get(request.url())
                .map_err(|_| {
                    AcquisitionError::Transport("metadata request could not be built".into())
                })?
                .header("User-Agent", "codegg")
                .timeout(timeout)
                .max_decoded_body_size(limits.max_metadata_bytes)
                .send()
                .await
                .map_err(|_| {
                    AcquisitionError::Transport(format!(
                        "metadata request failed for {}",
                        eggup_acquisition::redact_url(request.url())
                    ))
                })?;
            match response.status().as_u16() {
                404 => Ok(FetchOutcome::NotFound),
                status if !(200..300).contains(&status) => {
                    Err(AcquisitionError::Transport(format!(
                        "HTTP {status} from {}",
                        eggup_acquisition::redact_url(request.url())
                    )))
                }
                _ => {
                    let bytes = response.bytes().await.map_err(|_| {
                        AcquisitionError::Transport("metadata body read failed".into())
                    })?;
                    if bytes.len() > limits.max_metadata_bytes {
                        return Err(AcquisitionError::TooLarge {
                            limit: limits.max_metadata_bytes as u64,
                        });
                    }
                    Ok(FetchOutcome::Success(
                        eggup_acquisition::__adapter_metadata(bytes.to_vec()),
                    ))
                }
            }
        })
    }

    fn fetch_artifact(
        &self,
        request: &AcquisitionRequest,
        dest: &Path,
        limits: FetchLimits,
        cancel: &CancelFlag,
    ) -> Result<FetchOutcome<ArtifactEvidence>, AcquisitionError> {
        limits.validate()?;
        let parent = dest
            .parent()
            .ok_or_else(|| AcquisitionError::InvalidInput("artifact has no parent".into()))?;
        let metadata = fs::symlink_metadata(parent)
            .map_err(|_| AcquisitionError::Io("artifact parent unavailable".into()))?;
        if !metadata.is_dir() || metadata.file_type().is_symlink() {
            return Err(AcquisitionError::InvalidInput(
                "artifact parent is not a real directory".into(),
            ));
        }
        if fs::symlink_metadata(dest).is_ok() {
            return Err(AcquisitionError::InvalidInput(
                "artifact destination already exists".into(),
            ));
        }
        let timeout = eggfetch_core::Timeout::builder()
            .connect(limits.connect_timeout)
            .total(limits.total_timeout)
            .build();
        self.block_on(async {
            let mut response = self
                .client
                .get(request.url())
                .map_err(|_| {
                    AcquisitionError::Transport("artifact request could not be built".into())
                })?
                .header("User-Agent", "codegg")
                .timeout(timeout)
                .send()
                .await
                .map_err(|_| {
                    AcquisitionError::Transport(format!(
                        "artifact request failed for {}",
                        eggup_acquisition::redact_url(request.url())
                    ))
                })?;
            match response.status().as_u16() {
                404 => return Ok(FetchOutcome::NotFound),
                status if !(200..300).contains(&status) => {
                    return Err(AcquisitionError::Transport(format!(
                        "HTTP {status} from {}",
                        eggup_acquisition::redact_url(request.url())
                    )))
                }
                _ => {}
            }
            let mut file = OpenOptions::new()
                .create_new(true)
                .write(true)
                .open(dest)
                .map_err(|_| {
                    AcquisitionError::Io("could not create artifact destination".into())
                })?;
            let result: Result<u64, AcquisitionError> = async {
                let mut stream = response.bytes_stream().map_err(|_| {
                    AcquisitionError::Transport("artifact response stream unavailable".into())
                })?;
                let mut count = 0u64;
                while let Some(chunk) = stream.next().await {
                    if cancel.is_cancelled() {
                        return Err(AcquisitionError::Cancelled);
                    }
                    let chunk = chunk.map_err(|_| {
                        AcquisitionError::Transport("artifact stream failed".into())
                    })?;
                    count = count.saturating_add(chunk.len() as u64);
                    if limits.max_artifact_bytes.is_some_and(|max| count > max) {
                        return Err(AcquisitionError::TooLarge {
                            limit: limits.max_artifact_bytes.unwrap(),
                        });
                    }
                    file.write_all(&chunk)
                        .map_err(|_| AcquisitionError::Io("artifact write failed".into()))?;
                }
                file.flush()
                    .map_err(|_| AcquisitionError::Io("artifact flush failed".into()))?;
                Ok(count)
            }
            .await;
            match result {
                Ok(bytes_written) => Ok(FetchOutcome::Success(
                    eggup_acquisition::__adapter_artifact(bytes_written),
                )),
                Err(error) => {
                    drop(file);
                    let _ = fs::remove_file(dest);
                    Err(error)
                }
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use eggup_acquisition::{FixtureResponse, FixtureTransport};
    use flate2::write::GzEncoder;
    use flate2::Compression;
    use std::fs::File;

    fn make_archive(path: &Path, members: &[(&str, &[u8])]) {
        let file = File::create(path).unwrap();
        let encoder = GzEncoder::new(file, Compression::default());
        let mut archive = tar::Builder::new(encoder);
        for (name, bytes) in members {
            let mut header = tar::Header::new_gnu();
            header.set_size(bytes.len() as u64);
            header.set_mode(0o755);
            header.set_cksum();
            archive.append_data(&mut header, name, *bytes).unwrap();
        }
        archive.into_inner().unwrap().finish().unwrap();
    }

    #[test]
    fn checksum_selection_requires_one_exact_basename() {
        let digest = "ab".repeat(32);
        let manifest = format!("{digest}  codegg-x86_64-unknown-linux-gnu.tar.gz\n");
        assert!(checksum_for_archive(
            manifest.as_bytes(),
            "codegg-x86_64-unknown-linux-gnu.tar.gz"
        )
        .is_ok());
        assert!(checksum_for_archive(b"", "missing.tar.gz").is_err());
        assert!(checksum_for_archive(
            format!("{manifest}{manifest}").as_bytes(),
            "codegg-x86_64-unknown-linux-gnu.tar.gz"
        )
        .is_err());
        assert!(checksum_for_archive(
            format!("{digest}  path/codegg.tar.gz\n").as_bytes(),
            "codegg.tar.gz"
        )
        .is_err());
    }

    #[test]
    fn extraction_accepts_only_required_members_and_fixed_notice() {
        let work = tempfile::tempdir().unwrap();
        let archive_path = work.path().join("bundle.tar.gz");
        make_archive(
            &archive_path,
            &[
                ("codegg", b"main"),
                ("codegg-sandbox-helper", b"helper"),
                ("codegg-eggsearch", b"eggsearch"),
                ("THIRD-PARTY-NOTICES.txt", b"notice"),
            ],
        );
        let output = work.path().join("out");
        fs::create_dir(&output).unwrap();
        extract_bundle(&archive_path, &output).unwrap();
        assert_eq!(fs::read(output.join("codegg")).unwrap(), b"main");
        assert_eq!(
            fs::read(output.join("THIRD-PARTY-NOTICES.txt")).unwrap(),
            b"notice"
        );
    }

    #[test]
    fn extraction_rejects_missing_extra_nested_and_duplicate_members() {
        for members in [
            vec![("codegg", b"main".as_slice())],
            vec![
                ("codegg", b"main".as_slice()),
                ("codegg-sandbox-helper", b"helper".as_slice()),
                ("codegg-eggsearch", b"eggsearch".as_slice()),
                ("unexpected", b"extra".as_slice()),
            ],
            vec![
                ("codegg", b"main".as_slice()),
                ("codegg-sandbox-helper", b"helper".as_slice()),
                ("codegg-eggsearch", b"eggsearch".as_slice()),
                ("nested/file", b"bad".as_slice()),
            ],
            vec![
                ("codegg", b"first".as_slice()),
                ("codegg", b"second".as_slice()),
                ("codegg-sandbox-helper", b"helper".as_slice()),
                ("codegg-eggsearch", b"eggsearch".as_slice()),
            ],
            vec![
                ("codegg\\other", b"bad".as_slice()),
                ("codegg-sandbox-helper", b"helper".as_slice()),
                ("codegg-eggsearch", b"eggsearch".as_slice()),
            ],
            vec![
                ("CODEGG", b"bad".as_slice()),
                ("codegg-sandbox-helper", b"helper".as_slice()),
                ("codegg-eggsearch", b"eggsearch".as_slice()),
            ],
        ] {
            let work = tempfile::tempdir().unwrap();
            let archive_path = work.path().join("bundle.tar.gz");
            make_archive(&archive_path, &members);
            let output = work.path().join("out");
            fs::create_dir(&output).unwrap();
            assert!(extract_bundle(&archive_path, &output).is_err());
        }
    }

    #[test]
    fn extraction_rejects_symlink_members() {
        let work = tempfile::tempdir().unwrap();
        let archive_path = work.path().join("bundle.tar.gz");
        let file = File::create(&archive_path).unwrap();
        let encoder = GzEncoder::new(file, Compression::default());
        let mut archive = tar::Builder::new(encoder);
        let mut link = tar::Header::new_gnu();
        link.set_entry_type(tar::EntryType::Symlink);
        link.set_size(0);
        link.set_link_name("target").unwrap();
        link.set_cksum();
        archive.append_data(&mut link, "codegg", &[][..]).unwrap();
        archive.into_inner().unwrap().finish().unwrap();
        let output = work.path().join("out");
        fs::create_dir(&output).unwrap();
        assert!(extract_bundle(&archive_path, &output).is_err());
        assert!(!output.join("codegg").exists());
    }

    #[test]
    fn archive_paths_reject_absolute_traversal_dot_nested_and_backslash() {
        for path in [
            "",
            ".",
            "..",
            "../codegg",
            "./codegg",
            "nested/codegg",
            "/codegg",
            "codegg\\other",
        ] {
            assert!(
                validate_archive_basename(path).is_err(),
                "accepted {path:?}"
            );
        }
        assert_eq!(validate_archive_basename("codegg").unwrap(), "codegg");
    }

    #[test]
    fn checked_in_target_matrix_matches_prebuilt_installer_contract() {
        let fixtures = [
            (
                "x86_64-unknown-linux-gnu",
                "codegg-x86_64-unknown-linux-gnu.tar.gz",
            ),
            (
                "aarch64-unknown-linux-gnu",
                "codegg-aarch64-unknown-linux-gnu.tar.gz",
            ),
            ("x86_64-apple-darwin", "codegg-x86_64-apple-darwin.tar.gz"),
            ("aarch64-apple-darwin", "codegg-aarch64-apple-darwin.tar.gz"),
        ];
        assert_eq!(SUPPORTED_TARGETS.len(), fixtures.len());
        for (target, archive) in fixtures {
            assert_eq!(archive_asset_name(target).unwrap(), archive);
            assert!(SUPPORTED_TARGETS.contains(&target));
        }
        assert!(archive_asset_name("x86_64-pc-windows-msvc").is_err());
    }

    #[test]
    fn version_probe_requires_exact_identity_and_version_token() {
        assert!(version_probe_matches("CodeGG v2.3.4\n", "codegg", "2.3.4"));
        assert!(version_probe_matches(
            "eggsearch 0.3.9\n",
            "eggsearch",
            "0.3.9"
        ));
        assert!(!version_probe_matches(
            "notcodegg 2.3.4\n",
            "codegg",
            "2.3.4"
        ));
        assert!(!version_probe_matches("codegg 12.3.4\n", "codegg", "2.3.4"));
        assert!(!version_probe_matches(
            "codegg 2.3.4-extra\n",
            "codegg",
            "2.3.4"
        ));
    }

    #[test]
    fn acquisition_wrappers_preserve_not_found_bounds_and_redaction() {
        let transport = FixtureTransport::new();
        let cancel = CancelFlag::new();
        let url = "https://example.test/checksums.txt?token=secret";
        transport.route(url, FixtureResponse::body(b"checksums".to_vec()));
        assert_eq!(
            fetch_metadata(&transport, url, 64, &cancel).unwrap(),
            b"checksums"
        );
        assert!(fetch_metadata(&transport, url, 4, &cancel).is_err());
        transport.route("https://example.test/missing", FixtureResponse::not_found());
        assert!(
            fetch_metadata(&transport, "https://example.test/missing", 64, &cancel)
                .unwrap_err()
                .contains("not found")
        );
        transport.route(
            "https://example.test/fail?token=private",
            FixtureResponse::failure("private upstream text"),
        );
        let error = fetch_metadata(
            &transport,
            "https://example.test/fail?token=private",
            64,
            &cancel,
        )
        .unwrap_err();
        assert!(!error.contains("private"));
        assert!(!error.contains("token="));
    }

    #[test]
    fn acquisition_wrapper_cleans_partial_artifact_failure() {
        let transport = FixtureTransport::new();
        let url = "https://example.test/codegg.tar.gz";
        transport.route(url, FixtureResponse::truncated(16));
        let work = tempfile::tempdir().unwrap();
        let dest = work.path().join("archive.tar.gz");
        assert!(fetch_artifact(
            &transport,
            url,
            &dest,
            MAX_ARCHIVE_BYTES,
            &CancelFlag::new()
        )
        .is_err());
        assert!(!dest.exists());
        assert_eq!(work.path().read_dir().unwrap().count(), 0);
    }
}
