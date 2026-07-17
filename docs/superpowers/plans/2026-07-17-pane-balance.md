# Pane Balance Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a `layout.balance` API method + `herdr pane balance` CLI + `prefix+=` TUI keybinding that resets every split ratio in a tab so all panes get equal ideal area.

**Architecture:** A new `TileLayout::balance()` tree walk (post-order, leaf-count-weighted ratios, clamped [0.1,0.9]) exposed through the existing socket-API pipeline, mirroring `layout.set_split_ratio` at every hop. Save/event emission gated on `changed`. Protocol bump 16→17.

**Tech Stack:** Rust 1.96.1 (pinned), serde/schemars wire schema, ratatui TUI. Spec: `docs/superpowers/specs/2026-07-17-pane-balance-design.md` (rev 2).

## Global Constraints

- All source edits happen in `K:\Downloads\__Projects.Mine\herdr4Windows\__references\herdr\` (the vendored herdr tree). All paths below are relative to that directory unless prefixed with `HerdrPlus:` (= repo root `K:\Downloads\__Projects.Mine\herdr4Windows\`).
- Branch: `feature/pane-balance` (already checked out). Commit after every task; end every commit message with `Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>`.
- Build/test from inside `__references/herdr`. `cargo` works with zero env setup — `HerdrPlus:.cargo/config.toml` sets `ZIG`, `ZIG_GLOBAL_CACHE_DIR`, and redirects target-dir to `HerdrPlus:__solutions/target/`. Do NOT delete or bypass that config. First `cargo test` invocation compiles the vendored libghostty-vt with Zig — a one-time ~1–2 min cost per profile.
- Shell is PowerShell: set env vars as `$env:NAME="value"`, clear with `Remove-Item Env:NAME`.
- Wire method name is exactly `layout.balance`; CLI command is exactly `herdr pane balance`; keybinding default is exactly `prefix+=`; new protocol version is exactly `17`. Response variant is `LayoutBalanced { layout, changed }`.
- `tab_id`/`pane_id` are mutually exclusive everywhere (resolver returns error, CLI parser rejects). No silent fallback on invalid IDs.
- Session save + `layout.updated` event fire ONLY when `changed == true`.

---

### Task 1: `TileLayout::balance()` core algorithm

**Files:**
- Modify: `src/layout.rs` (add method near `set_ratio_at` at ~line 208; add free fn near `split_ratios` at ~line 478; add tests in the existing `#[cfg(test)] mod tests` at ~line 642)

**Interfaces:**
- Consumes: existing `Node`, `TileLayout`, `split_ratios(&Node) -> Vec<(Vec<bool>, f32)>`, `valid_split_ratio(f32) -> f32`.
- Produces: `pub fn TileLayout::balance(&mut self) -> bool` (true iff any ratio changed). Task 2's handler calls exactly this.

- [ ] **Step 1: Write the failing tests**

Append inside `mod tests` in `src/layout.rs` (match existing style: `pane(id)` helper, `TileLayout::new()`, `TileLayout::from_saved`, `split_snapshot`):

