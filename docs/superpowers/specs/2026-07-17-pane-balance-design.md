# Pane Balance — Design Spec

Date: 2026-07-17
Branch: `feature/pane-balance`
Status: approved by Tony (design review in session)

## Problem

herdr has no way to evenly distribute pane sizes in a tab. tmux has `select-layout tiled`; psmux exposes the same. In herdr, even layouts are only achievable by choosing exact split ratios in the right order at creation time, and drift from manual resizing is permanent. Verified absent across CLI (`pane`/`tab` subcommands), the API schema (`herdr-api.schema.json`), and TUI actions.

## Decision summary

- **Semantics: equal area, keep tree.** Rewrite every split's ratio so all leaf panes get equal area. The split tree structure is preserved — panes never move or restructure. Full grid re-tiling (tmux `tiled`) is explicitly out of scope for v1.
- **Surface: CLI + socket API + TUI keybinding.**
- **Architecture: server-side API method** (`pane.balance`), mirroring `layout.set_split_ratio`. Client-side composition of existing APIs was rejected: non-atomic, N redraw events, unusable from the TUI action.

## Algorithm

New method on `TileLayout` (`src/layout.rs`):

```
balance() -> bool  // true if any ratio changed
```

Recursive walk of `Node`: for every `Node::Split { ratio, first, second, .. }`:

```
ratio = leaf_count(first) / (leaf_count(first) + leaf_count(second))
```

clamped to the existing `[0.1, 0.9]` bounds via `valid_split_ratio`, matching manual-resize behavior (deep same-direction chains degrade gracefully). `changed` is computed by diffing `split_ratios()` before/after, same as `resize_pane`.

Properties:
- Pure tree mutation; unit-testable without a running server.
- Equal *areas* guaranteed (within clamp + integer cell rounding); shapes follow the tree.
- Idempotent: balancing a balanced tree returns `changed: false`.

## API

- `Method::PaneBalance(PaneBalanceParams)`, wire name `"pane.balance"` (`src/api/schema.rs`).
- `PaneBalanceParams { tab_id: Option<TabId>, pane_id: Option<PaneId> }` (`src/api/schema/panes.rs`). Target resolution reuses `resolve_layout_export_target` (`app/api/layouts.rs`): explicit tab → tab containing pane → active tab.
- Handler `handle_pane_balance` in `app/api/layouts.rs` (it is a layout-wide operation; lives next to `handle_layout_set_split_ratio`): resolve tab → `tab.layout.balance()` → `schedule_session_save()` → `emit_layout_updated_event` → respond.
- Response: new `ResponseResult::PaneBalanced { layout, changed }` variant carrying the resulting layout payload (same layout shape as `LayoutSplitRatioSet`) plus the `changed` flag.
- Registration points (mirror `layout.set_split_ratio` wiring): `api/schema.rs`, `api/schema/panes.rs`, `api/schema/response.rs`, `app/api.rs` dispatch, `api/server.rs` method-name map, `api/mod.rs` mutation classification, `app/runtime_mutations.rs` in-process dispatch.
- **Protocol: bump `PROTOCOL_VERSION` 16 → 17** (`protocol/wire.rs`) and regenerate the schema artifact (`HERDR_UPDATE_API_SCHEMA=1`, enforced by `generated_protocol_schema_artifact_is_current`).

## CLI

```
herdr pane balance [--tab ID | --pane ID | --current]
```

Default: active tab. Wiring: match arm in `run_pane_command` (`cli/pane.rs`), usage line in `print_pane_help`, subcommand in `pane_command()` (`cli/spec.rs`), request send via `print_method_response` (`cli/runtime.rs` pattern).

## TUI keybinding

- New `balance_panes: ActionKeybinds` field in `config/keybinds.rs`, following the `split_vertical`/`split_horizontal` pattern (default set, `empty_action!` init, `apply_action!` merge, config model field in `config/model.rs`).
- Default binding: `prefix+=` (tmux muscle memory).
- The bound action invokes the same balance path the API handler uses (via the runtime mutation dispatch), so behavior is identical to the CLI.

## Error handling

- Unknown tab/pane id → existing API error responses from `resolve_layout_export_target`.
- Single-pane or empty tab → success with `changed: false`, no event churn beyond the normal path.
- Ratio clamp means extreme trees (10+ leaf chain in one direction) come out as even as the existing engine allows — identical limits to manual resizing.

## Testing

1. **Unit (`layout.rs` tests)**: 2-pane 0.5; 3-pane asymmetric tree A|(B/C) → root 1/3 (per preview mockup); deep same-direction chain hits 0.1 clamp; balanced tree → `changed: false`.
2. **Schema**: regenerate artifact; `generated_protocol_schema_artifact_is_current` passes.
3. **E2E (manual, CLI)**: build uneven 3-pane layout, run `herdr pane balance`, assert equal areas via `pane layout` rects; verify TUI keybinding does the same.

## Out of scope (v1)

- Grid re-tiling / tree restructuring (`--tile`)
- Balancing a subtree only (balance always targets a whole tab)
- Animation/transition of ratio changes
