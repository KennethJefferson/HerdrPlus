# Pane Communication Layer ("msg") — Design Spec

Date: 2026-07-18 (rev 2 — post Sol56/gpt-5.6-sol design review; all 5 blockers and 10 major findings addressed)
Status: awaiting user approval
Feature: universal omni-directional pane-to-pane messaging for HerdrPlus

## Problem

herdr's only inter-pane channel is keystroke injection (`pane send-text`/`send-keys`/`run`) plus screen-scraping (`pane read`). That supports top-down orchestration but not clean agent-to-agent conversation: injected text corrupts the target's terminal input, replies require scraping, and there is no history, structure, or stable addressing.

Goal: any pane messages any pane — external orchestrator → panes, in-herdr orchestrator pane → workers, worker ↔ worker, across workspaces — without ever touching the target's terminal.

## Decisions

1. **Delivery: server-side inboxes + TUI unread badge.** Messages land in a per-pane queue inside the herdr server; they never touch any PTY. Panes with unread messages show a visible unread indicator on their border (count beside the label). The PTY "nudge" from rev 1 is REMOVED — review demonstrated raw-byte injection cannot be made inert (readline state, vim/TUI targets act on bytes immediately) and unsanitized labels in nudge text form a terminal-escape injection vector.
2. **Addressing: manual pane labels are the addresses, plus `@`-namespaced groups.** `pane rename` (the `manual_label`) is the addressable identity. Caveat (review): the border may *display* a dynamic metadata title or auto agent label in preference to `manual_label` (terminal/state.rs:1339 precedence) — the manual label remains the address regardless of what's displayed; `msg who` is the authoritative directory. Groups and broadcast use a distinct namespace: `@devs`, `@all` — bare names never collide with builtins.
3. **Collisions: scoped resolution, refuse ambiguity** (unchanged from rev 1, tightened): unqualified labels resolve sender-workspace-first, then globally; >1 match in the searched scope → error listing qualified candidates. Qualified form uses **workspace ids only** (`w1/worker-1`) — workspace display names are unstable (CWD-derived, duplicable; workspace.rs:145) and are not addressable.

## Architecture: server-native message bus