```rust
    #[test]
    fn balance_two_panes_resets_ratio_to_half() {
        let (mut layout, root) = TileLayout::new();
        layout.focus_pane(root);
        layout.split_focused_with_ratio(Direction::Horizontal, 0.3);

        let changed = layout.balance();

        assert!(changed);
        let splits = split_snapshot(&layout);
        assert_eq!(splits.len(), 1);
        assert!((splits[0].1 - 0.5).abs() < f32::EPSILON);
    }

    #[test]
    fn balance_asymmetric_three_pane_tree_weights_by_leaf_count() {
        // A | (B over C): root should get ratio 1/3 (1 leaf vs 2), inner 0.5.
        let mut layout = TileLayout::from_saved(
            Node::Split {
                direction: Direction::Horizontal,
                ratio: 0.5,
                first: Box::new(Node::Pane(pane(1))),
                second: Box::new(Node::Split {
                    direction: Direction::Vertical,
                    ratio: 0.7,
                    first: Box::new(Node::Pane(pane(2))),
                    second: Box::new(Node::Pane(pane(3))),
                }),
            },
            pane(1),
        );

        let changed = layout.balance();

        assert!(changed);
        let splits = split_snapshot(&layout);
        assert_eq!(splits.len(), 2);
        assert!((splits[0].1 - 1.0 / 3.0).abs() < 1e-6);
        assert!((splits[1].1 - 0.5).abs() < f32::EPSILON);
    }

    #[test]
    fn balance_mixed_direction_tree_from_sample_layout() {
        // sample_layout(): 4 leaves. Root: 1 vs 3 -> 0.25; middle: 1 vs 2 -> 1/3; inner: 0.5.
        let mut layout = sample_layout();

        let changed = layout.balance();

        assert!(changed);
        let splits = split_snapshot(&layout);
        assert_eq!(splits.len(), 3);
        assert!((splits[0].1 - 0.25).abs() < 1e-6);
        assert!((splits[1].1 - 1.0 / 3.0).abs() < 1e-6);
        assert!((splits[2].1 - 0.5).abs() < f32::EPSILON);
    }

    #[test]
    fn balance_eleven_leaf_chain_clamps_root_ratio() {
        // Right-deep chain of 11 leaves: ideal root ratio 1/11 (< 0.1) clamps to 0.1.
        let mut node = Node::Pane(pane(11));
        for id in (1..=10).rev() {
            node = Node::Split {
                direction: Direction::Horizontal,
                ratio: 0.5,
                first: Box::new(Node::Pane(pane(id))),
                second: Box::new(node),
            };
        }
        let mut layout = TileLayout::from_saved(node, pane(1));

        layout.balance();

        let splits = split_snapshot(&layout);
        assert_eq!(splits.len(), 10);
        assert!((splits[0].1 - 0.1).abs() < f32::EPSILON);
        // The 10-leaf subtree at depth 1 wants 1/10 = 0.1: exactly at the clamp, allowed.
        assert!((splits[1].1 - 0.1).abs() < 1e-6);
    }

    #[test]
    fn balance_is_idempotent_and_reports_unchanged_second_time() {
        let mut layout = sample_layout();
        assert!(layout.balance());
        assert!(!layout.balance());
    }

    #[test]
    fn balance_single_pane_reports_unchanged() {
        let (mut layout, _root) = TileLayout::new();
        assert!(!layout.balance());
    }
```

Note: if `split_snapshot` returns something other than `Vec<(Direction, f32)>` in pre-order (root, first-subtree, second-subtree), adapt the index assertions to its actual shape — check its definition in the same test module first.

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --lib balance` (from `__references/herdr`)
Expected: compile error `no method named `balance` found for struct `TileLayout``

- [ ] **Step 3: Implement**

In `src/layout.rs`, add to `impl TileLayout` (directly after `set_ratio_at`, ~line 211):

```rust
    /// Reset every split ratio so all leaf panes get equal ideal area
    /// (leaf-count weighted, clamped like manual resize). Returns true if
    /// any ratio changed.
    pub fn balance(&mut self) -> bool {
        let before = split_ratios(&self.root);
        balance_node(&mut self.root);
        split_ratios(&self.root) != before
    }
