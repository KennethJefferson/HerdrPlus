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
        if params.name == "all" {
            return encode_error(
                id,
                "invalid_team_name",
                "team name \"all\" is reserved (@all is the broadcast target)",
            );
        }

        const MAX_TEAM_PANES: usize = 24;
        let total = params.entries.len() + usize::from(params.with_orch);
        if total > MAX_TEAM_PANES {
            return encode_error(
                id,
                "team_too_large",
                format!("team of {total} panes exceeds the {MAX_TEAM_PANES}-pane limit"),
            );
        }

        let mut labels: Vec<String> = Vec::with_capacity(params.entries.len());
        let mut counters: HashMap<String, usize> = HashMap::new();
        for entry in &params.entries {
            let label = match &entry.label {
                Some(label) => label.trim().to_string(),
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
            if crate::msg::is_public_pane_id(&label) {
                return encode_error(
                    id,
                    "invalid_label",
                    format!("label {label:?} matches pane-id syntax and would be unaddressable"),
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
        let saved_active = self.state.active;
        let saved_selected = self.state.selected;

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
                self.state.active = saved_active;
                self.state.selected = saved_selected;
                return encode_error(
                    id,
                    "team_spawn_failed",
                    format!("{} failed: {} (workspace rolled back)", $step, $err),
                );
            }};
        }

        // ---- split to a rough row-major grid; balance() equalizes areas after ----
        let cols = (total as f64).sqrt().ceil() as usize;
        let mut row_seeds: Vec<String> = vec![root_pane_id.clone()];
        let rows = total.div_ceil(cols);
        for _ in 1..rows {
            let seed = row_seeds.last().cloned().unwrap();
            match self.split_team_pane(&id, &seed, SplitDirection::Down, params.cwd.as_deref()) {
                Ok(new_id) => row_seeds.push(new_id),
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
                    Ok(new_id) => row_panes.push(new_id),
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::schema::{
        ResponseResult, SuccessResponse, TeamSpawnEntry, TeamSpawnParams,
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

    #[tokio::test]
    async fn spawn_happy_path_creates_panes_labels_and_group() {
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
        let who = app.handle_msg_who("req_2".into());
        let success: SuccessResponse = serde_json::from_str(&who).unwrap();
        let ResponseResult::MsgWho { groups, .. } = success.result else { panic!() };
        let review = groups.iter().find(|g| g.name == "review").expect("group exists");
        assert_eq!(review.members.len(), 3);
    }

    #[tokio::test]
    async fn spawn_with_orch_adds_orch_pane_in_group() {
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

    #[tokio::test]
    async fn rollback_leaves_no_workspace_or_group_residue() {
        let mut app = app();
        let before = app.state.workspaces.len();
        let res = app.handle_team_spawn_with_fault(
            "req_1".into(),
            spawn_params("doomed", vec![entry(None, "pwsh"), entry(None, "pwsh")]),
            Some(TeamSpawnFault::AfterSplits),
        );
        assert!(res.contains("team_spawn_failed"), "got: {res}");
        assert_eq!(app.state.workspaces.len(), before, "workspace rolled back");
        assert!(app.state.msg_bus.group_members("doomed").is_empty(), "group purged");
    }

    #[test]
    fn reserved_name_all_refused() {
        let mut app = app();
        let before = app.state.workspaces.len();
        let res = app.handle_team_spawn(
            "req_1".into(),
            spawn_params("all", vec![entry(None, "pwsh")]),
        );
        assert!(res.contains("invalid_team_name"), "got: {res}");
        assert_eq!(app.state.workspaces.len(), before);
    }

    #[test]
    fn pane_id_shaped_label_refused() {
        let mut app = app();
        let before = app.state.workspaces.len();
        let res = app.handle_team_spawn(
            "req_1".into(),
            spawn_params("t", vec![entry(Some("w1:p2"), "pwsh")]),
        );
        assert!(res.contains("invalid_label"), "got: {res}");
        assert_eq!(app.state.workspaces.len(), before);
    }

    #[test]
    fn labels_trimmed_before_dup_check() {
        let mut app = app();
        let res = app.handle_team_spawn(
            "req_1".into(),
            spawn_params("t", vec![entry(Some("a"), "pwsh"), entry(Some("a "), "pwsh")]),
        );
        assert!(res.contains("duplicate_label"), "got: {res}");
    }

    #[test]
    fn oversized_roster_refused() {
        let mut app = app();
        let entries = (0..25).map(|_| entry(None, "pwsh")).collect();
        let res = app.handle_team_spawn(
            "req_1".into(),
            spawn_params("t", entries),
        );
        assert!(res.contains("team_too_large"), "got: {res}");
    }

    #[tokio::test]
    async fn rollback_restores_focus() {
        let mut app = app();
        app.state.active = Some(0);
        app.state.selected = 0;
        let res = app.handle_team_spawn_with_fault(
            "req_1".into(),
            spawn_params("doomed", vec![entry(None, "pwsh"), entry(None, "pwsh")]),
            Some(TeamSpawnFault::AfterSplits),
        );
        assert!(res.contains("team_spawn_failed"), "got: {res}");
        assert_eq!(app.state.active, Some(0));
        assert_eq!(app.state.selected, 0);
    }
}
