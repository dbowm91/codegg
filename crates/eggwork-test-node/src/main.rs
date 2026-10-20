//! Bounded Eggwork test node for CodeGG C001 live qualification.
//!
//! Hosts a real [`NodeServer`](eggwork_server::NodeServer) with
//! [`LocalProcessRunner`](eggwork_runner::LocalProcessRunner) as the
//! canonical process owner, exactly as `eggworkd` would. All trust and
//! identity material arrives as caller-supplied PEM files; nothing is
//! generated here and no secret bytes are ever printed.
//!
//! Protocol: on readiness the helper prints `READY port=<N>` to stdout and
//! then waits for SIGINT/SIGTERM or stdin EOF. The test harness kills the
//! child on completion (`kill_on_drop`) and removes the temp state roots.

use std::collections::HashMap;
use std::io::BufReader;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use eggwork_runner::{LocalProcessRunner, TrustedLandlockSetup};
use eggwork_server::{
    telemetry::HostTelemetryConfig, FingerprintPrincipalResolver, NodeConfig, NodePrincipal,
    NodeServer, Operation,
};

mod synthetic_provider;

fn usage() -> String {
    r#"codegg-eggwork-test-node: bounded Eggwork fixture node

Required:
  --node-id <id>            Eggwork node id
  --bind <addr>             bind socket (use 127.0.0.1:0 for ephemeral)
  --db <path>               node sqlite path (fresh temp file)
  --exec-root <dir>         execution root dir
  --blob-root <dir>         blob store dir
  --workspace-root <dir>    workspace store dir
  --server-cert <pem>       server certificate chain (PEM)
  --server-key <pem>        server private key (PEM)
  --ca <pem>                client-auth trust root(s) (PEM)
  --client-cert <pem>       authorized controller certificate (PEM)
  --principal <name>        principal id mapped to --client-cert

Optional:
  --lease-ttl-secs <n>      execution lease TTL (default 300)
  --max-active <n>          max active executions (default 4)
  --blob-quota-bytes <n>    (default 268435456)
  --workspace-quota-bytes <n> (default 268435456)
  --telemetry-provider      run a synthetic loopback Gregg provider and
                            configure the node's host telemetry against it
"#
    .to_string()
}

#[derive(Debug)]
struct Args {
    values: HashMap<String, String>,
}

/// Flags that carry no value.
///
/// The usage text above documents `--telemetry-provider` as a switch while
/// every other option shows a `<value>`. The parser reads a value for
/// every flag, so without this list the switch consumed the next argument
/// — or, as the last argument, failed with "missing value". The switch is
/// checked with `contains_key`, so its value is never read.
const BOOLEAN_FLAGS: &[&str] = &["telemetry-provider"];

impl Args {
    fn parse() -> Result<Self, String> {
        Self::parse_from(std::env::args().skip(1))
    }

    /// Parse from an explicit argument sequence, so the contract is
    /// testable without spawning the binary.
    fn parse_from<I: IntoIterator<Item = String>>(args: I) -> Result<Self, String> {
        let mut values = HashMap::new();
        let mut raw = args.into_iter().peekable();
        while let Some(arg) = raw.next() {
            if arg == "--help" || arg == "-h" {
                return Err(usage());
            }
            let Some(key) = arg.strip_prefix("--") else {
                return Err(format!("expected --flag, got '{arg}'\n{}", usage()));
            };
            if BOOLEAN_FLAGS.contains(&key) {
                values.insert(key.to_string(), "true".to_string());
                continue;
            }
            let Some(value) = raw.next() else {
                return Err(format!("missing value for --{key}\n{}", usage()));
            };
            values.insert(key.to_string(), value);
        }
        Ok(Self { values })
    }

    fn required(&self, key: &str) -> Result<String, String> {
        self.values
            .get(key)
            .cloned()
            .ok_or_else(|| format!("missing required --{key}\n{}", usage()))
    }

    fn optional(&self, key: &str, default: &str) -> String {
        self.values
            .get(key)
            .cloned()
            .unwrap_or_else(|| default.to_string())
    }
}

fn read_certs(path: &str) -> Result<Vec<rustls::pki_types::CertificateDer<'static>>, String> {
    let file = std::fs::File::open(path).map_err(|e| format!("open {path}: {e}"))?;
    rustls_pemfile::certs(&mut BufReader::new(file))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| format!("parse certificates in {path}: {e}"))
}