New `msg.*` methods in the existing socket API; inbox state lives in server state beside workspaces/panes. All topologies (external CLI, orchestrator pane, workers) use the same socket path. Waiting uses the **existing subscription machinery** (`events.subscribe` / `ActiveSubscription`, subscriptions.rs:295) — a new `MsgReceived` subscription event; no client-side polling loops (rev 1's polling design rejected by review as redundant with existing server-held wait support).

## Sender identity

- Every `msg.*` request carries explicit identity: `sender_pane_id: Option<String>` on send; `pane_id` on list/ack/group ops. The server cannot see callers' env vars — the CLI reads `HERDR_PANE_ID` itself and passes it, same pattern as `caller_pane_id` in `pane.current` (schema/panes.rs:225, cli/pane.rs:98).
- No `sender_pane_id` → sender is `external`. Identity is **claimed, not authenticated**: the local socket is a single trust domain and any client may assert any pane id. Documented as a non-goal (same trust model as every existing pane.* mutation).

## Message model

```
{ seq: u64, from_pane_id: string|null, from_workspace_id: string|null,
  from_label: string, to: string, body: string, timestamp: string }
```

- `seq`: per-inbox monotonic u64 starting at 1; doubles as the message id within an inbox.
- `from_pane_id`/`from_workspace_id`: immutable routing identity captured at send time (null for external) — replies route by pane id even if the sender is later renamed (review: presentation-only `from` made replies unroutable after rename).
- `from_label`: sender's manual label at send time (presentation only), or `external`.
- `to`: the target expression as typed. `body`: plain text, uninterpreted.
- `timestamp`: RFC 3339 UTC, server clock.

## Inboxes

- Per-pane, in-memory, created lazily, destroyed on pane close. Caps: **500 messages AND 4 MiB total body bytes** per inbox (review: count-only caps are insufficient — the socket accepts 1 MiB request lines); oldest dropped first; a per-inbox `dropped: u64` counter is exposed so consumers can detect loss.
- **Body size limit: 64 KiB per message** (structured error on violation).
- One ack cursor per inbox. `msg.list` is **pure** (never moves the cursor); `msg.ack { pane_id, up_to_seq }` advances it explicitly (review blocker: read-that-consumes loses messages when delivery fails after cursor advance, and is racy).
- Unread count = messages with `seq >` cursor; drives the TUI badge.
- Not persisted across server restarts (session restore brings back panes, not chat history).

## Resolution rules

1. Raw pane id (`w1:p3`) → direct.
2. `workspace_id/label` (`w1/worker-1`) → that workspace only; 0 matches → error; >1 → error listing pane ids.
3. Bare label → sender's workspace first (external senders skip this phase); exactly 1 → deliver; 0 → global phase; exactly 1 → deliver; >1 in the searched phase → error listing qualified candidates + pane ids. Never guess.
4. `@<group>` → fan out to member inboxes; empty/unknown group → error.
5. `@all` → every pane inbox except the sender's own.
6. Matching is exact and case-sensitive against `manual_label`. Labels that are empty, contain `/`, or begin with `@` are legal cosmetically but unaddressable; `msg.send` to such an expression returns a structured error explaining why.

## API surface

- `msg.send { target, body, sender_pane_id? }` → `{ delivered_to: [pane_id...], message }`
- `msg.list { pane_id, after_seq?, include_read?: bool }` → `{ messages, unread, dropped, ack_seq }` (read-only)
- `msg.ack { pane_id, up_to_seq }` → `{ ack_seq }`
- `msg.group_join { pane_id, group }` / `msg.group_leave { pane_id, group }` → `{ groups }`
- `msg.who {}` → `{ panes: [{ pane_id, workspace_id, label, groups, unread }], groups: [...] }`
- New subscription event `MsgReceived { pane_id, seq }` (ids only — the global EventHub ring holds 512 events; bodies stay in inboxes) delivered through the existing subscription stream; `msg wait` = subscribe + filter + fetch via `msg.list`.
- **Redraw classification** (review: `request_changes_ui` is a renderer redraw hint, not a mutation taxonomy — runtime.rs:84): `msg.send` and `msg.ack` ARE redraw-triggering (badge appears/clears); `msg.list`/`msg.who`/group ops are not (group membership has no v1 UI).
- No runtime-mutation wrappers in v1 (review: premature — no TUI code path calls these methods; the badge is render-side state, not an action).

## Protocol / compatibility

- `PROTOCOL_VERSION` 17 → 18 (project precedent: strict-equality version gate, bumped for `pane.move` and `layout.balance`; a capability-flag scheme was considered per review and rejected for v1 to stay consistent with upstream's versioning policy — noted as a future discussion if bump fatigue sets in).
- Bump checklist (supersedes the layout.balance recipe): `protocol/wire.rs:16`, schema artifact regen via `HERDR_UPDATE_API_SCHEMA=1`, `tests/api_ping.rs:307`, `tests/cli_wrapper.rs` literal sites, **and `tests/support/mod.rs:18`** (hardcodes the version and feeds the headless-handshake tests; missed by the previous checklist — review). Gate per AGENTS.md:185: confirm current source is not already ahead of the latest released tag before bumping.

## TUI unread badge

- Panes with `unread > 0` render an indicator appended to the border title (e.g. `─ worker-1 ✉ 3 ─`), via the existing `pane_border_title` path (ui/panes.rs:25, composed at ui/panes.rs:623). Truncation follows existing border-title rules; the badge attaches to whatever title the precedence chain displays.
- Badge state reads directly from server-side inbox state during render (same process); appears on `msg.send`, clears on `msg.ack` (both redraw-triggering).

## CLI surface

```
herdrplus msg send <target> <text>
herdrplus msg read [--all] [--after SEQ] [--pane ID]     # list + auto-ack displayed messages
herdrplus msg peek [--all] [--after SEQ] [--pane ID]     # list only, never acks
herdrplus msg ack <up-to-seq> [--pane ID]
herdrplus msg wait [--timeout MS] [--pane ID]            # subscription-based block, then prints new messages (no ack)
herdrplus msg group join|leave <name> [--pane ID]
herdrplus msg who
```

- `msg read` composes pure `msg.list` + `msg.ack` client-side (ack only after messages are successfully printed — the API stays loss-proof; the convenience lives in the CLI).
- `--pane` defaults from `HERDR_PANE_ID`; absent both → error for read/peek/ack/wait/group.
- `msg send` inside a pane passes the env pane id as `sender_pane_id` automatically.

## Group lifecycle

- Groups are in-memory name → member-pane-id sets; created on first join, removed when the last member leaves or closes; membership is not persisted across server restarts. Group names: nonempty, no `/`, no leading `@` (the `@` is the address-form sigil, not part of the name).

## Error handling

- Unknown target / unknown or empty group / ambiguous label / unaddressable label → structured errors with candidate lists where applicable.
- Sending to a closed pane id → `pane_not_found`. Body over 64 KiB → `message_too_large`.
- Overflow → oldest dropped, `dropped` counter incremented (visible in `msg.list`/`msg.who`).
- `msg.ack` with `up_to_seq` beyond newest → clamps to newest; behind current cursor → no-op success.

## Out of scope (v1)

- PTY nudge (removed — unsafe by construction)
- Message persistence, delivery receipts, threading, body schemas
- Cross-machine routing beyond what remote attach already provides
- Group UI, per-group badges
- Authenticated sender identity

## Testing

- Unit: resolution rules 1–6 (local-first, global fallback, ambiguity, qualified-by-workspace-id, external-global, `@` namespace, unaddressable labels), inbox caps (count AND bytes, dropped counter), ack cursor semantics (pure list, explicit ack, clamp/no-op), group lifecycle, `@all` excludes sender, pane-id non-reuse assumption (explicit test).
- Handler: send→list→ack round trip; redraw classification (send/ack yes, list/who no); closed-pane and oversized-body errors; `MsgReceived` event emission (ids only).
- Schema/protocol: serde round-trips, artifact regen, version fixtures incl. `tests/support/mod.rs`.
- CLI: parser tests per subcommand; `read` ack-after-print ordering; `wait` timeout.
- E2E: orchestrator + worker-1 + worker-2 (labeled panes) messaging in all directions; external CLI → `@devs`; cross-workspace ambiguity error; badge appears/clears in border title (assert via `pane read` of border row or layout title API); `wait` unblocks on send.

## Review provenance

Rev 2 incorporates an external design review by GPT-5.6 Sol (high reasoning, 270k tokens, source-verified): 5 blockers (sender identity in schema, nudge removal, label injection vector, pure-list/explicit-ack split, `tests/support/mod.rs` fixture), 10 major (subscription-based wait, redraw-hint semantics, label/display precedence, workspace-id qualification, `@` namespace, immutable from-ids, byte caps, protocol-bump policy, AGENTS.md release-tag gate, wrapper removal), 5 minor (seq/id/timestamp formats, group lifecycle, pane-id reuse test). The user approved replacing the nudge with the TUI unread badge.
