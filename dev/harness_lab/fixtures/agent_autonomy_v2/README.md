# Agent autonomy recovery controls v2

This is an independent versioned contract. The v1 manifest, fixtures, scores,
receipts and digests remain historical and unchanged. V2 records the observed
ACP terminal, full tool and argument identity, workspace revision, repeated
failure count, actual stop owner, and whether the targeted branch was entered.

`not_exercised` and `unknown` are unresolved evidence, never passes. Pair
scoring requires the same v2 suite version and exact manifest digest. A typed
ACP terminal and a harness-measured workspace revision are required; text in
tool output cannot establish failure or progress.