```

Add the free function directly after `split_ratios` (~line 502):

```rust
/// Post-order walk: set each split's ratio to first_leaves/total_leaves and
/// return this subtree's leaf count.
fn balance_node(node: &mut Node) -> usize {
    match node {
        Node::Pane(_) => 1,
        Node::Split {
            ratio,
            first,
            second,
            ..
        } => {
            let first_leaves = balance_node(first);
            let second_leaves = balance_node(second);
            *ratio =
                valid_split_ratio(first_leaves as f32 / (first_leaves + second_leaves) as f32);
            first_leaves + second_leaves
        }
    }
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --lib balance`
Expected: 6 passed. Then run `cargo test --lib` — all layout tests still pass.

- [ ] **Step 5: Commit**

```powershell
git add src/layout.rs
git commit -m "feat: add TileLayout::balance equal-area ratio reset

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

### Task 2: `layout.balance` API method end-to-end

**Files:**
- Modify: `src/api/schema/panes.rs` (params struct, after `LayoutSetSplitRatioParams` ~line 142)
- Modify: `src/api/schema.rs` (Method variant, after `LayoutSetSplitRatio` ~line 137)
- Modify: `src/api/schema/response.rs` (response variant, after `LayoutSplitRatioSet` ~line 139)
- Modify: `src/app/api/layouts.rs` (handler, after `handle_layout_set_split_ratio` ~line 249)
- Modify: `src/app/api.rs` (dispatch arm, after `LayoutSetSplitRatio` arm ~line 1013)
- Modify: `src/api/server.rs` (name map arm ~line 382)
- Modify: `src/api/mod.rs` (mutation classification `request_changes_ui` ~line 43)
- Modify: `src/app/runtime_mutations.rs` (wrapper + import, ~line 150)
- Test: `src/api/schema/tests.rs` (serde round-trip)

**Interfaces:**
- Consumes: `TileLayout::balance(&mut self) -> bool` (Task 1); existing `resolve_layout_export_target`, `layout_description`, `schedule_session_save`, `emit_layout_updated_event`, `encode_error`, `encode_success`.
- Produces: `LayoutBalanceParams { tab_id: Option<String>, pane_id: Option<String> }`; `Method::LayoutBalance(LayoutBalanceParams)` (wire `"layout.balance"`); `ResponseResult::LayoutBalanced { layout: LayoutDescription, changed: bool }`; `pub(crate) fn runtime_layout_balance(&mut self, id: &'static str, params: LayoutBalanceParams) -> String`. Tasks 4 and 5 use these exact names.

- [ ] **Step 1: Write the failing serde round-trip test**

Append to `src/api/schema/tests.rs`:

```rust
#[test]
fn layout_balance_method_serde_round_trip() {
    let method = Method::LayoutBalance(LayoutBalanceParams {
        tab_id: None,
        pane_id: Some("w1:p2".into()),
    });
    let json = serde_json::to_value(&method).unwrap();
    assert_eq!(json["method"], "layout.balance");
    let back: Method = serde_json::from_value(json).unwrap();
    assert_eq!(back, method);
}
```

(If the test module lacks the import, add `LayoutBalanceParams` to the existing `use` list at the top of tests.rs.)

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test --lib layout_balance_method_serde_round_trip`
Expected: compile error — `LayoutBalanceParams`/`LayoutBalance` not found.

- [ ] **Step 3: Implement all hops**

`src/api/schema/panes.rs`, after `LayoutSetSplitRatioParams`:

```rust
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema, Default)]
pub struct LayoutBalanceParams {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tab_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pane_id: Option<String>,
}
```

`src/api/schema.rs`, after the `LayoutSetSplitRatio` variant:

```rust
    #[serde(rename = "layout.balance")]
    LayoutBalance(LayoutBalanceParams),
```

(Add `LayoutBalanceParams` to the `use` of panes types at the top of schema.rs, wherever `LayoutSetSplitRatioParams` is imported.)

`src/api/schema/response.rs`, after the `LayoutSplitRatioSet` variant:

```rust
    LayoutBalanced {
        layout: LayoutDescription,
        changed: bool,
    },
