# codegg-sdk

The typed client for CodeGG's embedding host protocol (`apiVersion: 1`).

It spawns `codegg host --stdio`, speaks newline-delimited JSON-RPC 2.0 with it,
correlates requests, and delivers notifications as a bounded stream. It depends
on `codegg-host-protocol` — the single wire definition — and on nothing else in
CodeGG: no daemon, no TUI, no frontend protocol.

The contract of record is [`docs/embedding.md`](../../docs/embedding.md); the
stability policy is ADR-0060.

```rust,no_run
use codegg_sdk::HostClient;
use codegg_host_protocol::SessionOpenParams;

# async fn example() -> Result<(), codegg_sdk::HostError> {
let client = HostClient::spawn("codegg", &["host", "--stdio"])?;
client.initialize("my-host", "1.0.0", &[]).await?;
let session = client
    .open_session(SessionOpenParams {
        workspace: "/workspace".to_owned(),
        model: "openai/deepseek-v4.1-flash".to_owned(),
        agent: Some("build".to_owned()),
        approval: Some("yolo".to_owned()),
        sandbox: Some("full-host".to_owned()),
        session_id: None,
        provider: None,
    })
    .await?;
let _turn = client.submit(&session.session_id, "write hello.txt", None).await?;
while let Some(event) = client.recv().await? {
    if matches!(event, codegg_host_protocol::NotificationBody::Done(_)) {
        break;
    }
}
client.close(&session.session_id).await?;
# Ok(())
# }
```
