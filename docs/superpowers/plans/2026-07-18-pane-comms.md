# Pane Communication Layer Implementation Plan

> **For agentic workers:** This plan is executed by three CLI dev agents (dev-1-grok, dev-2-grok, dev-3-agy) working in herdrplus panes on the same checkout, orchestrated externally. Each agent owns ONE workstream and must not modify files owned by another workstream. Steps use checkbox (`- [ ]`) syntax.

**Goal:** Server-native message bus: per-pane inboxes, label-based addressing, subscription-backed wait, TUI unread badge, `herdrplus msg` CLI.

**Architecture:** New `MsgBus` state module owned by the server; `msg.*` socket-API methods mirroring the `layout.balance` registration pattern (in-tree worked example); `MsgReceived` subscription event through the existing subscription machinery; badge rendered via the existing `pane_border_title` path.

**Tech Stack:** Rust 1.96.1 (pinned), serde/schemars wire schema, ratatui TUI.

**Authoritative spec:** `docs/superpowers/specs/2026-07-18-pane-comms-design.md` (rev 2). Where this plan and the spec disagree, the spec wins — flag the conflict to the orchestrator.

## Global Constraints

- Branch: `feature/pane-comms`. Repo root: `K:\Downloads\__Projects.Mine\herdr4Windows`; all source under `__references/herdr/`. Build/test from `__references/herdr`: `cargo build --release`, `cargo test --bin herdrplus <filter>`. NO lib target — never `cargo test --lib`.
- NEVER set `HERDR_UPDATE_API_SCHEMA` except in Workstream 2's single regen step (it invalidates the build script → ~5-10 min rebuild).
- Every commit message ends with: `Co-Authored-By: <your CLI name> <noreply@x.ai or noreply@google.com>` — use your actual identity (Grok / Antigravity), NOT Claude.
- Stale `K:\...\herdr4Windows\.git\index.lock` with no live git process → delete it, retry.
- Known pre-existing test failures on this Windows box (NOT yours; do not chase): `integration::tests` cascade (kimi asset cfg bug poisons a shared env mutex), `app::worktrees`, `app::api::worktrees`, `detect::manifest_update`, `pane_graphics_stream`. Judge your work by targeted module runs.
- Exact values from the spec (verbatim): inbox caps **500 messages AND 4 MiB** total body bytes; body limit **64 KiB** (`message_too_large`); seq starts at 1 per inbox; timestamps RFC 3339 UTC; qualified addresses are `workspace_id/label` (e.g. `w1/worker-1`); groups/broadcast are `@group` / `@all`; labels that are empty, contain `/`, or start with `@` are unaddressable; `PROTOCOL_VERSION` 17 → **18**.
- Coordination: WS1 lands first (core module), WS2 second (API), WS3 last (CLI+badge). WS2/WS3 may scaffold early but their commits go in after their dependency's interfaces compile. When your workstream is complete and its tests pass, print exactly `WS<n>-DONE` in your terminal.

---

## File ownership map

| Workstream | Owner | Files |
|---|---|---|
| WS1 core bus | dev-1-grok | Create `src/msg.rs`; one line `mod msg;` in `src/main.rs` |
| WS2 API layer | dev-2-grok | `src/api/schema.rs`, `src/api/schema/panes.rs` (or new `schema/msg.rs` + mod line), `src/api/schema/response.rs`, `src/api/schema/tests.rs`, `src/app/api.rs`, new `src/app/api/msg.rs` (+ mod line in `src/app/api.rs`), `src/api/server.rs`, `src/api/mod.rs`, subscription files, `src/protocol/wire.rs`, `tests/api_ping.rs`, `tests/cli_wrapper.rs`, `tests/support/mod.rs`, `docs/next/api/herdr-api.schema.json` |
| WS3 CLI + badge | dev-3-agy | New `src/cli/msg.rs` (+ dispatch in `src/cli.rs`, spec in `src/cli/spec.rs`), `src/ui/panes.rs`, repo-root `Usage.md`, `Changelog.md` |

---

### Workstream 1 (dev-1-grok): core `MsgBus` module

**Files:** Create `src/msg.rs`; add `mod msg;` to `src/main.rs` beside the other module declarations. Tests inline (`#[cfg(test)]`).

**Produces (contract for WS2 — exact signatures):**

