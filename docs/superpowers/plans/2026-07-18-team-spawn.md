# Team Spawn Primitive Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** One command (`herdrplus team spawn`) that creates a workspace + N labeled agent panes + launches an agent CLI in each + joins all to a msg group, atomically with rollback, returning a label→pane map.

**Architecture:** New server-native wire method `team.spawn` (protocol 19) whose handler *composes the existing handler methods* (`handle_workspace_create`, `handle_pane_split`, `handle_pane_rename`, `handle_msg_group_join`, `handle_layout_balance`, `handle_pane_send_input`) and rolls back via `handle_workspace_close` on any failure. Agent name → command resolution comes from a new `[team.agents]` config section flattened onto `AppState`. A new `team` CLI namespace sends the request and optionally polls readiness client-side (`--wait`).

**Tech Stack:** Rust 1.96.1 (pinned), serde/schemars wire schema, hand-rolled CLI arg parsing (no clap), in-process unit tests (`cargo test --bin herdrplus`).

**Spec:** `docs/superpowers/specs/2026-07-18-team-spawn-design.md`

## Global Constraints

- Working codebase root: `__references/herdr/` — ALL file paths below are relative to it unless prefixed with repo-root markers (`docs/`, `Changelog.md` live at repo root `K:\Downloads\__Projects.Mine\herdr4Windows\`).
- Build: `cd __references/herdr && cargo build --release`. Output `__solutions/target/release/herdrplus.exe`. Never delete `.cargo/config.toml` (sets ZIG, ZIG_GLOBAL_CACHE_DIR on K:, target-dir).
- Tests: `cargo test --bin herdrplus` (package is `herdr`, bin renamed; there is NO lib target — never `--lib`). The `tests/` integration harness is Unix-only — do NOT add Windows-breaking integration tests there; all new tests are in-process under `src/` (msg-bus precedent).
- A running `herdrplus.exe` locks the binary — kill all `herdrplus`/`herdr` processes before any release build.
- Protocol bump 18 → **19**. Strict-equality client/server gate. Literal sites: `src/protocol/wire.rs:16`, `tests/support/mod.rs:18` (`CURRENT_PROTOCOL`), `tests/api_ping.rs:307`. Schema artifact `docs/next/api/herdr-api.schema.json` is NEVER hand-edited — regenerate with `HERDR_UPDATE_API_SCHEMA=1` (see Task 2; expect a long rebuild — build.rs watches that env var).
- `Changelog.md` (repo root) is append-only, Keep a Changelog format.
- Windows-only target (`x86_64-pc-windows-msvc`). Commit after each task.
- Existing error-code style: snake_case codes via `encode_error(id, "code", message)`; CLI exit codes 0 = ok, 1 = server/runtime error, 2 = usage; this feature adds 3 = `--wait` timeout (spawn itself succeeded).

---

### Task 1: `[team.agents]` config registry, flattened onto AppState

**Files:**
- Modify: `src/config/model.rs` (Config struct ~line 287-301; add `TeamConfig` nearby, model on `SessionConfig` at ~:244-258)
- Modify: `src/config.rs:21-26` (re-export `TeamConfig` in the `pub use self::model::{...}` block)
- Modify: `src/config/io.rs` (`KNOWN_TOP_LEVEL_CONFIG_KEYS` at :7-19 — add `"team"`; `load_live_config_from_str` section calls at :247-326 — add a `load_live_section` call for `team`)
- Modify: `src/app/state.rs` (add `team_agents` field to `AppState`)
- Modify: `src/app/mod.rs` (`App::new` ~:355-474 — populate `state.team_agents` from `config.team.agents`; `apply_live_config` ~:1341 — same on reload)

**Interfaces:**
- Consumes: existing `Config` derive pattern (`#[derive(Debug, Default, Deserialize)] #[serde(default)]` per section), `load_live_section(table, name, desc, ..., closure)` calls in io.rs (copy an existing call's exact signature, e.g. the `session` one).
- Produces: `pub struct TeamConfig { pub agents: std::collections::BTreeMap<String, String> }` as `Config.team`; `AppState.team_agents: std::collections::BTreeMap<String, String>` — Task 3's handler reads `self.state.team_agents`.

- [ ] **Step 1: Write the failing tests** — append to `src/config/model.rs` (new `#[cfg(test)] mod team_config_tests` at end of file; if the file already has a tests module, add these tests to it instead):

```rust
#[cfg(test)]
mod team_config_tests {
    use super::*;

    #[test]
    fn team_agents_section_parses() {
        let config: Config = toml::from_str(
            "[team.agents]\nclaude = \"claude --dangerously-skip-permissions\"\npi = \"pi\"\n",
        )
        .expect("config with [team.agents] parses");
        assert_eq!(
            config.team.agents.get("claude").map(String::as_str),
            Some("claude --dangerously-skip-permissions")
        );
        assert_eq!(config.team.agents.get("pi").map(String::as_str), Some("pi"));
    }

    #[test]
    fn team_section_defaults_empty() {
        let config: Config = toml::from_str("").expect("empty config parses");
        assert!(config.team.agents.is_empty());
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --bin herdrplus team_config_tests -- --nocapture`
Expected: FAIL to compile — `no field `team` on type `Config``

- [ ] **Step 3: Implement**

In `src/config/model.rs`, next to `SessionConfig`:

```rust
#[derive(Debug, Default, Clone, Deserialize)]
#[serde(default)]
pub struct TeamConfig {
    pub agents: std::collections::BTreeMap<String, String>,
}
```

Add `pub team: TeamConfig,` to `Config` (with `#[serde(default)]` already covering it via the struct-level attribute). Re-export `TeamConfig` in `src/config.rs`'s `pub use self::model::{...}` list.

In `src/config/io.rs`: add `"team"` to `KNOWN_TOP_LEVEL_CONFIG_KEYS`; in `load_live_config_from_str`, add a `load_live_section` call for `"team"` copying the exact shape of the `session` call (closure assigns `config.team = section`).

In `src/app/state.rs`: add field `pub team_agents: std::collections::BTreeMap<String, String>,` to `AppState` (initialize `BTreeMap::new()` wherever `AppState` is constructed, including `test_new()` at ~:1736 if it builds the struct literally).

In `src/app/mod.rs`: in `App::new` where other `config.*` values are flattened onto state (~:400-404), add `state.team_agents = config.team.agents.clone();` (adapt to the local variable naming at that site). In `apply_live_config` (~:1341), add `self.state.team_agents = config.team.agents.clone();`.

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --bin herdrplus team_config_tests`
Expected: PASS (2 tests). Also run `cargo test --bin herdrplus config` to confirm no existing config tests broke (the unknown-section guard test must not flag `team`).

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "feat: [team.agents] config registry flattened onto AppState"
```

---

### Task 2: Wire surface — `Method::TeamSpawn`, params/result, protocol 19, schema regen

**Files:**
- Create: `src/api/schema/team.rs`
- Modify: `src/api/schema.rs` (Method enum :47-244 — add variant; re-export block :17-29 — add `pub use team::*;` + `mod team;` alongside the other modules)
- Modify: `src/api/schema/response.rs` (`ResponseResult` tagged enum :42-44 area — add `TeamSpawned` variant)
- Modify: `src/api/mod.rs:22-76` (`request_changes_ui` — register `Method::TeamSpawn` so headless re-renders)
- Modify: `src/protocol/wire.rs:16` (`PROTOCOL_VERSION` 18 → 19)
- Modify: `tests/support/mod.rs:18` (`CURRENT_PROTOCOL` 18 → 19)
- Modify: `tests/api_ping.rs:307` (literal 18 → 19)
- Modify: `src/api/schema/tests.rs` (add serde round-trip test, model on `msg_methods_serde_round_trip` at :1205)
- Regenerated: `docs/next/api/herdr-api.schema.json` (via env var, never by hand)

**Interfaces:**
- Consumes: `WorkspaceInfo` (from `schema/workspaces.rs`, already re-exported), the serde tagging conventions (`#[serde(tag="method", content="params")]` on Method; `#[serde(tag="type", rename_all="snake_case")]` on ResponseResult).
- Produces (Task 3 and Task 4 depend on these exact names):
  - `TeamSpawnEntry { label: Option<String>, agent: String }`
  - `TeamSpawnParams { name: String, entries: Vec<TeamSpawnEntry>, cwd: Option<String>, with_orch: bool, orch_command: Option<String>, focus: bool }`
  - `TeamPaneInfo { label: String, pane_id: String, agent: String, command: String }`
  - `Method::TeamSpawn(TeamSpawnParams)` with wire name `"team.spawn"`
  - `ResponseResult::TeamSpawned { workspace: WorkspaceInfo, group: String, panes: Vec<TeamPaneInfo> }`

- [ ] **Step 1: Write the failing round-trip test** — in `src/api/schema/tests.rs`, next to `msg_methods_serde_round_trip` (:1205):

```rust
#[test]
fn team_methods_serde_round_trip() {
    let method = Method::TeamSpawn(TeamSpawnParams {
        name: "review".into(),
        entries: vec![
            TeamSpawnEntry { label: Some("ws1".into()), agent: "claude".into() },
            TeamSpawnEntry { label: None, agent: "grok".into() },
        ],
        cwd: Some("C:/work".into()),
        with_orch: true,
        orch_command: None,
        focus: false,
    });
    let json = serde_json::to_value(&method).unwrap();
    assert_eq!(json["method"], "team.spawn");
    assert_eq!(json["params"]["name"], "review");
    assert_eq!(json["params"]["entries"][0]["label"], "ws1");
    let back: Method = serde_json::from_value(json).unwrap();
    assert_eq!(back, method);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --bin herdrplus team_methods_serde_round_trip`
Expected: FAIL to compile — `TeamSpawnParams` not found

- [ ] **Step 3: Implement the schema types**

Create `src/api/schema/team.rs`:

```rust
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct TeamSpawnEntry {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    pub agent: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct TeamSpawnParams {
    pub name: String,
    pub entries: Vec<TeamSpawnEntry>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    #[serde(default)]
    pub with_orch: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub orch_command: Option<String>,
    #[serde(default)]
    pub focus: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct TeamPaneInfo {
    pub label: String,
    pub pane_id: String,
    pub agent: String,
    pub command: String,
}
```

In `src/api/schema.rs`: declare the module and re-export exactly as the other domain modules do (`mod team;` + `pub use team::*;` in the :17-29 block). Add the Method variant next to the msg variants (~:238):

```rust
#[serde(rename = "team.spawn")]
TeamSpawn(TeamSpawnParams),
```

In `src/api/schema/response.rs`, add to `ResponseResult` (match surrounding variant style; the enum is `#[serde(tag = "type", rename_all = "snake_case")]` so this serializes as `"type":"team_spawned"`):

```rust
TeamSpawned {
    workspace: WorkspaceInfo,
    group: String,
    panes: Vec<TeamPaneInfo>,
},
```

In `src/api/mod.rs` `request_changes_ui` (:22-76): add `Method::TeamSpawn(_)` to the set of UI-dirtying methods, matching the existing arm style (WorkspaceCreate/LayoutBalance/PaneSplit are all listed — copy their pattern).

In `src/protocol/wire.rs:16`: `pub const PROTOCOL_VERSION: u32 = 19;`
In `tests/support/mod.rs:18`: `pub const CURRENT_PROTOCOL: u32 = 19;`
In `tests/api_ping.rs:307`: assert against `19`.

- [ ] **Step 4: Run the round-trip test**

Run: `cargo test --bin herdrplus team_methods_serde_round_trip`
Expected: PASS

- [ ] **Step 5: Regenerate the schema artifact**

Run (PowerShell): `$env:HERDR_UPDATE_API_SCHEMA = "1"; cargo test --bin herdrplus generated_protocol_schema_artifact_is_current; Remove-Item Env:HERDR_UPDATE_API_SCHEMA`
Expected: PASS after rewriting `docs/next/api/herdr-api.schema.json` (now `"protocol": 19` + `team.spawn` present). NOTE: this env var triggers a large rebuild (build.rs watches it) — run it exactly once here, then unset. Immediately re-run WITHOUT the env var to prove the artifact is current: `cargo test --bin herdrplus generated_protocol_schema_artifact_is_current` → PASS.

- [ ] **Step 6: Full compile + suite sanity**

Run: `cargo test --bin herdrplus`
Expected: PASS. The new Method variant hits the `_ => not_implemented` dispatch fallthrough (`src/app/api.rs:1128`), which compiles fine — the handler comes in Task 3.

- [ ] **Step 7: Commit**

```bash
git add -A
git commit -m "feat: team.spawn wire surface, protocol 19, schema regen"
```

---

### Task 3: Server handler `handle_team_spawn` + dispatch

**Files:**
- Create: `src/app/api/team.rs`
- Modify: `src/app/api.rs` (module list :4-16 — add `mod team;`; dispatch match :875-1135 — add arm next to the msg arms at :1122-1127)

**Interfaces:**
- Consumes (all verified against source):
  - `self.handle_workspace_create(id, WorkspaceCreateParams) -> String` → JSON `ResponseResult::WorkspaceCreated { workspace, tab, root_pane }`
  - `self.handle_pane_split(id, PaneSplitParams) -> String` → `ResponseResult::PaneInfo { pane }` (`src/app/api/panes.rs:32`)
  - `self.handle_pane_rename(id, PaneRenameParams) -> String` (`panes.rs:1149`)
  - `self.handle_msg_group_join(id, MsgGroupJoinParams) -> String` (`src/app/api/msg.rs:217`)
  - `self.handle_layout_balance(id, LayoutBalanceParams) -> String` (`src/app/api/layouts.rs:257`)
  - `self.handle_pane_send_input(id, PaneSendInputParams) -> String` (`panes.rs:1501`; text bytes first, then keys — pass `keys: vec!["Enter".into()]` as `src/cli/pane.rs:1001-1005` does)
  - `self.handle_workspace_close(id, WorkspaceTarget) -> String` (`workspaces.rs:227` — owns full teardown incl. msg-bus purge)
  - `self.state.msg_bus.group_members(&str) -> Vec<String>` (`src/msg.rs:391`) for the collision check
  - `self.state.team_agents: BTreeMap<String, String>` (Task 1)
  - `encode_error` / `encode_success` from `super::responses`; `SuccessResponse` from `crate::api::schema`
- Produces: `pub(super) fn handle_team_spawn(&mut self, id: String, params: TeamSpawnParams) -> String` returning `ResponseResult::TeamSpawned`. Error codes: `empty_roster`, `invalid_team_name`, `invalid_label`, `duplicate_label`, `team_exists`, `team_spawn_failed`.

- [ ] **Step 1: Write the failing tests** — `src/app/api/team.rs` skeleton with tests module (handler body `todo!()`-free: write the file with a stub returning `encode_error(id, "not_implemented", "")` so tests compile but fail):

The tests module, modeled exactly on `src/app/api/msg.rs:346-370` (`app_with_workspace` builds an `App` with `exiting_test_command()` shell, so real handler calls work in-process on Windows):

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::schema::{
        MsgWhoParams, ResponseResult, SuccessResponse, TeamSpawnEntry, TeamSpawnParams,
    };
    use crate::app::App;
    use crate::app::api::test_support::exiting_test_command;
    use crate::config::{Config, ShellModeConfig};
    use crate::workspace::Workspace;

    fn app() -> App {
        let (_api_tx, api_rx) = tokio::sync::mpsc::unbounded_channel();
        let mut app = App::new(
            &Config::default(),
            true,
            None,
            api_rx,
            crate::api::EventHub::default(),
        );
        app.state.default_shell = exiting_test_command().into();
        app.state.shell_mode = ShellModeConfig::NonLogin;
        app.state.workspaces = vec![Workspace::test_new("home")];
        app.state.active = Some(0);
        app.state.selected = 0;
        app.state.ensure_test_terminals();
        app
    }

    fn spawn_params(name: &str, entries: Vec<TeamSpawnEntry>) -> TeamSpawnParams {
        TeamSpawnParams {
            name: name.into(),
            entries,
            cwd: None,
            with_orch: false,
            orch_command: None,
            focus: false,
        }
    }

    fn entry(label: Option<&str>, agent: &str) -> TeamSpawnEntry {
        TeamSpawnEntry { label: label.map(Into::into), agent: agent.into() }
    }

    fn parse_spawned(res: &str) -> (String, String, Vec<crate::api::schema::TeamPaneInfo>) {
        let success: SuccessResponse = serde_json::from_str(res).expect("success response");
        let ResponseResult::TeamSpawned { workspace, group, panes } = success.result else {
            panic!("expected TeamSpawned, got: {res}");
        };
        (workspace.workspace_id, group, panes)
    }

    #[test]
    fn spawn_happy_path_creates_panes_labels_and_group() {
        let mut app = app();
        app.state.team_agents.insert("claude".into(), "claude --dangerously-skip-permissions".into());
        let res = app.handle_team_spawn(
            "req_1".into(),
            spawn_params("review", vec![entry(Some("ws1"), "claude"), entry(None, "claude"), entry(None, "grok")]),
        );
        let (workspace_id, group, panes) = parse_spawned(&res);
        assert_eq!(group, "review");
        assert_eq!(panes.len(), 3);
        assert_eq!(panes[0].label, "ws1");
        assert_eq!(panes[0].command, "claude --dangerously-skip-permissions");
        assert_eq!(panes[1].label, "claude-1");
        assert_eq!(panes[2].label, "grok-1");
        assert_eq!(panes[2].command, "grok"); // registry miss -> verbatim passthrough
        assert!(!workspace_id.is_empty());

        // every pane is a member of the group, addressable by label
        let who = app.handle_msg_who("req_2".into(), MsgWhoParams::default());
        let success: SuccessResponse = serde_json::from_str(&who).unwrap();
        let ResponseResult::MsgWho { groups, .. } = success.result else { panic!() };
        let review = groups.iter().find(|g| g.group == "review").expect("group exists");
        assert_eq!(review.members.len(), 3);
    }

    #[test]
    fn spawn_with_orch_adds_orch_pane_in_group() {
        let mut app = app();
        let mut params = spawn_params("t", vec![entry(None, "pwsh"), entry(None, "pwsh")]);
        params.with_orch = true;
        let res = app.handle_team_spawn("req_1".into(), params);
        let (_, _, panes) = parse_spawned(&res);
        assert_eq!(panes.len(), 3);
        assert_eq!(panes[2].label, "orch");
        assert_eq!(panes[2].command, ""); // no command sent: plain shell
        assert_eq!(app.state.msg_bus.group_members("t").len(), 3);
    }

    #[test]
    fn duplicate_labels_rejected_before_creation() {
        let mut app = app();
        let before = app.state.workspaces.len();
        let res = app.handle_team_spawn(
            "req_1".into(),
            spawn_params("t", vec![entry(Some("a"), "pwsh"), entry(Some("a"), "pwsh")]),
        );
        assert!(res.contains("duplicate_label"), "got: {res}");
        assert_eq!(app.state.workspaces.len(), before);
    }

    #[test]
    fn invalid_labels_and_names_rejected() {
        let mut app = app();
        for bad_name in ["", "@team", "a/b", "has space"] {
            let res = app.handle_team_spawn(
                format!("req_{bad_name}"),
                spawn_params(bad_name, vec![entry(None, "pwsh")]),
            );
            assert!(res.contains("invalid_team_name"), "name {bad_name:?} got: {res}");
        }
        let res = app.handle_team_spawn(
            "req_l".into(),
            spawn_params("t", vec![entry(Some("@bad"), "pwsh")]),
        );
        assert!(res.contains("invalid_label"), "got: {res}");
        let res = app.handle_team_spawn("req_e".into(), spawn_params("t", vec![]));
        assert!(res.contains("empty_roster"), "got: {res}");
        // "orch" reserved only when with_orch is set
        let mut params = spawn_params("t", vec![entry(Some("orch"), "pwsh")]);
        params.with_orch = true;
        let res = app.handle_team_spawn("req_o".into(), params);
        assert!(res.contains("invalid_label"), "got: {res}");
    }

    #[test]
    fn existing_group_name_refused() {
        let mut app = app();
        let root = app.state.workspaces[0].tabs[0].root_pane;
        let root_pane_id = app.public_pane_id(0, root).unwrap();
        app.state.msg_bus.group_join(&root_pane_id, "review").unwrap();
        let before = app.state.workspaces.len();
        let res = app.handle_team_spawn(
            "req_1".into(),
            spawn_params("review", vec![entry(None, "pwsh")]),
        );
        assert!(res.contains("team_exists"), "got: {res}");
        assert_eq!(app.state.workspaces.len(), before);
    }

    #[test]
    fn rollback_leaves_no_workspace_or_group_residue() {
        let mut app = app();
        let before = app.state.workspaces.len();
        // Force a mid-spawn failure via the test-only failure injection point
        // (see implementation: TEAM_SPAWN_FAIL_AFTER_SPLITS).
        let res = app.handle_team_spawn_with_fault(
            "req_1".into(),
            spawn_params("doomed", vec![entry(None, "pwsh"), entry(None, "pwsh")]),
            Some(TeamSpawnFault::AfterSplits),
        );
        assert!(res.contains("team_spawn_failed"), "got: {res}");
        assert_eq!(app.state.workspaces.len(), before, "workspace rolled back");
        assert!(app.state.msg_bus.group_members("doomed").is_empty(), "group purged");
    }
}
```

Note for the implementer: field names on `MsgWhoGroupInfo` (`group`, `members`) and `WorkspaceInfo` (`workspace_id`) should be confirmed against `src/api/schema/msg.rs:70+` and `schema/workspaces.rs` when writing assertions — adjust accessor names to the real ones if they differ (the msg test at `src/app/api/msg.rs:846-900` shows the real shapes). `MsgWhoParams::default()` — if it lacks `Default`, construct it literally as the msg tests do.

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --bin herdrplus "api::team"`
Expected: FAIL (stub returns not_implemented / missing functions)