fn read_key(path: &str) -> Result<rustls::pki_types::PrivateKeyDer<'static>, String> {
    let file = std::fs::File::open(path).map_err(|e| format!("open {path}: {e}"))?;
    rustls_pemfile::private_key(&mut BufReader::new(file))
        .map_err(|e| format!("parse private key in {path}: {e}"))?
        .ok_or_else(|| format!("no private key found in {path}"))
}

async fn run() -> Result<NodeServer, String> {
    fn phase(stage: &str) {
        use std::io::Write;
        println!("PHASE {stage}");
        let _ = std::io::stdout().flush();
    }
    let args = Args::parse()?;
    // Test-only startup phase markers on stdout (C002 WP3): the harness
    // parses stdout for `READY` and tolerates these lines; on a readiness
    // timeout the last observed phase pinpoints the stall. Stage names
    // only — never paths, identities, or key material.
    phase("args-parsed");
    let node_id = eggwork_core::NodeId::new(args.required("node-id")?)
        .map_err(|e| format!("invalid --node-id: {e:?}"))?;
    let bind: std::net::SocketAddr = args
        .required("bind")?
        .parse()
        .map_err(|e| format!("invalid --bind: {e}"))?;
    let tls = eggserve_core::tls::TlsServerConfig::builder()
        .single_identity(
            read_certs(&args.required("server-cert")?)?,
            read_key(&args.required("server-key")?)?,
        )
        .map_err(|e| format!("server identity invalid: {e:?}"))?
        .client_auth_required(read_certs(&args.required("ca")?)?)
        .map_err(|e| format!("trust roots invalid: {e:?}"))?
        .build()
        .map_err(|e| format!("TLS configuration invalid: {e:?}"))?;
    phase("tls-ready");

    let client_certs = read_certs(&args.required("client-cert")?)?;
    let leaf = client_certs
        .first()
        .ok_or_else(|| "client certificate file is empty".to_string())?;
    let principal = eggwork_core::PrincipalId::new(args.required("principal")?)
        .map_err(|e| format!("invalid --principal: {e:?}"))?;
    let resolver = Arc::new(FingerprintPrincipalResolver::new([(
        FingerprintPrincipalResolver::fingerprint(leaf.as_ref()),
        principal,
    )]));
    // Authorize exactly the fixed-target executor operation set. Any future
    // server operation fails closed here at compile time (no wildcard arm).
    //
    // Eggwork M002: the host-telemetry read is granted unconditionally. It
    // is a read of the node's own provider state, and "no provider" is one
    // of the answers it has to be able to give — a node with no provider
    // must be able to say so. Forbidding the read when there is no provider
    // would make the node answer "forbidden" instead of "disabled", which
    // is both less honest and not what the route is for.
    let authorizer = Arc::new(|_: &NodePrincipal, operation: Operation| {
        matches!(
            operation,
            Operation::Capabilities
                | Operation::Status
                | Operation::Execute
                | Operation::Observe
                | Operation::Cancel
                | Operation::Renew
                | Operation::Events
                | Operation::BlobRead
                | Operation::BlobWrite
                | Operation::WorkspaceCreate
                | Operation::ArtifactRead
                | Operation::Telemetry
        )
    });

    let lease_ttl = Duration::from_secs(
        args.optional("lease-ttl-secs", "300")
            .parse()
            .map_err(|e| format!("invalid --lease-ttl-secs: {e}"))?,
    );
    let max_active: u32 = args
        .optional("max-active", "4")
        .parse()
        .map_err(|e| format!("invalid --max-active: {e}"))?;
    let blob_quota: u64 = args
        .optional("blob-quota-bytes", "268435456")
        .parse()
        .map_err(|e| format!("invalid --blob-quota-bytes: {e}"))?;
    let workspace_quota: u64 = args
        .optional("workspace-quota-bytes", "268435456")
        .parse()
        .map_err(|e| format!("invalid --workspace-quota-bytes: {e}"))?;

    for key in ["exec-root", "blob-root", "workspace-root"] {
        let dir = args.required(key)?;
        std::fs::create_dir_all(&dir).map_err(|e| format!("create {dir}: {e}"))?;
    }
    phase("storage-ready");
    // `LocalProcessRunner` remains the node's canonical process owner. The
    // test harness builds the helper from this exact Eggwork revision and
    // installs it as a trusted sibling, exercising the qualified backend.
    let runner = Arc::new(LocalProcessRunner::new(
        TrustedLandlockSetup::discover_sibling(),
    ));
    // Eggwork M002: the node's host telemetry is opt-in and default-off.
    // With `--telemetry-provider` the fixture stands up a synthetic loopback
    // Gregg provider and points the node at it; without the flag the node
    // has no provider at all, which is the honest "no telemetry" shape.
    let host_telemetry = if args.values.contains_key("telemetry-provider") {
        let provider = synthetic_provider::start().await?;
        let endpoint = format!("http://127.0.0.1:{}/v2/status", provider.port);
        Some(
            HostTelemetryConfig::new(&endpoint, 1, 5)
                .map_err(|e| format!("configure host telemetry: {e:?}"))?,
        )
    } else {
        None
    };
    phase("server-starting");
    let server = NodeServer::start(
        NodeConfig {
            host_telemetry,
            node_id,
            bind,
            execution_root: PathBuf::from(args.required("exec-root")?),
            database_path: PathBuf::from(args.required("db")?),
            blob_root: PathBuf::from(args.required("blob-root")?),
            blob_quota_bytes: blob_quota,
            workspace_root: PathBuf::from(args.required("workspace-root")?),
            workspace_quota_bytes: workspace_quota,
            max_active_executions: max_active,
            lease_ttl,
            tls,
        },
        runner,
        resolver,
        authorizer,
    )
    .await
    .map_err(|e| format!("node failed to start: {e:?}"))?;
    Ok(server)
}

