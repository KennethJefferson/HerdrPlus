# Pane Balance — Design Spec

Date: 2026-07-17 (rev 2, post Sol56/gpt-5.6-sol review)
Branch: `feature/pane-balance`
Status: design approved by Tony; rev 2 incorporates external review findings

## Problem

herdr has no way to evenly distribute pane sizes in a tab. tmux has layout balancing (`prefix+Space` / `select-layout tiled`); psmux exposes the same. In herdr, even layouts are only achievable by choosing exact split ratios in the right order at creation time, and drift from manual resizing is permanent. Verified absent across CLI (`pane`/`tab` subcommands), the API schema (`herdr-api.schema.json`), and TUI actions.

## Decision summary

- **Semantics: equal ideal area, keep tree.** Rewrite every split's ratio so all leaf panes get equal *ideal (pre-rounding) outer layout area*. The split tree structure is preserved — panes never move or restructure. Full grid re-tiling is out of scope for v1.
- **Surface: CLI + socket API + TUI keybinding.**
- **Architecture: server-side API method**, mirroring `layout.set_split_ratio`. Client-side composition was rejected: non-atomic, N redraw events, unusable from the TUI action.
- **Wire namespace: `layout.balance`** (not `pane.balance`) — this mutates a whole layout tree and belongs beside `layout.export`/`layout.apply`/`layout.set_split_ratio` in the `Method` enum. The CLI surface remains `herdr pane balance` (user-facing wording need not match wire namespace).

## Algorithm

New method on `TileLayout` (`src/layout.rs`):

```
balance() -> bool  // true if any ratio changed
```

Single post-order traversal of `Node` that returns subtree leaf counts bottom-up (avoids quadratic re-counting on deep chains). For every `Node::Split { ratio, first, second, .. }`:

```
ratio = leaf_count(first) / (leaf_count(first) + leaf_count(second))
```

clamped to the existing `[0.1, 0.9]` bounds via `valid_split_ratio`. `changed` is computed by diffing `split_ratios()` before/after, same as `resize_pane`.

### Equality caveats (explicit, verified in review)

- **"Equal area" means ideal continuous area before integer rounding.** Actual cell areas differ because `split_rect` rounds per node (`.round()` at layout.rs:624/632). Example: 10×5 area, tree A|(B/C) → ideal 33.3% each, actual cells 30%/42%/28%. Re-balancing cannot fix this; it is a rendering artifact, not a convergence issue. Pane chrome (`apply_pane_chrome`, ui/panes.rs:90) can shrink visible content further and asymmetrically.
- **Clamp breaks equality for lopsided trees**: 1 leaf vs balanced-10-leaf sibling wants ratio 1/11 ≈ 0.0909 → clamped to 0.1 (lone pane 10%, others 9% each). A same-direction chain of 10 leaves lands exactly at the clamp; 11 is the first chain actually distorted by it.
- **Idempotent at the ratio level**: second call recomputes identical values → `changed: false`. Note `changed: false` does NOT imply visibly equal cells (rounding, above).

## API

- `Method::LayoutBalance(LayoutBalanceParams)`, wire name `"layout.balance"` (`src/api/schema.rs`, beside the other `layout.*` methods).
- `LayoutBalanceParams { tab_id: Option<TabId>, pane_id: Option<PaneId> }` (`src/api/schema/panes.rs`). Resolution via `resolve_layout_export_target` (`app/api/layouts.rs:251`): **`tab_id` and `pane_id` are mutually exclusive** — both present is an error (same policy as the resolver: `None` → invalid-target error), neither present → active tab. Invalid explicit IDs error (no silent active-tab fallback). Error variants mirror `layout.set_split_ratio`'s existing paths.
- Response: `ResponseResult::LayoutBalanced { layout, changed }` where `layout` is the tree-oriented `LayoutDescription` (same shape as `LayoutSplitRatioSet`, response.rs:137) — NOT the geometric `PaneLayoutSnapshot` that `pane.layout` returns. Documented to avoid conflation.
- Handler `handle_layout_balance` in `app/api/layouts.rs` (next to `handle_layout_set_split_ratio`). **Pinned operation order:** resolve tab → `tab.layout.balance()` → snapshot response payload → if `changed`: `schedule_session_save()` + `emit_layout_updated_event` → respond. Save and event are **gated on `changed`** (no churn for single-pane/already-balanced tabs). Note: the `layout.updated` event carries a geometric `PaneLayoutSnapshot` (app/api/panes.rs:1734) and may reach subscribers before the requester's response; payload shapes differ by design.
- **Zoomed tabs:** balance mutates the underlying tree without unzooming. A user watching a zoomed pane sees no visible change until unzoom — expected behavior, not a bug (response still carries `zoomed` + full tree).
- Registration points (mirror `layout.set_split_ratio`): `api/schema.rs`, `api/schema/panes.rs`, `api/schema/response.rs`, `app/api.rs` dispatch, `api/server.rs` method-name map, `api/mod.rs` mutation classification, `app/runtime_mutations.rs` in-process dispatch.
- **Protocol: bump `PROTOCOL_VERSION` 16 → 17** (`protocol/wire.rs:16`) and regenerate the schema artifact (`HERDR_UPDATE_API_SCHEMA=1`). Additional hardcoded version expectations to update: `tests/api_ping.rs:307` and `tests/cli_wrapper.rs` (~1705–1779). CLI↔server compatibility is exact-match (`ensure_server_protocol_compatible`, cli.rs:1009); precedent for feature bumps exists (`pane.move`).