- [ ] **Step 3: Implement the handler**

`src/app/api/team.rs` (complete implementation; adjust only where noted):

```rust
use crate::api::schema::{
    LayoutBalanceParams, MsgGroupJoinParams, PaneRenameParams, PaneSendInputParams,
    PaneSplitParams, ResponseResult, SplitDirection, SuccessResponse, TeamPaneInfo,
    TeamSpawnParams, WorkspaceCreateParams, WorkspaceTarget,
};
use crate::app::App;
use crate::app::api::responses::{encode_error, encode_success};
use std::collections::HashMap;

/// Test-only fault injection points for exercising rollback.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum TeamSpawnFault {
    AfterSplits,
}

fn valid_name(s: &str) -> bool {
    !s.is_empty()
        && !s.contains('/')
        && !s.starts_with('@')
        && !s.chars().any(char::is_whitespace)
}

/// Parse a sibling handler's JSON reply; Ok(result) on success, Err(code: message) on error.
fn parse_reply(response: &str) -> Result<ResponseResult, String> {
    if let Ok(success) = serde_json::from_str::<SuccessResponse>(response) {
        return Ok(success.result);
    }
    let value: serde_json::Value = serde_json::from_str(response).unwrap_or_default();
    let code = value["error"]["code"].as_str().unwrap_or("unknown");
    let message = value["error"]["message"].as_str().unwrap_or("unknown error");
    Err(format!("{code}: {message}"))
}

fn pane_id_of(result: ResponseResult) -> Result<String, String> {
    match result {
        ResponseResult::PaneInfo { pane } => Ok(pane.pane_id),
        other => Err(format!("unexpected reply: {other:?}")),
    }
}

impl App {
    pub(super) fn handle_team_spawn(&mut self, id: String, params: TeamSpawnParams) -> String {
        self.handle_team_spawn_with_fault(id, params, None)
    }

    pub(super) fn handle_team_spawn_with_fault(
        &mut self,
        id: String,
        params: TeamSpawnParams,
        fault: Option<TeamSpawnFault>,
    ) -> String {
        // ---- validate everything before creating anything ----
        if params.entries.is_empty() {
            return encode_error(id, "empty_roster", "at least one --agents entry is required");
        }
        if !valid_name(&params.name) {
            return encode_error(
                id,
                "invalid_team_name",
                "team name must be non-empty with no '/', leading '@', or whitespace",
            );
        }
        let mut labels: Vec<String> = Vec::with_capacity(params.entries.len());
        let mut counters: HashMap<String, usize> = HashMap::new();
        for entry in &params.entries {
            let label = match &entry.label {
                Some(label) => label.clone(),
                None => {
                    let n = counters.entry(entry.agent.clone()).or_insert(0);
                    *n += 1;
                    format!("{}-{}", entry.agent, n)
                }
            };
            if !valid_name(&label) || (params.with_orch && label == "orch") {
                return encode_error(
                    id,
                    "invalid_label",
                    format!("invalid pane label {label:?}"),
                );
            }
            if labels.contains(&label) {
                return encode_error(id, "duplicate_label", format!("duplicate label {label:?}"));
            }
            labels.push(label);
        }
        if !self.state.msg_bus.group_members(&params.name).is_empty() {
            return encode_error(
                id,
                "team_exists",
                format!(
                    "msg group {:?} already has members — pick another name or close the old workspace",
                    params.name
                ),
            );
        }
        let commands: Vec<String> = params
            .entries
            .iter()
            .map(|e| {
                self.state
                    .team_agents
                    .get(&e.agent)
                    .cloned()
                    .unwrap_or_else(|| e.agent.clone())
            })
            .collect();

        // ---- create workspace (root pane = first team pane) ----
        let created = self.handle_workspace_create(
            format!("{id}:ws"),
            WorkspaceCreateParams {
                cwd: params.cwd.clone(),
                focus: params.focus,
                label: Some(params.name.clone()),
                env: HashMap::new(),
            },
        );
        let (workspace, root_pane_id) = match parse_reply(&created) {
            Ok(ResponseResult::WorkspaceCreated { workspace, root_pane, .. }) => {
                (workspace, root_pane.pane_id)
            }
            Ok(other) => {
                return encode_error(id, "team_spawn_failed", format!("workspace create: unexpected reply {other:?}"))
            }
            Err(err) => return encode_error(id, "team_spawn_failed", format!("workspace create: {err}")),
        };
        let workspace_id = workspace.workspace_id.clone();

        // Everything after this point must roll back on failure.
        macro_rules! fail {
            ($step:expr, $err:expr) => {{
                let _ = self.handle_workspace_close(
                    format!("{id}:rollback"),
                    WorkspaceTarget { workspace_id: workspace_id.clone() },
                );
                return encode_error(
                    id,
                    "team_spawn_failed",
                    format!("{} failed: {} (workspace rolled back)", $step, $err),
                );
            }};
        }

        // ---- split to a rough row-major grid; balance() equalizes areas after ----
        let total = params.entries.len() + usize::from(params.with_orch);
        let cols = (total as f64).sqrt().ceil() as usize;
        let mut pane_ids: Vec<String> = vec![root_pane_id.clone()];
        let mut row_seeds: Vec<String> = vec![root_pane_id.clone()];
        let rows = total.div_ceil(cols);
        for _ in 1..rows {
            let seed = row_seeds.last().cloned().unwrap();
            match self.split_team_pane(&id, &seed, SplitDirection::Down, params.cwd.as_deref()) {
                Ok(new_id) => {
                    row_seeds.push(new_id.clone());
                    pane_ids.push(new_id);
                }
                Err(err) => fail!("pane split", err),
            }
        }
        // fill each row left-to-right (row-major: row r holds entries r*cols..)
        let mut grid: Vec<String> = Vec::with_capacity(total);
        for (row, seed) in row_seeds.iter().enumerate() {
            let count = (total - row * cols).min(cols);
            let mut row_panes = vec![seed.clone()];
            for _ in 1..count {
                let target = row_panes.last().cloned().unwrap();
                match self.split_team_pane(&id, &target, SplitDirection::Right, params.cwd.as_deref()) {
                    Ok(new_id) => {
                        pane_ids.push(new_id.clone());
                        row_panes.push(new_id);
                    }
                    Err(err) => fail!("pane split", err),
                }
            }
            grid.extend(row_panes);
        }
        if fault == Some(TeamSpawnFault::AfterSplits) {
            fail!("fault injection", "test-forced failure");
        }

        // ---- balance the tab ----
        let balanced = self.handle_layout_balance(
            format!("{id}:balance"),
            LayoutBalanceParams { tab_id: None, pane_id: Some(root_pane_id.clone()) },
        );
        if let Err(err) = parse_reply(&balanced) {
            fail!("layout balance", err);
        }

        // ---- label + group-join every pane (before any agent launches) ----
        let mut all_labels = labels.clone();
        if params.with_orch {
            all_labels.push("orch".to_string());
        }
        for (pane_id, label) in grid.iter().zip(all_labels.iter()) {
            let renamed = self.handle_pane_rename(
                format!("{id}:rename"),
                PaneRenameParams { pane_id: pane_id.clone(), label: Some(label.clone()) },
            );
            if let Err(err) = parse_reply(&renamed) {
                fail!("pane rename", err);
            }
            let joined = self.handle_msg_group_join(
                format!("{id}:join"),
                MsgGroupJoinParams { pane_id: pane_id.clone(), group: params.name.clone() },
            );
            if let Err(err) = parse_reply(&joined) {
                fail!("group join", err);
            }
        }

        // ---- send launch commands last ----
        let mut result_panes: Vec<TeamPaneInfo> = Vec::with_capacity(total);
        for (i, (pane_id, label)) in grid.iter().zip(all_labels.iter()).enumerate() {
            let (agent, command) = if i < params.entries.len() {
                (params.entries[i].agent.clone(), commands[i].clone())
            } else {
                ("orch".to_string(), params.orch_command.clone().unwrap_or_default())
            };
            if !command.is_empty() {
                let sent = self.handle_pane_send_input(
                    format!("{id}:run"),
                    PaneSendInputParams {
                        pane_id: pane_id.clone(),
                        text: command.clone(),
                        keys: vec!["Enter".into()],
                    },
                );
                if let Err(err) = parse_reply(&sent) {
                    fail!("command launch", err);
                }
            }
            result_panes.push(TeamPaneInfo {
                label: label.clone(),
                pane_id: pane_id.clone(),
                agent,
                command,
            });
        }

        encode_success(
            id,
            ResponseResult::TeamSpawned { workspace, group: params.name, panes: result_panes },
        )
    }

    fn split_team_pane(
        &mut self,
        id: &str,
        target_pane_id: &str,
        direction: SplitDirection,
        cwd: Option<&str>,
    ) -> Result<String, String> {
        let response = self.handle_pane_split(
            format!("{id}:split"),
            PaneSplitParams {
                workspace_id: None,
                target_pane_id: Some(target_pane_id.to_string()),
                direction,
                ratio: None,
                cwd: cwd.map(str::to_string),
                focus: false,
                env: HashMap::new(),
            },
        );
        parse_reply(&response).and_then(pane_id_of)
    }
}
```

