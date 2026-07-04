# Envelope Unwrapping Fixtures

Small, controlled JSONL fixtures for exercising the envelope unwrapping and
type-routing logic in `src/parser/jsonl.rs` and `src/plugin.rs::Envelope`.

These fixtures are **not** tied to one real-world agent — they use the same
envelope schema as the Pi plugin (`plugins/pi.toml`) so they can be parsed with
that plugin config, while covering every type-routing branch in a single file.

## Envelope Schema

Matches the `[source.envelope]` block in `plugins/pi.toml`:

| Setting        | Value      | Meaning                                              |
|----------------|------------|------------------------------------------------------|
| `type_field`   | `"type"`   | Wrapper field holding the routing discriminator       |
| `payload_field`| `"message"`| Wrapper field holding the actual event data           |

Each line is a wrapper object:

```json
{"type": "<routing-type>", "timestamp": "<ISO-8601>", "message": { ... payload ... }}
```

- `timestamp` lives on the **wrapper** (envelope) level, matching
  `timestamp = "^timestamp"` in the plugin (the `^` prefix means "extract from
  the envelope, not the payload").
- For `message`-type lines, the `message` payload carries `role` and `content`
  (content may be a string or an array of content blocks).
- For skip/meta lines the payload is never read — those lines are dropped by
  type routing before any payload extraction — so its contents are illustrative
  only. The real Pi session header, for example, carries `cwd` rather than a
  `message` field; line 1 mirrors that.

## Type Routing

`Envelope::get_routing` returns one of three actions. `jsonl.rs::parse_line`
acts on them as follows:

| Action  | Behavior in `parse_line`                                   |
|---------|------------------------------------------------------------|
| `event` | Unwrap `message` payload → produce an `Event`             |
| `skip`  | Return early → **no event** (line dropped)                |
| `meta`  | Return early → **no event** (metadata only, not a message)|

In the current implementation both `skip` and `meta` short-circuit to
`Ok(Vec::new())`, so neither produces events; the distinction is conceptual
(`skip` = ignore, `meta` = session metadata that is not a conversational turn).
Unknown types default to `skip`.

> **Note on `pi.toml`:** in the real Pi config both `compaction` and
> `session_info` are routed to `"skip"` (not `"meta"`). They are listed under
> "meta-type" below to group the *kinds* of non-message lines the task asked
> for; against `pi.toml` they are configured as `skip`. Either way the parse
> outcome is identical: **no event**.

## Fixture: `envelope-routing.jsonl`

Nine lines covering every routing branch. Expected total output when parsed
with the Pi plugin: **4 events** (the four `message` lines), with the other
five lines dropped.

| Line | `type`         | Routing category | Payload role | Expected parse outcome                                  |
|------|----------------|------------------|--------------|---------------------------------------------------------|
| 1    | `session`      | skip             | —            | No event — session header dropped by routing            |
| 2    | `session_info` | meta             | —            | No event — session metadata, not a conversational turn  |
| 3    | `message`      | event            | `user`       | 1 event — user message ("What files are in this directory?") |
| 4    | `model_change` | skip             | —            | No event — model switch dropped by routing              |
| 5    | `message`      | event            | `assistant`  | 1 event — assistant turn incl. a `bash` tool call (array content) |
| 6    | `message`      | event            | `toolResult` | 1 event — tool result, mapped to role `tool_result` via `role_map` |
| 7    | `message`      | event            | `assistant`  | 1 event — plain-text assistant reply (string content)   |
| 8    | `compaction`   | meta             | —            | No event — compaction summary, not a conversational turn|
| 9    | `custom`       | skip             | —            | No event — extension data dropped by routing            |

### Coverage checklist

- **event-type (4):** ≥1 user, ≥1 assistant, ≥1 tool_result — ✅
  (user ×1, assistant ×2, toolResult ×1)
- **skip-type (3):** `session`, `model_change`, `custom` — ✅
- **meta-type (2):** `session_info`, `compaction` — ✅
- **Content forms:** both string content (line 7) and array-of-blocks content
  (lines 5, 6) are represented.

### Notes for consumers

- This file is a hand-authored control fixture; it is not a captured real
  session.
- Line 6 uses role `toolResult`, which `pi.toml`'s `[parser.role_map]` remaps
  to `tool_result`. Assert against the remapped role.
- The wrapper-level `timestamp` (not the inner `message.timestamp`) is the
  authoritative event timestamp under `timestamp = "^timestamp"`.
