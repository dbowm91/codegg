# 0001 — A tool-call part encoded a duplicate `id` key

## Status

Fixed.

The defecting code reached HEAD `7642b9e38` on branch
`feat-auto-20261109-f8abad0d`, where it stood from `2026-09-25` (the commit that
added `crates/codegg-core/src/session/message.rs`). The fix is in the working
tree on top of that commit. No release tag exists in this clone, so "shipped in
a release" is not claimed: what escaped is the suite and the review of the
change that introduced it, and the defect sat in a merged branch.

## Date

2026-11-11

Written the day the fix landed on this branch; the defect entered the tree on
`2026-09-25`.

## Symptom

A stored transcript containing a tool call could not be read back. Reading one
message was enough: `MessageStore::list` and `MessageStore::get` both decode
through `Message::data` → `MessageData::parts: Vec<PartInfo>`, so any message
with one tool-call part failed the whole read, not just that part.

Two error strings were observed, from the two decode paths:

- `serde_json::from_str` → `` duplicate field `id` ``
- `serde_json::from_value` (the `message_to_dto` bridge) → `` missing field `id` ``

The second is the more confusing of the two: the value it was handed was
produced by `serde_json::to_value` from the same parts, so the value *looked*
well formed. It was well formed — the duplicate had already been collapsed, and
the part id had been lost on the way in.

## Reproduction

The standing reproduction is the guard this record names — it passes today and
fails the moment a shared wire name returns:

```bash
cargo test -p codegg-protocol --lib -- --exact dto::tests::tool_call_part_encodes_exactly_one_id_key
```

The original failure needs the pre-fix shape, which is reachable from history:

```bash
git show 7642b9e38:crates/codegg-core/src/session/message.rs | sed -n '/pub struct PartInfo/,/^}/p'
git show 7642b9e38:crates/codegg-core/src/session/message.rs | sed -n '/^    ToolCall {/,/^    },/p'
```

Both print `id` — once as `PartInfo`'s own field, once inside
`PartData::ToolCall`, joined by `#[serde(flatten)]`. The minimal bytes are that
part serialized:

```json
{"id":"m1_0","sessionID":"s","messageID":"m1","type":"tool_call","id":"call_1","name":"read","input":{},"output":null,"status":"pending"}
```

What this record cannot do is re-run the failure: the fix is in the tree, and
reintroducing the defect would edit production code. ``duplicate field `id` ``
is the string the fix's own wire-format note records
(`crates/codegg-core/src/session/message.rs`, the `PartInfo` section);
`` missing field `id` `` is what the collapsed value must produce, because that
same note records that the tool-call id was the value kept and `PartInfo::id`
was never stored at all.

## Root cause

`PartInfo` carried a part id named `id`, and `PartData::ToolCall` carried a
tool-call id named `id`, and the two were flattened into one JSON object. A
flattened enum contributes its fields to the *same* object as the struct that
flattens it, so serialization emitted two keys named `id`.

Deserialization then had two independent ways to lose:

1. **From text.** The derived deserializer meets `id` twice. The first
   occurrence fills `PartInfo::id`; the second is a duplicate of a known field,
   and serde rejects it. Nothing partial: the row is unreadable.
2. **From a value.** `serde_json::to_value` serializes through a map, and a map
   holds one value per key, so the second write wins. `PartData::ToolCall` is
   serialized after `PartInfo`'s own fields, so the stored `id` was the
   *tool-call* id and `PartInfo::id` was never written at all. Decoding that
   map consumes `id` for `PartInfo` and hands the flattened remainder to
   `PartData::ToolCall`, which then finds no `id` — `` missing field `id` ``.

So the defect was not merely mirrored across two crates. It had two failure
modes that disagree about which id survived: one rejects the row outright, and
the other silently keeps the wrong id. The same bytes failed `from_str` and
produced a different value through `to_value`.

## Why the existing evidence missed it

The suite had a serialization test and a round trip. It used the one variant
that could not fail.

- `test_message_data_serialization` (`crates/codegg-core/src/session/message.rs`)
  does serialize a `MessageData` and decode it back — with a single
  `PartData::Text` part. A text part has no field named `id`, so the round trip
  was true and said nothing about the variant where the collision lives.
- `test_part_info_serialization` (same file) asserts the serialized part's
  `"type":"text"` — the same variant again.
- `test_part_data_tool_call` (same file) is a construction check:
  `assert!(matches!(part, PartData::ToolCall { .. }))`. It builds the variant
  and asserts nothing about its wire form. This is the test that looks like
  coverage and is not.