```

`src/app/api/layouts.rs`, after `handle_layout_set_split_ratio` (mirror its style; note save/event are GATED on `changed` per spec, unlike set_split_ratio):

```rust
    pub(super) fn handle_layout_balance(
        &mut self,
        id: String,
        params: LayoutBalanceParams,
    ) -> String {
        let Some((ws_idx, tab_idx)) = self.resolve_layout_export_target(&LayoutExportParams {
            tab_id: params.tab_id,
            pane_id: params.pane_id,
        }) else {
            return encode_error(id, "layout_not_found", "layout target not found");
        };

        let Some(changed) = self
            .state
            .workspaces
            .get_mut(ws_idx)
            .and_then(|ws| ws.tabs.get_mut(tab_idx))
            .map(|tab| tab.layout.balance())
        else {
            return encode_error(id, "layout_not_found", "layout unavailable");
        };

        let Some(layout) = self.layout_description(ws_idx, tab_idx) else {
            return encode_error(id, "layout_not_found", "layout unavailable");
        };
        if changed {
            self.schedule_session_save();
            self.emit_layout_updated_event(ws_idx, tab_idx);
        }
        encode_success(id, ResponseResult::LayoutBalanced { layout, changed })
    }
```

(Add `LayoutBalanceParams` to layouts.rs's imports next to `LayoutSetSplitRatioParams`.)

`src/app/api.rs`, after the `LayoutSetSplitRatio` dispatch arm:

```rust
            Method::LayoutBalance(params) => {
                return self.handle_layout_balance(request.id, params);
            }
```

`src/api/server.rs`, after the `layout.set_split_ratio` arm:

```rust
        Method::LayoutBalance(_) => "layout.balance",
```

`src/api/mod.rs`, in the `request_changes_ui` `matches!` list, after `| Method::LayoutSetSplitRatio(_)`:

```rust
            | Method::LayoutBalance(_)
```

`src/app/runtime_mutations.rs`, after `runtime_layout_set_split_ratio` (~line 150), and add `LayoutBalanceParams` to the import at line 2:

```rust
    pub(crate) fn runtime_layout_balance(
        &mut self,
        id: &'static str,
        params: LayoutBalanceParams,
    ) -> String {
        self.dispatch_runtime_mutation(id, Method::LayoutBalance(params))
    }
```

Note: `runtime_layout_balance` will be dead code until Task 5 wires the TUI action — if the build fails on `-D dead_code`/warnings, add `#[allow(dead_code)]` temporarily and remove it in Task 5.

- [ ] **Step 4: Run tests**

Run: `cargo test --lib layout_balance_method_serde_round_trip`
Expected: PASS.
Run: `cargo test --lib`
Expected: everything passes EXCEPT `generated_protocol_schema_artifact_is_current` (stale artifact — fixed in Task 3). If other exhaustive-match compile errors surface (a `match` over `Method` somewhere this plan missed), add the analogous `LayoutBalance` arm following the sibling `LayoutSetSplitRatio` arm in that file.

- [ ] **Step 5: Commit**

```powershell
git add src/api src/app
git commit -m "feat: add layout.balance API method

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

### Task 3: Protocol bump 16→17 + schema artifact + version fixtures

**Files:**
- Modify: `src/protocol/wire.rs:16`
- Modify: `tests/api_ping.rs:307`
- Modify: `tests/cli_wrapper.rs` lines 1705, 1738, 1750, 1760, 1774, 1779 (the literal `16` protocol assertions inside `status_commands_report_client_and_server_versions`)
- Regenerate: `docs/next/api/herdr-api.schema.json`

**Interfaces:**
- Consumes: Task 2's schema changes (the artifact embeds the Method/Response schemas + protocol number).
- Produces: consistent protocol `17` everywhere; fresh schema artifact.

- [ ] **Step 1: Bump the constant**

`src/protocol/wire.rs` line 16: change `pub const PROTOCOL_VERSION: u32 = 16;` → `= 17;`

- [ ] **Step 2: Update hardcoded fixtures**

- `tests/api_ping.rs:307`: `assert_eq!(value["result"]["protocol"], 16);` → `17` (keep the comment above it).
- `tests/cli_wrapper.rs`: replace the six literal-16 assertions: `"  protocol: 16"` → `"  protocol: 17"` (line 1705), `"protocol: 16"` → `"protocol: 17"` (lines 1738, 1750), and `["protocol"], 16` → `["protocol"], 17` (lines 1760, 1774, 1779). Do NOT touch uses of the `CURRENT_PROTOCOL` constant elsewhere in the file.

- [ ] **Step 3: Regenerate the schema artifact**

```powershell
$env:HERDR_UPDATE_API_SCHEMA = "1"
cargo test --lib generated_protocol_schema_artifact_is_current
Remove-Item Env:HERDR_UPDATE_API_SCHEMA
```

Expected: test passes (it writes the file and returns early). `git status` should show `docs/next/api/herdr-api.schema.json` modified with the new `layout.balance` entries and `"protocol": 17`.

- [ ] **Step 4: Run the full test suite**

Run: `cargo test`
Expected: all green, including the artifact test (now current) and both fixture tests. Note: integration tests in `tests/` build the full binary — first run takes minutes.

- [ ] **Step 5: Commit**

```powershell
git add src/protocol/wire.rs tests/api_ping.rs tests/cli_wrapper.rs docs/next/api/herdr-api.schema.json
git commit -m "feat: bump protocol to 17 for layout.balance

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

