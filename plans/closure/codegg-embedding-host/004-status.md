# CodeGG Embedding Host E004 — Integration evidence and closure: closure record

Status: **closed**.

Plan: [004-integration-evidence-and-closure](../../implementation/codegg-embedding-host/004-integration-evidence-and-closure.md).
Stability policy: [ADR-0061](../../adrs/ADR-0061-embedding-host-wire-as-a-public-contract.md).
Commit: `089f9479c` plus the follow-up commit on `feat/codegg-embedding-host`.

The host used for this evidence is a real program that speaks only the documented
wire: it builds a descriptor from EggUI's saved provider connection, spawns
`codegg host --stdio`, and drives it over the pipe. No internal API, no test
seam, no config file.

## Build identity

| Field | Value |
|---|---|
| Binary | `codegg` `0.1.0`, `dev` profile, macOS 26 arm64, and the pinned Linux build inside the coding image (below) |
| Command | `/tmp/codegg-e004/codegg host --stdio` |
| Provider connection | EggUI connection `ocg`, `https://opencode.ai/inference/go/openai/v1/` |
| Model | `deepseek-v4.1-flash` — absent from every built-in reviewed list |
| Required header | `x-opencode-session`, supplied by the descriptor |

## Transcripts

### Handshake, no provider, EOF

```text
-> {"jsonrpc":"2.0","id":1,"method":"initialize","params":{"clientName":"e004"}}
<- {"jsonrpc":"2.0","id":1,"result":{"apiVersion":1,"codeggVersion":"0.1.0","capabilities":["descriptor","streaming","cancellation","usage"]}}
exit=0
```

### A real turn with tool use

```text
-> initialize
<- apiVersion 1, capabilities [descriptor, streaming, cancellation, usage]
-> session/open {workspace: /tmp/codegg-e004/ws, model: "deepseek-v4.1-flash",
                 agent: "build", approval: "yolo", sandbox: "full-host",
                 provider: {id: "eggui-e004", baseUrl: …, headers: [{x-opencode-session, …}],
                            credential: {kind: "api_key", value: <redacted>},
                            models: [{id: "deepseek-v4.1-flash", contextWindow: 128000,
                                      maxOutputTokens: 8192, supportsTools: true}]}}
<- {"sessionId":"2423cfa6-…","model":"deepseek-v4.1-flash","agent":"build","workspace":"/tmp/codegg-e004/ws"}
-> turn/submit {sessionId, turnId: "t1", prompt: "Use bash to run: echo hi. Then reply with the single word: ok"}
<- {"turnId":"t1"}
<- turn/text ×2
<- turn/tool {phase:"started", toolId, toolName:"bash"}
<- turn/tool {phase:"finished", success:true}
<- turn/usage
<- turn/done {"stopReason":"end_turn","usage":{…}}
-> session/close
<- {"closed":true}
host exit: 0
```

An earlier, longer turn in the same setup ran 20+ tool calls
(`repo_map`, `bash`, `security`, `tool_search`, `tool_program`), every one
`success: true`, which is the proof that a host session can execute tools — not
merely stream text — with no daemon and no project.

### Cancellation

```text
-> turn/submit {"turnId":"c1"} (a long "read every file and summarise it" prompt)
-> turn/cancel {"sessionId":…,"turnId":"c1"}
<- {"cancelled":true}
<- turn/done {"stopReason":"cancelled"}          after 0.38 s
-> turn/submit {"turnId":"c2","prompt":"Reply with the single word: ok"}
<- turn/done {"stopReason":"end_turn"}
```

Cancellation is bounded well inside the 5 s the plan requires, and the next turn
runs on the same session.

### A typed provider failure

```text
-> turn/submit {"prompt":"Say ok."} with model "no-such-model-xyz"
<- turn/error {"class":"invalid_request","message":"the upstream rejected the request",
               "retryable":false,"providerStatus":400,"usage":{"reported":false}}
```

The process stayed alive and the session closed cleanly afterwards. The upstream
answers an unknown model with `400`, so `invalid_request` is the honest class;
the taxonomy is what made it actionable without parsing prose.

### Restart

```text
run 1: session=cf52fb4d-… stopReason=end_turn exit=0
run 2: session=e75649c4-… stopReason=end_turn exit=0
```

Two processes, two fresh sessions, no residual state.

### Usage

The same upstream sometimes returns every counter at zero for a completed turn.
The first evidence run reported `{"reported":true,"inputTokens":0,"outputTokens":0}`,
which is a lie a host could budget against. The terminal frame now takes the
usage of the turn's own final response, and a response whose counters are all
zero is reported as `{"reported":false}`. A turn whose provider did report
numbers carries them: `inputTokens: 19119, outputTokens: 4` was observed on
`claude-haiku-5-5` in the same setup.

## Log scan

The driver redacts the credential and the header value before printing, and the
raw captured transcripts were scanned for credential-shaped strings
(`sk-[A-Za-z0-9]{20,}`): **0 matches** across the error and restart logs. The
host's own stderr was empty for every run.

## Compatibility

ACP and `exec` are untouched by this milestone: `cargo test -p codegg --bin
codegg exec` is unchanged, and `src/acp.rs` is not modified.

## The pinned Linux build

The same driver ran unchanged against the image built from this branch
(CodeGG `089f9479c`, `aarch64-unknown-linux-gnu`, `--release --locked` with
`lto=false` / `codegen-units=16` in a 6 vCPU / 12 GiB VM), spawned through
`eggui-vm run`:

```text
-> initialize            <- {"apiVersion":1,"codeggVersion":"0.1.0",
                             "capabilities":["descriptor","streaming","cancellation","usage"]}
-> session/open          <- {"sessionId":"65ff3c65-…","model":"deepseek-v4.1-flash",
                             "agent":"build","workspace":"/workspace"}
-> turn/submit {"prompt":"Use bash to run: echo hi. Then reply with the single word: ok"}
<- turn/text ×2
<- turn/tool {phase:"started", toolName:"bash"}
<- turn/tool {phase:"finished", success:true}
<- turn/usage, turn/done {"stopReason":"end_turn"}
-> session/close         <- {"closed":true}
host exit: 0
```

The image verification reports **51 PASS / 0 FAIL**, asserts no relay and no
`gateway.json`, and checks `initialize` answers `apiVersion: 1` with no provider
and no network.

## Outstanding

- `exec` still carries its own copy of the loop setup; delegating it to
  `HeadlessRuntime` is a mechanical follow-up with no behaviour change.
- An upstream that answers a completed turn with all-zero counters is now
  reported as `reported: false`; the per-response numbers are proved by the
  `claude-haiku-5-5` turn (`inputTokens: 19119, outputTokens: 4`) and by the
  usage unit tests.

## Recommendation

**Closed.** The descriptor, the wire and the SDK are proved against a real host,
a real provider, a real model outside every built-in list, and the pinned Linux
build, including a tool-executing turn, cancellation in 0.38 s, a typed provider
failure and a clean restart.
