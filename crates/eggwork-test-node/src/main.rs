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

use eggwork_runner::{LocalProcessRunner, NoExecutionSetup};
use eggwork_server::{
    FingerprintPrincipalResolver, NodeConfig, NodePrincipal, NodeServer, Operation,
};

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
"#
    .to_string()
}

struct Args {
    values: HashMap<String, String>,
}

impl Args {
    fn parse() -> Result<Self, String> {
        let mut values = HashMap::new();
        let mut raw = std::env::args().skip(1).peekable();
        while let Some(arg) = raw.next() {
            if arg == "--help" || arg == "-h" {
                return Err(usage());
            }
            let Some(key) = arg.strip_prefix("--") else {
                return Err(format!("expected --flag, got '{arg}'\n{}", usage()));
            };
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
    let args = Args::parse()?;
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
    // `LocalProcessRunner` is the node's canonical process owner.
    // `NoExecutionSetup` keeps the fixture hermetic on ordinary CI hosts
    // (no sandbox-helper sibling install required); BestEffort isolation
    // downgrades to NotApplied while Required still fails.
    let runner = Arc::new(LocalProcessRunner::new(NoExecutionSetup));
    let server = NodeServer::start(
        NodeConfig {
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