## CLI

```
herdr pane balance [--tab ID | --pane ID | --current]
```

- No flags → active tab.
- `--tab` and `--pane` are mutually exclusive; parser rejects both together.
- `--current` resolves the invoking pane via `HERDR_PANE_ID` (same mechanism as `pane split --current`) and sends it as `pane_id` — balances the tab containing the shell that ran the command.
- Wiring: match arm in `run_pane_command` (`cli/pane.rs`), usage line in `print_pane_help`, subcommand in `pane_command()` (`cli/spec.rs`), send via the `print_method_response` pattern (`cli/runtime.rs`).
- Scripting note: when precision matters under concurrent clients, pass explicit `--tab`/`--pane` — no-target resolution races against focus changes by other clients (resolution happens server-side at execution time; mutations themselves are serialized by the app event loop, app/mod.rs:1081).

## TUI keybinding

- New `balance_panes: ActionKeybinds` in `config/keybinds.rs` following the `split_vertical` pattern, **plus** the full plumbing review identified: config model field + overlay-application + effective-profile serialization (`config/model.rs:585,:680`) so remote/SSH attaches with local keybindings don't silently drop the binding (`server/headless.rs:991`); NavigateAction variant + lookup/execution + help screen (`app/input/navigate.rs:1296,:1424,:179`; `ui/keybind_help.rs:139`); the exhaustive test-only action-executor match (`navigate.rs:~1542`).
- Default binding: `prefix+=`. Rationale: **Vim muscle memory** (`Ctrl-W =` equalizes windows) and `=` as an "equalize" mnemonic. (Corrected from rev 1: tmux's `prefix+=` is the paste-buffer chooser; tmux balances via `prefix+Space`/`M-1..7`.) Verified free: no default claims `=` (model.rs:920), it parses via `parse_key_combo` (keybinds.rs:1206), and existing `BindingRegistry` safeguards handle prefix-reservation and user-override displacement correctly.
- The bound action invokes the same balance path as the API handler (via runtime mutation dispatch).

## Error handling

- Unknown/invalid tab or pane id, or both targets set → existing resolver error responses (no fallback).
- Single-pane tab → success, `changed: false`, no save/event. ("Empty tab" cannot occur: `TileLayout` always has a root and `close_focused` refuses to remove the last pane.)
- Extreme trees degrade via the ratio clamp exactly like manual resizing.

## Testing

1. **Unit (`layout.rs`)**: 2-pane → 0.5; 3-pane asymmetric A|(B/C) → root 1/3; mixed-direction trees; 10-leaf chain (at clamp) vs 11-leaf chain (first clamped); balanced tree → `changed: false`; single pane → `changed: false`.
2. **Handler**: no-op suppresses save/event; both-targets-set error; invalid-ID error; zoomed tab balances underlying tree.
3. **Schema/protocol**: regenerate artifact; `generated_protocol_schema_artifact_is_current`; request/response serde round-trip; update hardcoded versions in `tests/api_ping.rs` + `tests/cli_wrapper.rs`.
4. **CLI**: target-flag parsing incl. mutual exclusion and `--current` via `HERDR_PANE_ID`.
5. **Keybinding**: custom-binding displacement of the default; remote/local keybinding profile serialization includes `balance_panes`.
6. **E2E (manual)**: uneven 3-pane layout → `herdr pane balance` → assert near-equal areas via `pane layout` **using clamp-free trees and a ±1-cell-per-axis rounding tolerance** (or terminal dims divisible by the leaf counts); session save/restore round-trip preserves balanced ratios; TUI binding matches CLI behavior.

## Out of scope (v1)

- Grid re-tiling / tree restructuring (`--tile`)
- Subtree-only balancing (always whole tab)
- Making rendered cell areas exactly equal (rounding is a renderer property)
- Animation of ratio changes

## Review provenance

Rev 2 incorporates the full findings of an external review by GPT-5.6 Sol (high reasoning) over the spec and 18 source files; all findings were verified against source cites and accepted. Key corrections: equality caveats made explicit, wire method renamed to `layout.balance`, save/event gated on `changed`, target mutual-exclusion policy, `--current` semantics, remote-keybinding threading, protocol-bump test fixtures, tmux-rationale correction.