Implementation notes:
- `WorkspaceTarget` — confirm its exact field name (`workspace_id`) in `src/api/schema/workspaces.rs`; `handle_workspace_close(id, target)` is at `workspaces.rs:227`.
- The `macro_rules! fail` avoids borrowing issues a closure would hit on `&mut self`.
- Rollback rides `handle_workspace_close` → `close_selected_workspace()` (`src/app/actions.rs:1524`) which owns the msg-bus purge — the PR #2-hardened path.
- If `ResponseResult` doesn't derive `Debug` for the `format!("{other:?}")` calls, format a static string instead.

Dispatch: in `src/app/api.rs` add `mod team;` to the module list (:4-16) and this arm next to the msg arms (:1122-1127):

```rust
Method::TeamSpawn(params) => return self.handle_team_spawn(request_id, params),
```

(match the surrounding arms' exact style for the request-id variable name).

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --bin herdrplus "api::team"`
Expected: PASS (6 tests). Then full suite: `cargo test --bin herdrplus` → PASS.

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "feat: team.spawn server handler with grid split, group join, rollback"
```

---

### Task 4: CLI `herdrplus team spawn` (+ `--wait`)

**Files:**
- Create: `src/cli/team.rs`
- Modify: `src/cli.rs` (module list :13-28 — add `mod team;`; namespace dispatch :68-93 — add `"team" => team::run_team_command(&args[2..])?,` next to `"msg"` at :87)

**Interfaces:**
- Consumes: `super::send_request(&Request) -> io::Result<serde_json::Value>` (`src/cli.rs:997`), `super::print_response(&serde_json::Value) -> io::Result<i32>` (:973), `Method::TeamSpawn` + `TeamSpawnParams`/`TeamSpawnEntry` (Task 2), `Method::PaneGet`-equivalent for polling (find the exact pane-get Method variant + params in `src/api/schema.rs` — the CLI `pane get` handler in `src/cli/pane.rs` shows the construction), `crate::detect::identify_agent(&str) -> Option<Agent>` (`src/detect/mod.rs:130`) for wait-detectability.
- Produces: `pub(super) fn run_team_command(args: &[String]) -> std::io::Result<i32>`; pure parse fn `parse_spawn_args(args: &[String]) -> Result<SpawnArgs, String>` with `struct SpawnArgs { params: TeamSpawnParams, wait: bool, timeout_secs: u64 }`.

- [ ] **Step 1: Write the failing parse tests** — in `src/cli/team.rs` (tests modeled on `src/cli/msg.rs:662+`):

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn parse_minimal_roster() {
        let parsed = parse_spawn_args(&args(&["review", "--agents", "claude,grok"])).unwrap();
        assert_eq!(parsed.params.name, "review");
        assert_eq!(parsed.params.entries.len(), 2);
        assert_eq!(parsed.params.entries[0].agent, "claude");
        assert_eq!(parsed.params.entries[0].label, None);
        assert!(!parsed.params.with_orch);
        assert!(!parsed.wait);
    }

    #[test]
    fn parse_labels_and_flags_any_order() {
        let parsed = parse_spawn_args(&args(&[
            "review", "--wait", "--agents", "ws1=claude,reviewer=grok", "--timeout", "120",
            "--cwd", "C:/work",
        ]))
        .unwrap();
        assert_eq!(parsed.params.entries[0].label.as_deref(), Some("ws1"));
        assert_eq!(parsed.params.entries[1].agent, "grok");
        assert_eq!(parsed.params.cwd.as_deref(), Some("C:/work"));
        assert!(parsed.wait);
        assert_eq!(parsed.timeout_secs, 120);
    }

    #[test]
    fn parse_with_orch_optional_value() {
        let bare = parse_spawn_args(&args(&["t", "--agents", "pi", "--with-orch"])).unwrap();
        assert!(bare.params.with_orch);
        assert_eq!(bare.params.orch_command, None);

        let with_cmd =
            parse_spawn_args(&args(&["t", "--agents", "pi", "--with-orch", "pwsh -NoLogo"])).unwrap();
        assert!(with_cmd.params.with_orch);
        assert_eq!(with_cmd.params.orch_command.as_deref(), Some("pwsh -NoLogo"));

        // a following flag is NOT consumed as the orch command
        let flag_after =
            parse_spawn_args(&args(&["t", "--agents", "pi", "--with-orch", "--wait"])).unwrap();
        assert!(flag_after.params.with_orch);
        assert_eq!(flag_after.params.orch_command, None);
        assert!(flag_after.wait);
    }

    #[test]
    fn parse_rejects_bad_input() {
        assert!(parse_spawn_args(&args(&[])).is_err()); // no name
        assert!(parse_spawn_args(&args(&["t"])).is_err()); // no --agents
        assert!(parse_spawn_args(&args(&["t", "--agents", ""])).is_err()); // empty roster
        assert!(parse_spawn_args(&args(&["t", "--agents", "a=claude,a=grok"])).is_err()); // dup label
        assert!(parse_spawn_args(&args(&["t", "--agents", "claude", "--timeout", "abc"])).is_err());
    }

    #[test]
    fn default_timeout_is_60() {
        let parsed = parse_spawn_args(&args(&["t", "--agents", "pi", "--wait"])).unwrap();
        assert_eq!(parsed.timeout_secs, 60);
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --bin herdrplus "cli::team"`
Expected: FAIL to compile — module/functions missing

- [ ] **Step 3: Implement**

`src/cli/team.rs` structure (complete the marked call sites against the real signatures when wiring — they are all named above in Interfaces):

```rust
use std::collections::BTreeMap;
use crate::api::schema::{Method, PaneTarget, Request, TeamSpawnEntry, TeamSpawnParams};

pub(super) struct SpawnArgs {
    pub params: TeamSpawnParams,
    pub wait: bool,
    pub timeout_secs: u64,
}

pub(super) fn run_team_command(args: &[String]) -> std::io::Result<i32> {
    let Some(subcommand) = args.first().map(|arg| arg.as_str()) else {
        print_team_help();
        return Ok(2);
    };
    match subcommand {
        "spawn" => team_spawn(&args[1..]),
        "help" | "--help" | "-h" => {
            print_team_help();
            Ok(0)
        }
        _ => {
            print_team_help();
            Ok(2)
        }
    }
}

fn print_team_help() {
    eprintln!("usage: herdr team spawn <name> --agents <entry>[,<entry>...] [--cwd DIR] [--with-orch [CMD]] [--wait] [--timeout SECS]");
    eprintln!("  entry = <agent> | <label>=<agent>   (agent resolved via [team.agents] config; unknown names run verbatim)");
}

pub(super) fn parse_spawn_args(args: &[String]) -> Result<SpawnArgs, String> {
    let mut name: Option<String> = None;
    let mut entries: Vec<TeamSpawnEntry> = Vec::new();
    let mut cwd: Option<String> = None;
    let mut with_orch = false;
    let mut orch_command: Option<String> = None;
    let mut wait = false;
    let mut timeout_secs: u64 = 60;

    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--agents" => {
                let value = args.get(index + 1).ok_or("missing value for --agents")?;
                for raw in value.split(',') {
                    let raw = raw.trim();
                    if raw.is_empty() {
                        return Err("empty --agents entry".into());
                    }
                    let (label, agent) = match raw.split_once('=') {
                        Some((label, agent)) => (Some(label.to_string()), agent.to_string()),
                        None => (None, raw.to_string()),
                    };
                    if agent.trim().is_empty() {
                        return Err(format!("entry {raw:?} has an empty agent"));
                    }
                    if let Some(label) = &label {
                        if entries.iter().any(|e| e.label.as_deref() == Some(label)) {
                            return Err(format!("duplicate label {label:?}"));
                        }
                    }
                    entries.push(TeamSpawnEntry { label, agent });
                }
                index += 2;
            }
            "--cwd" => {
                cwd = Some(args.get(index + 1).ok_or("missing value for --cwd")?.clone());
                index += 2;
            }
            "--with-orch" => {
                with_orch = true;
                match args.get(index + 1) {
                    Some(next) if !next.starts_with("--") => {
                        orch_command = Some(next.clone());
                        index += 2;
                    }
                    _ => index += 1,
                }
            }
            "--wait" => {
                wait = true;
                index += 1;
            }
            "--timeout" => {
                let value = args.get(index + 1).ok_or("missing value for --timeout")?;
                timeout_secs = value.parse().map_err(|_| format!("invalid --timeout {value:?}"))?;
                index += 2;
            }
            other if name.is_none() && !other.starts_with("--") => {
                name = Some(other.to_string());
                index += 1;
            }
            other => return Err(format!("unexpected argument {other:?}")),
        }
    }

    let name = name.ok_or("missing team name")?;
    if entries.is_empty() {
        return Err("--agents with at least one entry is required".into());
    }
    Ok(SpawnArgs {
        params: TeamSpawnParams {
            name,
            entries,
            cwd,
            with_orch,
            orch_command,
            focus: false,
        },
        wait,
        timeout_secs,
    })
}

