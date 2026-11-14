# codegg-host-protocol

The single definition of CodeGG's embedding host wire, `apiVersion: 1`. The host
binary (`src/host/`) and `codegg-sdk` both depend on this crate; a second copy of
these types is a defect (ADR-0060).

The contract of record is [`docs/embedding.md`](../../docs/embedding.md).

```rust
use codegg_host_protocol::{API_VERSION, RequestFrame};
```

Framing, bounds, the error taxonomy and the golden-frame tests live here. The
types carry no transport: the caller owns stdin/stdout.