```rust
pub struct MsgBus { /* private */ }
pub struct StoredMsg {
    pub seq: u64,
    pub from_pane_id: Option<String>,
    pub from_workspace_id: Option<String>,
    pub from_label: String,
    pub to: String,
    pub body: String,
    pub timestamp: String, // RFC 3339 UTC, caller-supplied
}
pub enum MsgTarget { PaneId(String), Qualified { workspace_id: String, label: String }, Label(String), Group(String), All }
pub enum ResolveError { NotFound, Ambiguous(Vec<(String, String, String)>), // (workspace_id, label, pane_id)
    Unaddressable(String), EmptyGroup(String) }
impl MsgBus {
    pub fn new() -> Self;
    pub fn parse_target(expr: &str) -> Result<MsgTarget, ResolveError>; // "@x"->Group/All, "id with ':'"->PaneId, "ws/label"->Qualified, else Label
    pub fn resolve(&self, target: &MsgTarget, sender_workspace: Option<&str>,
                   directory: &[(String, String, String)]) -> Result<Vec<String>, ResolveError>;
                   // directory entries: (workspace_id, pane_id, manual_label); returns pane_ids per spec rules 1-6
    pub fn deliver(&mut self, pane_id: &str, msg: StoredMsg) -> Result<(), MsgError>; // enforces 64KiB body, 500/4MiB caps (drop oldest, bump dropped)
    pub fn list(&self, pane_id: &str, after_seq: Option<u64>, include_read: bool) -> (Vec<StoredMsg>, u64 /*unread*/, u64 /*dropped*/, u64 /*ack_seq*/);
    pub fn ack(&mut self, pane_id: &str, up_to_seq: u64) -> u64; // clamps to newest; behind-cursor = no-op; returns new ack_seq
    pub fn unread(&self, pane_id: &str) -> u64; // 0 for unknown pane (WS3 badge reads this)
    pub fn next_seq(&self, pane_id: &str) -> u64; // seq the NEXT delivered message will get (WS2 stamps StoredMsg.seq via deliver; deliver assigns seq internally — StoredMsg.seq passed in is ignored/overwritten)
    pub fn group_join(&mut self, pane_id: &str, group: &str) -> Result<Vec<String>, MsgError>;  // validates name; returns pane's groups
    pub fn group_leave(&mut self, pane_id: &str, group: &str) -> Result<Vec<String>, MsgError>;
    pub fn groups_of(&self, pane_id: &str) -> Vec<String>;
    pub fn group_members(&self, group: &str) -> Vec<String>;
    pub fn remove_pane(&mut self, pane_id: &str); // inbox + group membership teardown (WS2 hooks pane close)
}
pub enum MsgError { MessageTooLarge, InvalidGroupName(String) }
```

- [ ] TDD: write failing unit tests first covering — `parse_target` for all five forms + unaddressable labels (empty, `/`, leading `@`); resolution rules 1–6 from the spec against a fake directory (local-first, global fallback, ambiguity error with candidates, external sender = no local phase via `sender_workspace: None`, `@all` excludes sender only at WS2 level — `resolve` for All returns every directory pane id and WS2 filters the sender, OR resolve takes sender_pane_id — pick one, document in code, test it); deliver/list/ack (seq from 1, pure list, cursor clamp/no-op, unread math); caps (501st message drops oldest and bumps `dropped`; 4 MiB byte cap; 64 KiB reject); group lifecycle (join/leave/auto-remove-on-empty, invalid names); `remove_pane` clears membership.
- [ ] Implement minimally; match the file style of `src/layout.rs` (self-contained module + tests at bottom).
- [ ] `cargo test --bin herdrplus msg::` green; commit (`feat: add MsgBus core module`).
- [ ] Print `WS1-DONE`.

---

### Workstream 2 (dev-2-grok): API layer, subscription event, protocol 18

**Consumes:** WS1's `MsgBus` contract above (compiles after WS1's commit).
**Produces (contract for WS3):** wire methods `msg.send`, `msg.list`, `msg.ack`, `msg.group_join`, `msg.group_leave`, `msg.who` with params/response shapes exactly as spec §API surface; subscription event `MsgReceived { pane_id, seq }`; `AppState`-reachable `msg_bus: MsgBus` plus an unread accessor WS3's renderer can call: `app.state.msg_bus.unread(pane_id)`.