fn team_spawn(args: &[String]) -> std::io::Result<i32> {
    let parsed = match parse_spawn_args(args) {
        Ok(parsed) => parsed,
        Err(message) => {
            eprintln!("{message}");
            print_team_help();
            return Ok(2);
        }
    };
    let wait = parsed.wait;
    let timeout_secs = parsed.timeout_secs;
    let response = super::send_request(&Request {
        id: "cli:team:spawn".into(),
        method: Method::TeamSpawn(parsed.params),
    })?;
    let exit = super::print_response(&response)?;
    if exit != 0 || !wait {
        return Ok(exit);
    }
    wait_for_team_ready(&response, timeout_secs)
}

/// Poll each detectable agent pane until agent_status != "unknown", or timeout.
/// Prints a per-pane readiness report; exit 0 all ready, exit 3 on timeout.
fn wait_for_team_ready(spawn_response: &serde_json::Value, timeout_secs: u64) -> std::io::Result<i32> {
    let panes = spawn_response["result"]["panes"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    // detectable = herdr's detect module knows this agent name (claude/codex/gemini/pi/...)
    let mut pending: BTreeMap<String, String> = panes
        .iter()
        .filter(|p| {
            p["agent"]
                .as_str()
                .map(|a| crate::detect::identify_agent(a).is_some())
                .unwrap_or(false)
        })
        .filter_map(|p| {
            Some((p["pane_id"].as_str()?.to_string(), p["label"].as_str()?.to_string()))
        })
        .collect();
    let skipped: Vec<String> = panes
        .iter()
        .filter(|p| {
            p["agent"]
                .as_str()
                .map(|a| crate::detect::identify_agent(a).is_none())
                .unwrap_or(true)
        })
        .filter_map(|p| p["label"].as_str().map(str::to_string))
        .collect();
    if !skipped.is_empty() {
        eprintln!("not detectable (skipped from --wait): {}", skipped.join(", "));
    }
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(timeout_secs);
    while !pending.is_empty() {
        if std::time::Instant::now() >= deadline {
            let stuck: Vec<&str> = pending.values().map(String::as_str).collect();
            eprintln!("--wait timeout: not ready: {}", stuck.join(", "));
            return Ok(3);
        }
        let mut ready: Vec<String> = Vec::new();
        for pane_id in pending.keys().cloned().collect::<Vec<_>>() {
            let response = super::send_request(&Request {
                id: "cli:team:wait".into(),
                method: Method::PaneGet(PaneTarget { pane_id: pane_id.clone() }),
            })?;
            let status = response["result"]["pane"]["agent_status"].as_str().unwrap_or("unknown");
            if status != "unknown" {
                ready.push(pane_id);
            }
        }
        for pane_id in ready {
            if let Some(label) = pending.remove(&pane_id) {
                eprintln!("ready: {label}");
            }
        }
        if !pending.is_empty() {
            std::thread::sleep(std::time::Duration::from_millis(1000));
        }
    }
    Ok(0)
}
```

(`Request { id, method }` construction and `Method::PaneGet(PaneTarget { pane_id })` are verbatim from `src/cli/msg.rs:245-248` and `src/cli/pane.rs:90-95`; confirm the `use` paths match how those files import `Request`.)

In `src/cli.rs`: add `mod team;` to the module list and the dispatch arm `"team" => team::run_team_command(&args[2..])?,` next to `"msg"` (:87). Also add a `team` line to the top-level CLI help if `src/cli.rs` prints one (check `print_cli_help`/equivalent near the dispatch; and check `src/cli/spec.rs` — if it holds a namespace table for completions, add `team` there too).

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --bin herdrplus "cli::team"`
Expected: PASS (5 tests). Full suite: `cargo test --bin herdrplus` → PASS.

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "feat: herdr team spawn CLI with --wait readiness polling"
```

---

### Task 5: Changelog, release build, live e2e

**Files:**
- Modify: `Changelog.md` (repo root, append-only)
- No source changes expected; fixes discovered here get their own commits.

**Interfaces:**
- Consumes: everything above; the real `__solutions/target/release/herdrplus.exe`.

- [ ] **Step 1: Append Changelog entry** (repo root `Changelog.md`, Keep a Changelog format, new entry — never rewrite old ones):

```markdown
## [Unreleased] - 2026-07-18 (team spawn)
### Added
- `herdrplus team spawn <name> --agents <entry>[,...]` — one-shot team creation:
  workspace + N labeled agent panes + agent CLI launched in each + all panes
  joined to msg group `<name>`; balanced layout; label→pane map returned as JSON.
  Optional `--with-orch [cmd]`, `--cwd`, `--wait [--timeout secs]` (exit 3 on
  wait timeout, team still up). Server-native wire method `team.spawn`; full
  rollback (workspace close cascade) on partial failure; refuses existing group
  names (`team_exists`).
- `[team.agents]` config section: agent name → launch command registry
  (unknown names pass through verbatim).
### Changed
- Protocol version 18 → 19.
```

- [ ] **Step 2: Kill running instances and build release**

Run: `Get-Process herdrplus,herdr -ErrorAction SilentlyContinue | Stop-Process -Force -Confirm:$false` then `cd __references/herdr && cargo build --release`
Expected: clean build; fresh `__solutions/target/release/herdrplus.exe` (verify timestamp).

- [ ] **Step 3: Full test suite on release profile artifacts**

Run: `cargo test --bin herdrplus`
Expected: all green.

- [ ] **Step 4: Live e2e — passthrough smoke** (drive the REAL binary; this caught bugs green tests missed before)

1. Start the TUI: launch `__solutions\target\release\herdrplus.exe` in a real terminal window (leave it running).
2. From another shell: `herdrplus.exe team spawn smoke --agents a=pwsh,b=pwsh --with-orch`
   Expected: JSON pane map with labels `a`, `b`, `orch`; TUI shows a new balanced 3-pane workspace labeled `smoke`, with `pwsh` running in panes a/b.
3. `herdrplus.exe msg who` → group `smoke` has 3 members.
4. Msg round-trip: `herdrplus.exe msg send b "ping" --pane <pane_id_of_a>` then `herdrplus.exe msg read --pane <pane_id_of_b>` → shows "ping" from label `a`.
5. Collision refusal: re-run the same spawn → error `team_exists`, exit 1, no new workspace.
6. Teardown: `herdrplus.exe workspace close <workspace_id>` → workspace gone; `msg who` shows no `smoke` group.
7. ConPTY input-buffering check (spec Section: Data flow step 6): in the TUI confirm both `pwsh` panes actually received and executed their launch command (prompt visible, no half-typed command text).

- [ ] **Step 5: Live e2e — real agents + --wait**

1. `herdrplus.exe team spawn live --agents ws1=claude,ws2=grok --wait --timeout 120`
   Expected: both CLIs boot in their panes; `--wait` reports `ready: ws1`, `ready: ws2`, exit 0. Note whether grok is detected (detect module knows claude/codex/gemini/pi — if grok stays `unknown`, it should have been listed as "not detectable (skipped)" instead of blocking; record actual behavior).
2. `herdrplus.exe workspace close` the team when done.
3. Record BOTH e2e transcripts (commands + outputs) in the PR description.

- [ ] **Step 6: Update graph + commit**

If `graphify-out/graph.json` exists at repo root: run `graphify update .`
```bash
git add -A
git commit -m "docs: changelog for team spawn; e2e verified"
```

---

## Post-plan gates (not tasks — session workflow)

1. Internal whole-branch review (superpowers:requesting-code-review).
2. **Sol56 external review of the implementation diff** (two-reviewer gate; see memory `external-review-sol56`): `codex exec -s read-only -m gpt-5.6-sol -c model_reasoning_effort="high"` with the diff + spec + this plan; severity-tiered findings + merge verdict. Fold surviving findings, re-verify live e2e for critical ones.
3. Also fold in any late findings from the Sol56 SPEC review dispatched 2026-07-18 (was still running when implementation started).
4. PR to `main` per house style (feature branch, e2e transcripts in description).