#[tokio::main]
async fn main() {
    use std::io::Write;
    // Select the process-level rustls provider before any TLS builder runs:
    // with only `ring` enabled and no process default, provider resolution
    // would otherwise panic (same init the Eggwork in-tree tests perform).
    let _ = rustls::crypto::ring::default_provider().install_default();
    let server = match run().await {
        Ok(server) => server,
        Err(message) => {
            // Also mirrored to stdout: the harness parses stdout for
            // `READY`, so a startup failure stays diagnosable there.
            println!("ERROR {message}");
            let _ = std::io::stdout().flush();
            eprintln!("codegg-eggwork-test-node: {message}");
            std::process::exit(1);
        }
    };
    println!("READY port={}", server.local_addr().port());
    let _ = std::io::stdout().flush();
    // Wait for operator shutdown (SIGINT/SIGTERM). The harness kills the
    // child on completion (`kill_on_drop` guarantees cleanup on panic).
    let _ = tokio::signal::ctrl_c().await;
    server.shutdown().await;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(args: &[&str]) -> Result<Args, String> {
        Args::parse_from(args.iter().map(|s| (*s).to_string()))
    }

    /// The usage text documents `--telemetry-provider` as a switch while
    /// every other option shows a `<value>`. A parser that reads a value
    /// for every flag therefore rejected it — and, because the live
    /// harness passes it last, rejected it with "missing value", so the
    /// fixture exited before it ever reached `READY`.
    #[test]
    fn the_telemetry_provider_switch_takes_no_value() {
        let args = parse(&[
            "--node-id",
            "node-1",
            "--telemetry-provider",
            "--exec-root",
            "/tmp/exec",
        ])
        .expect("the switch must parse");
        assert!(args.values.contains_key("telemetry-provider"));
        assert_eq!(
            args.values.get("node-id").map(String::as_str),
            Some("node-1")
        );
        assert_eq!(
            args.values.get("exec-root").map(String::as_str),
            Some("/tmp/exec")
        );
    }

    /// The switch must also work as the last argument, which is how the
    /// live harness passes it.
    #[test]
    fn the_telemetry_provider_switch_may_be_last() {
        let args = parse(&["--node-id", "node-1", "--telemetry-provider"])
            .expect("a trailing switch must parse");
        assert!(args.values.contains_key("telemetry-provider"));
        assert_eq!(
            args.values.get("node-id").map(String::as_str),
            Some("node-1")
        );
    }

    /// A switch that is absent stays absent — the default-off shape the
    /// node's telemetry configuration depends on.
    #[test]
    fn an_absent_switch_is_absent() {
        let args = parse(&["--node-id", "node-1"]).expect("parse");
        assert!(!args.values.contains_key("telemetry-provider"));
    }

    /// A valued flag that is missing its value is still an error.
    #[test]
    fn a_valued_flag_without_its_value_is_refused() {
        let error = parse(&["--node-id"]).expect_err("a valued flag needs a value");
        assert!(
            error.contains("missing value for --node-id"),
            "unexpected error: {error}"
        );
    }
}