### Task 4: `herdr pane balance` CLI subcommand

**Files:**
- Modify: `src/cli/pane.rs` (match arm in `run_pane_command` ~line 18; new `pane_balance` + `parse_pane_balance_args` near `pane_zoom` ~line 352; help line in `print_pane_help` ~line 1420; parser unit tests in the file's test module if one exists — check bottom of file; if none exists, add `#[cfg(test)] mod balance_tests` at the end)
- Modify: `src/cli/runtime.rs` (dispatcher next to `pane_zoom` ~line 105; add `LayoutBalanceParams` import)
- Modify: `src/cli/spec.rs` (subcommand in `pane_command()` after the `zoom` block ~line 393)

**Interfaces:**
- Consumes: `Method::LayoutBalance(LayoutBalanceParams)` (Task 2); existing `super::normalize_pane_id`, `print_method_response`, `current_pane_args()`, `option()` helpers.
- Produces: `herdr pane balance [--tab TAB_ID|--pane ID|--current]` CLI; `pub(super) fn pane_balance(params: LayoutBalanceParams) -> std::io::Result<i32>` in cli/runtime.rs.

- [ ] **Step 1: Write the failing parser tests**

In `src/cli/pane.rs` (inside the existing test module if present, else a new one at file end):

```rust
#[cfg(test)]
mod balance_tests {
    use super::*;

    fn s(args: &[&str]) -> Vec<String> {
        args.iter().map(|a| a.to_string()).collect()
    }

    #[test]
    fn balance_args_default_to_no_target() {
        let params = parse_pane_balance_args(&s(&[]), None).unwrap();
        assert_eq!(params.tab_id, None);
        assert_eq!(params.pane_id, None);
    }

    #[test]
    fn balance_args_accept_tab() {
        let params = parse_pane_balance_args(&s(&["--tab", "w1:t1"]), None).unwrap();
        assert_eq!(params.tab_id.as_deref(), Some("w1:t1"));
        assert_eq!(params.pane_id, None);
    }

    #[test]
    fn balance_args_current_resolves_env_pane() {
        let params = parse_pane_balance_args(&s(&["--current"]), Some("w1:p3")).unwrap();
        assert_eq!(params.pane_id.as_deref(), Some("w1:p3"));
    }

    #[test]
    fn balance_args_current_without_env_errors() {
        assert!(parse_pane_balance_args(&s(&["--current"]), None).is_err());
    }

    #[test]
    fn balance_args_reject_tab_and_pane_together() {
        assert!(parse_pane_balance_args(&s(&["--tab", "w1:t1", "--pane", "w1:p1"]), None).is_err());
    }
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test --lib balance_args`
Expected: compile error — `parse_pane_balance_args` not found.

- [ ] **Step 3: Implement**

`src/cli/pane.rs` — add to the `run_pane_command` match (after `"resize"`):

```rust
        "balance" => pane_balance(&args[1..]),
```

Add near `pane_zoom` (the parser takes `env_pane_id` as a parameter for testability, mirroring `parse_pane_current_args`):

```rust
fn pane_balance(args: &[String]) -> std::io::Result<i32> {
    let env_pane_id = std::env::var("HERDR_PANE_ID")
        .ok()
        .filter(|value| !value.trim().is_empty());
    let params = match parse_pane_balance_args(args, env_pane_id.as_deref()) {
        Ok(params) => params,
        Err(message) => {
            eprintln!("{message}");
            return Ok(2);
        }
    };

    super::runtime::pane_balance(params)
}

fn parse_pane_balance_args(
    args: &[String],
    env_pane_id: Option<&str>,
) -> Result<LayoutBalanceParams, String> {
    let mut tab_id: Option<String> = None;
    let mut pane_id: Option<String> = None;

    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--tab" => {
                let Some(value) = args.get(index + 1) else {
                    return Err("missing value for --tab".into());
                };
                tab_id = Some(value.clone());
                index += 2;
            }
            "--pane" => {
                let Some(value) = args.get(index + 1) else {
                    return Err("missing value for --pane".into());
                };
                pane_id = Some(super::normalize_pane_id(value));
                index += 2;
            }
            "--current" => {
                let Some(env_id) = env_pane_id else {
                    return Err(
                        "--current requires HERDR_PANE_ID (run inside a herdr pane)".into()
                    );
                };
                pane_id = Some(super::normalize_pane_id(env_id));
                index += 1;
            }
            other => return Err(format!("unknown option: {other}")),
        }
    }

    if tab_id.is_some() && pane_id.is_some() {
        return Err("provide only one of --tab or --pane/--current".into());
    }
    Ok(LayoutBalanceParams { tab_id, pane_id })
}
```

(Add `LayoutBalanceParams` to pane.rs's schema imports, wherever `PaneZoomParams` is imported from.)

Add the help line in `print_pane_help`, after the resize line:

```rust
    eprintln!("  herdr pane balance [--tab TAB_ID|--pane ID|--current]");
```

`src/cli/runtime.rs` — after `pane_resize` (add `LayoutBalanceParams` to its imports):

```rust
pub(super) fn pane_balance(params: LayoutBalanceParams) -> std::io::Result<i32> {
    print_method_response("cli:pane:balance", Method::LayoutBalance(params))
}
```

`src/cli/spec.rs` — in `pane_command()`, after the `zoom` subcommand block:

```rust
        .subcommand(
            Command::new("balance")
                .about("Balance all panes in a tab to equal areas")
                .arg(option("tab", "TAB_ID"))
                .args(current_pane_args()),
        )
```

- [ ] **Step 4: Run tests**

Run: `cargo test --lib balance_args`
Expected: 5 passed. Then `cargo test --lib` — all green.

- [ ] **Step 5: Commit**

```powershell
git add src/cli
git commit -m "feat: add herdr pane balance CLI subcommand

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

### Task 5: TUI action + `prefix+=` keybinding

**Files:**
- Modify: `src/config/keybinds.rs` (field in `Keybinds` struct after `split_horizontal` ~line 355; `empty_action!` init ~line 515; `apply_action!` merge ~line 657)
- Modify: `src/config/model.rs` (BindingConfig field after `split_horizontal` ~line 409; `apply_field!` ~line 608; `copy_effective_action_field!` ~line 707; default ~line 970)
- Modify: `src/app/input/navigate.rs` (`NavigateAction` variant ~line 1325; lookup entry in `non_indexed_action_for_key` ~line 1464; runtime arm in `execute_tui_navigate_action` ~line 370; test-executor arm in `execute_navigate_action_in_context` ~line 1685)
- Modify: `src/ui/keybind_help.rs` (entry in the `panes` help group ~line 141)

**Interfaces:**
- Consumes: `runtime_layout_balance` + `LayoutBalanceParams` (Task 2); `TileLayout::balance` (Task 1, used by the test executor); existing `leave_navigate_mode`, `empty_action!`, `apply_action!`, `apply_field!`, `copy_effective_action_field!`, `BindingConfig::one`, `help_entry`, `keybind_label` patterns.
- Produces: `NavigateAction::BalancePanes`; `balance_panes` keybinding config field, default `prefix+=`.

- [ ] **Step 1: Write the failing test**

In `src/config/model.rs`, find the existing tests module (search `#[cfg(test)]` in the file; if absent, use keybinds.rs's test module — one of the two config files has default-resolution tests; mirror whichever asserts defaults like `split_vertical`). Add:

```rust
    #[test]
    fn balance_panes_default_binding_is_prefix_equals() {
        let config = KeybindsConfig::default();
        assert_eq!(config.balance_panes.first(), Some("prefix+="));
    }
```

Adapt the struct/method names to the actual default-test pattern in that module (e.g. if defaults are asserted as `BindingConfig::one("prefix+v")` equality for `split_vertical`, copy that exact assertion shape). The intent: the default for `balance_panes` is exactly `prefix+=`.

- [ ] **Step 2: Run to verify failure**

Run: `cargo test --lib balance_panes_default`
Expected: compile error — no field `balance_panes`.

- [ ] **Step 3: Implement**

`src/config/model.rs` — field (after `split_horizontal`):

```rust
    /// Balance all panes in the current tab to equal areas. Default: "prefix+="
    pub balance_panes: BindingConfig,
```

Default (in the defaults block, after `split_horizontal: BindingConfig::one("prefix+minus"),`):

```rust
            balance_panes: BindingConfig::one("prefix+="),
```

`apply_field!(balance_panes);` after `apply_field!(split_horizontal);` and `copy_effective_action_field!(balance_panes, keybinds.balance_panes);` after the `split_horizontal` line.

`src/config/keybinds.rs` — in the `Keybinds` struct after `split_horizontal: ActionKeybinds,`:

```rust
    pub balance_panes: ActionKeybinds,
```

`balance_panes: empty_action!(),` in the init block; `apply_action!(keybinds.balance_panes, balance_panes, source);` in the merge block (both adjacent to the `split_horizontal` lines).

If `parse_key_combo` rejects the literal `=` (test in Step 4 will show it): check how `minus` is named in the key-name table near `parse_key_combo` (~line 1206) and register/use the analogous named form (e.g. `prefix+equal`) in the default + doc comment instead. Verify by running the Step 1 test plus `cargo test --lib keybind`.

`src/app/input/navigate.rs` — add variant to `NavigateAction` (after `SplitHorizontal,`):

```rust
    BalancePanes,
```

Lookup entry in `non_indexed_action_for_key` (after the `split_horizontal` tuple):

```rust
        (&kb.balance_panes, NavigateAction::BalancePanes),
```

Runtime arm in `execute_tui_navigate_action` (after the `SplitHorizontal` arm; targets active tab via empty params):

```rust
            NavigateAction::BalancePanes => {
                self.runtime_layout_balance(
                    "tui.layout.balance",
                    crate::api::schema::LayoutBalanceParams {
                        tab_id: None,
                        pane_id: None,
                    },
                );
                leave_navigate_mode(&mut self.state);
            }
```

Test-executor arm in `execute_navigate_action_in_context` (after its `SplitHorizontal` arm; mutates in-process state the way that match does):

```rust
        NavigateAction::BalancePanes => {
            if let Some(ws_idx) = state.active {
                if let Some(ws) = state.workspaces.get_mut(ws_idx) {
                    let tab_idx = ws.active_tab_index();
                    if let Some(tab) = ws.tabs.get_mut(tab_idx) {
                        tab.layout.balance();
                    }
                }
            }
            leave_navigate_mode(state);
        }
```

(Adapt field access to the actual `AppState` shape used by neighboring arms — e.g. if `state.active` is a method or named differently, mirror how `handle_layout_set_split_ratio`/other arms reach the active tab.)

`src/ui/keybind_help.rs` — in the `panes` vec after the `split horizontal` entry:

```rust
        help_entry(keybind_label(&kb.balance_panes), "balance panes"),
```

Remove any `#[allow(dead_code)]` added to `runtime_layout_balance` in Task 2.

- [ ] **Step 4: Run tests**

Run: `cargo test --lib balance_panes_default`
Expected: PASS.
Run: `cargo test`
Expected: all green. Known likely failures to fix mechanically: any default-config snapshot/serialization test now including `balance_panes` (update the snapshot per the test's own failure message — the new field with default `prefix+=` is the only expected diff), and any exhaustive `NavigateAction` match this plan missed (add a `BalancePanes` arm mirroring the sibling `SplitHorizontal`/`Zoom` arm).

- [ ] **Step 5: Commit**

```powershell
git add src/config src/app/input src/ui
git commit -m "feat: add balance-panes TUI action bound to prefix+=

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

### Task 6: E2E verification, changelog, docs

**Files:**
- Modify: `HerdrPlus:Changelog.md` (append `[0.2.0]` entry at top, below the header block — append-only file)
- Modify: `HerdrPlus:Usage.md` (add balance command to the rebuild/usage notes)

**Interfaces:**
- Consumes: everything above; the built binary at `HerdrPlus:__solutions/target/release/herdr.exe`.

- [ ] **Step 1: Release build**

Run (from `__references/herdr`): `cargo build --release`
Expected: `Finished release profile` — binary at `HerdrPlus:__solutions\target\release\herdr.exe`.

- [ ] **Step 2: E2E — uneven layout balances to equal thirds**

```powershell
$exe = "K:\Downloads\__Projects.Mine\herdr4Windows\__solutions\target\release\herdr.exe"
Start-Process $exe -WindowStyle Maximized
Start-Sleep -Seconds 4
$p1 = ((& $exe pane list | ConvertFrom-Json).result.panes | Where-Object focused).pane_id
& $exe pane split $p1 --direction right --ratio 0.8 | Out-Null
& $exe pane split $p1 --direction right --ratio 0.9 | Out-Null
& $exe pane balance
& $exe pane layout --pane $p1
```

Expected: `pane balance` returns JSON with `"type":"layout_balanced"`, `"changed":true`. The layout's split ratios read back as ≈0.333/0.5 (leaf-weighted) and the three pane rect widths are within ±1 cell of each other.

- [ ] **Step 3: E2E — no-op and target validation**

```powershell
& $exe pane balance          # second call on same tab
& $exe pane balance --tab bogus
```

Expected: second balance → `"changed":false`. Bogus tab → error response `layout_not_found` (NOT a success against the active tab).

- [ ] **Step 4: E2E — keybinding + zoomed tab**

Manually in the herdr window: resize a pane (`prefix+r`, arrows, esc), press `prefix+=` → layout evens out. Zoom a pane (`prefix+z`), press `prefix+=`, unzoom → underlying layout is balanced. Then clean up:

```powershell
& $exe server stop
```

Expected: all herdr processes exit.

- [ ] **Step 5: Append changelog entry + usage note**

Prepend to `HerdrPlus:Changelog.md` below the intro block (append-only: do not modify existing entries):

```markdown
## [0.2.0] - <today's date>

### Added

- `herdr pane balance [--tab ID|--pane ID|--current]` — equalize all pane areas in a tab (equal ideal area, split tree preserved). New `layout.balance` socket API method and `prefix+=` TUI keybinding (Vim `Ctrl-W =` muscle memory). Protocol bumped 16 → 17. Design: `docs/superpowers/specs/2026-07-17-pane-balance-design.md`.
```

In `HerdrPlus:Usage.md`, add under the run/commands section:

```markdown
herdr.exe pane balance      # equalize pane areas in the current tab (TUI: prefix+=)
```

- [ ] **Step 6: Final full test run and commit**

Run: `cargo test` (from `__references/herdr`)
Expected: all green.

```powershell
git add -A
git commit -m "feat: pane balance e2e verified; changelog and usage docs

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```