- The two crates were kept in step by a `message_to_dto` JSON bridge whose
  round trip was stated in a doc comment and asserted nowhere. The test that
  now pins it says so itself: it exists because the round trip "the
  `dto::PartInfo` doc comment claims" was "what nothing exercised"
  (`crates/codegg-core/src/protocol_conversions.rs`).
- Nothing wrote a message containing a tool-call part through `MessageStore`
  and read it back. The property "a persisted message can be read" was never
  asserted for the part type every tool-using turn produces, so the defect was
  invisible structurally rather than missed by an assertion that was slightly
  wrong.

A `serde_json::Value` was the instrument that hid it: a value cannot represent a
duplicate key, so every check performed on a value agreed the part was fine.

## Guard added

- Guard: `crates/codegg-protocol/src/dto.rs::tool_call_part_encodes_exactly_one_id_key`
  Self-test: counts `2` `id` keys in a synthetic object written with a duplicate, which a value map cannot represent.
  Must mention: `toolCallID`

- Guard: `crates/codegg-core/src/session/message.rs::tool_call_part_round_trips_with_distinct_part_and_call_ids`
  Self-test: give the tool-call id a wire name shared with `PartInfo` and the round trip fails with `duplicate field`.
  Must mention: `tool-call id must survive`

- Guard: `crates/codegg-core/src/protocol_conversions.rs::message_to_dto_accepts_a_tool_call_and_preserves_both_ids`
  Self-test: return the part id in place of the tool-call id from `message_to_dto` and the bridge leg fails.
  Must mention: `survive the bridge`

- Guard: `crates/codegg-core/src/protocol_conversions.rs::both_crates_decode_the_same_tool_call_part_json`
  Self-test: let either crate rename the tool-call id and the two decoders disagree on a canonical row.
  Must mention: `toolCallID`

All four run at the paths above:

```bash
cargo test -p codegg-protocol --lib -- --exact dto::tests::tool_call_part_encodes_exactly_one_id_key
cargo test -p codegg-core --lib -- tool_call
```

The first entry is the guard that matters most, and it is the one this defect
taught: `serde_json::Value` structurally cannot see a duplicate key, so the
assertion counts *raw top-level keys* with a visitor rather than comparing a
value. That counter is then proved non-vacuous inside the same test by counting
two `id` keys in a synthetic duplicate-key object — the instrument that hid the
defect is the instrument the test calibrates.

The fix itself is a distinct wire name (`toolCallID`) and a hand-written
`Deserialize for PartInfo` that recovers a pre-rename row: the stored `id` is
the tool-call id, and the part id is synthesized as `{messageID}:{toolCallID}`,
which is why the separator is `:`. It is mirrored in both crates, and it owes no
storage migration: every write is canonical, reads recover in place, and
`STORAGE_LAYOUT_VERSION` was not bumped.

## Blast radius

Every message that carried a tool-call part was unreadable through the store's
own decode path — which is every assistant message produced by a turn that used
a tool, for roughly six weeks of branch history.

- **The write succeeded, so the failure surfaced later.** The turn's transcript
  is persisted from `serde_json::to_value` (`src/agent/turn_completion.rs`,
  `persist_turn_transcript`), and `MessageStore::append_batch` inserts the
  encoded string inside its transaction without decoding it. Nothing failed at
  write time. The error appeared on the next read — a reloaded session, a
  resumed turn, a replay — long after the turn that wrote it reported success.
- **One path left a partial write.** `MessageStore::create_with_id` inserts the
  row and *then* decodes the value it was handed. On a tool-call message the
  insert commits and the decode fails, so the call returns an error with the row
  already in the table. Its caller in the session import path
  (`src/main.rs`, `cmd_import`) uses `?` per message, so an import aborts at the
  first tool call and leaves a half-imported session behind; the retry cannot be
  idempotent because `MessageStore::create` mints a fresh UUID per attempt. The
  rows that did land are the pre-fix shape, which is exactly the shape the
  recovery arm now reads.
- **The stored rows needed read-side recovery, not a rewrite.** Because the
  pre-fix writer collapsed the duplicate, no stored row ever held a part id to
  recover: `PartInfo::id` was lost at write time on every tool-call row. The
  fix rebuilds one deterministically from the message id and the tool-call id
  instead of failing the row.
- **Text, reasoning, image and file parts were never affected.** They carry no
  field named `id`, so the flattened object had one `id` and both decode paths
  agreed. That is also why ordinary session reads worked and the defect was
  reachable only through a tool call.