- [ ] **Study the in-tree worked example first**: `layout.balance` (commit `ccb1a25`) shows every registration hop — `src/api/schema.rs` Method variant, params in `schema/panes.rs`, response variant, handler in `app/api/layouts.rs`, dispatch arm in `app/api.rs`, name map in `api/server.rs`, `request_changes_ui` in `api/mod.rs`, schema tests. Mirror it for six `msg.*` methods (params/response per spec §API surface; a new `src/app/api/msg.rs` handler module keeps `panes.rs` from growing).
- [ ] Add `msg_bus: MsgBus` to the server state struct (find the struct holding `workspaces` — the layouts handler reaches it as `self.state.workspaces`). Build the resolution directory from live state: iterate workspaces → panes → manual labels (find where `pane rename` stores `manual_label`, handler at `app/api/panes.rs:1139`). Hook `remove_pane` into pane-close teardown (find where panes leave state — trace `handle_pane_close`).
- [ ] `msg.send`: parse+resolve via WS1 (`sender_pane_id` → its workspace for local-first; None → external), enforce `@all`-excludes-sender, deliver to each target inbox, stamp timestamp (RFC 3339 UTC), emit `MsgReceived { pane_id, seq }` per delivery through the subscription/event machinery (study how existing subscription kinds are defined and delivered — `events.subscribe`-style Methods and `ActiveSubscription`; carry ids only, never bodies). Redraw classification: `msg.send` + `msg.ack` ARE in `request_changes_ui`; `msg.list`/`msg.who`/group ops are NOT.
- [ ] Errors: structured codes `pane_not_found`, `ambiguous_target` (with candidates in message), `unknown_target`, `empty_group`, `unaddressable_label`, `message_too_large`, `invalid_group_name`.
- [ ] TDD handler tests in `src/app/api/msg.rs` mirroring `app/api/layouts.rs`'s `layout_balance_*` test style (`app_with_workspace()`, direct handler calls, `event_hub.events_after`): send→list→ack round trip; ambiguity error; external-vs-pane sender resolution; `@all` excludes sender; MsgReceived emitted with ids only; redraw classification; oversized body.
- [ ] Serde round-trip tests for all six methods in `src/api/schema/tests.rs` (mirror `layout_balance_method_serde_round_trip`).
- [ ] Protocol: `PROTOCOL_VERSION` 17→18 in `src/protocol/wire.rs:16`; update literal fixtures `tests/api_ping.rs:307`, `tests/cli_wrapper.rs` (six literal sites ~1705–1779), **and `tests/support/mod.rs:18`**; verify per AGENTS.md:185 that source is ahead of the latest released tag (it is — protocol 17 is unreleased local work; state this in the commit body). Regenerate the schema artifact: bash, `export HERDR_UPDATE_API_SCHEMA=1` then `cargo test --bin herdrplus generated_protocol_schema_artifact_is_current` (expect a long rebuild), then unset and re-run the same test to confirm it passes clean.
- [ ] `cargo test --bin herdrplus msg` + `app::api::` + `api::schema::` green; commit per logical unit (`feat: add msg.* API methods`, `feat: bump protocol to 18 for msg bus`).
- [ ] Print `WS2-DONE`.

---

### Workstream 3 (dev-3-agy): CLI subcommands + TUI unread badge + docs

**Consumes:** WS2's wire methods and `unread()` accessor.

- [ ] **CLI** — new `src/cli/msg.rs` mirroring `src/cli/pane.rs`'s structure (hand-rolled arg loops, `super::runtime`-style dispatchers via `print_method_response`, help printer). Subcommands exactly: `send <target> <text>`, `read [--all] [--after SEQ] [--pane ID]`, `peek [--all] [--after SEQ] [--pane ID]`, `ack <up-to-seq> [--pane ID]`, `wait [--timeout MS] [--pane ID]`, `group join|leave <name> [--pane ID]`, `who`. Register `"msg"` in the top-level CLI dispatch (find where `"pane"` routes to `run_pane_command` in `src/cli.rs`) and add a `msg_command()` to `src/cli/spec.rs` mirroring `pane_command()`.
  - `--pane` defaults from `HERDR_PANE_ID` env (read client-side, pattern: `cli/pane.rs:98`); absent both → error exit 2 for read/peek/ack/wait/group. `send` passes env pane id as `sender_pane_id` when present.
  - `read` = `msg.list` then, after successfully printing, `msg.ack` up to the highest printed seq (ack-after-print; never ack on print failure). `peek` never acks.
  - `wait` = subscribe to `MsgReceived` for the pane via WS2's subscription method and block until an event or `--timeout MS`, then print new messages via `msg.list` (no ack). Follow how the existing `herdr wait` CLI consumes its blocking machinery (`src/cli/wait*.rs` or the wait dispatch in `src/cli.rs`).
  - TDD parser tests mirroring `cli/pane.rs`'s test module (env resolution, missing-value errors, timeout parsing, unknown flags).
- [ ] **Badge** — in `src/ui/panes.rs`, where the border title is composed (`~line 623`, `border_label(...)` → `pane_border_title(...)`): when `app.state.msg_bus.unread(pane_id) > 0`, append ` ✉ <n>` to the title string before truncation (unlabeled panes with unread get just `✉ <n>`). Add a rendering unit test if the file's existing tests support it (there is a test around line 1025 setting labels); otherwise cover via an `unread()`-driven title-builder function you extract and test.
- [ ] **Docs** — repo root: append Changelog `[0.3.0]` entry (Added: msg bus API/CLI/badge, protocol 18; append-only, new entry on top, date 2026-07-18) and add the msg commands to `Usage.md`.
- [ ] `cargo test --bin herdrplus cli::` + `ui::` green; commit (`feat: add herdrplus msg CLI and unread badge`).
- [ ] Print `WS3-DONE`.

---

### Orchestrator-run finale (not a pane agent)

- [ ] Full targeted suite sweep + release build (`cargo build --release` → `__solutions/target/release/herdrplus.exe`).
- [ ] Live e2e per spec §Testing: 3 labeled panes, all-directions messaging, `@devs` group send from external CLI, cross-workspace ambiguity error, badge appears/clears, `msg wait` unblocks on send.
- [ ] Reviews (internal + Sol56), PR to main.
