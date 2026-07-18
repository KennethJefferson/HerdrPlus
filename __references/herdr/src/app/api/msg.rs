use crate::api::schema::{
    MsgAckParams, MsgGroupJoinParams, MsgGroupLeaveParams, MsgInfo, MsgListParams,
    MsgSendParams, MsgSendResult, MsgWhoGroupInfo, MsgWhoPaneInfo,
    ResponseResult,
};
use crate::app::App;
use crate::app::api::responses::{encode_error, encode_success};

impl App {
    pub(super) fn handle_msg_send(&mut self, id: String, params: MsgSendParams) -> String {
        let target = match crate::msg::MsgBus::parse_target(&params.target) {
            Ok(t) => t,
            Err(e) => {
                return encode_error(id, "invalid_target", format!("failed to parse target: {e:?}"));
            }
        };

        let mut directory = Vec::new();
        for (ws_idx, ws) in self.state.workspaces.iter().enumerate() {
            let workspace_id = self.public_workspace_id(ws_idx);
            for &pane_id in ws.public_pane_numbers.keys() {
                if let Some(pane) = ws.pane_state(pane_id) {
                    if let Some(terminal) = self.state.terminals.get(&pane.attached_terminal_id) {
                        let manual_label = terminal.manual_label.clone().unwrap_or_default();
                        if let Some(public_pane_id) = self.public_pane_id(ws_idx, pane_id) {
                            directory.push((workspace_id.clone(), public_pane_id, manual_label));
                        }
                    }
                }
            }
        }

        let sender_workspace = if let Some(ref sender_id) = params.sender_pane_id {
            if let Some((ws_idx, _)) = self.parse_pane_id(sender_id) {
                Some(self.public_workspace_id(ws_idx))
            } else {
                None
            }
        } else {
            None
        };

        let mut resolved = match self.state.msg_bus.resolve(&target, sender_workspace.as_deref(), &directory) {
            Ok(r) => r,
            Err(e) => {
                let msg = match e {
                    crate::msg::ResolveError::NotFound => "target not found".to_string(),
                    crate::msg::ResolveError::Ambiguous(candidates) => {
                        format!("target is ambiguous, matches: {:?}", candidates)
                    }
                    crate::msg::ResolveError::Unaddressable(expr) => {
                        format!("target is unaddressable: {}", expr)
                    }
                    crate::msg::ResolveError::EmptyGroup(g) => {
                        format!("group {} is empty or unknown", g)
                    }
                };
                return encode_error(id, "invalid_target", msg);
            }
        };

        if matches!(target, crate::msg::MsgTarget::All) {
            if let Some(ref sender_id) = params.sender_pane_id {
                resolved.retain(|pane_id| pane_id != sender_id);
            }
        }

        let from_label = if let Some(ref sender_id) = params.sender_pane_id {
            if let Some((ws_idx, pane_id)) = self.parse_pane_id(sender_id) {
                if let Some(pane) = self.state.workspaces.get(ws_idx).and_then(|ws| ws.pane_state(pane_id)) {
                    if let Some(terminal) = self.state.terminals.get(&pane.attached_terminal_id) {
                        terminal.manual_label.clone().unwrap_or_default()
                    } else {
                        "".to_string()
                    }
                } else {
                    "".to_string()
                }
            } else {
                "".to_string()
            }
        } else {
            "external".to_string()
        };

        let timestamp = time::OffsetDateTime::now_utc()
            .format(&time::format_description::well_known::Rfc3339)
            .unwrap_or_else(|_| "2026-07-18T00:00:00Z".to_string());

        let mut first_seq = 0;
        let mut first = true;
        for pane_id in &resolved {
            let seq = self.state.msg_bus.next_seq(pane_id);
            if first {
                first_seq = seq;
                first = false;
            }
            let msg = crate::msg::StoredMsg {
                seq,
                from_pane_id: params.sender_pane_id.clone(),
                from_workspace_id: sender_workspace.clone(),
                from_label: from_label.clone(),
                to: params.target.clone(),
                body: params.body.clone(),
                timestamp: timestamp.clone(),
            };
            if let Err(e) = self.state.msg_bus.deliver(pane_id, msg) {
                let code = match e {
                    crate::msg::MsgError::MessageTooLarge => "message_too_large",
                    crate::msg::MsgError::InvalidGroupName(_) => "invalid_group_name",
                };
                return encode_error(id, code, "message delivery failed");
            }

            self.emit_event(crate::api::schema::EventEnvelope {
                event: crate::api::schema::EventKind::MsgReceived,
                data: crate::api::schema::EventData::PaneMsgReceived {
                    pane_id: pane_id.clone(),
                    seq,
                },
            });
        }

        let res = MsgSendResult {
            delivered_to: resolved,
            message: MsgInfo {
                seq: first_seq,
                from_pane_id: params.sender_pane_id,
                from_workspace_id: sender_workspace,
                from_label,
                to: params.target,
                body: params.body,
                timestamp,
            },
        };
        encode_success(id, ResponseResult::MsgSend {
            delivered_to: res.delivered_to,
            message: res.message,
        })
    }

    pub(super) fn handle_msg_list(&mut self, id: String, params: MsgListParams) -> String {
        if self.parse_pane_id(&params.pane_id).is_none() {
            return encode_error(id, "pane_not_found", format!("pane {} not found", params.pane_id));
        }

        let include_read = params.include_read.unwrap_or(true);
        let (messages, unread, dropped, ack_seq) = self.state.msg_bus.list(
            &params.pane_id,
            params.after_seq,
            include_read,
        );

        let messages_info = messages
            .into_iter()
            .map(|m| MsgInfo {
                seq: m.seq,
                from_pane_id: m.from_pane_id,
                from_workspace_id: m.from_workspace_id,
                from_label: m.from_label,
                to: m.to,
                body: m.body,
                timestamp: m.timestamp,
            })
            .collect();

        encode_success(
            id,
            ResponseResult::MsgList {
                messages: messages_info,
                unread,
                dropped,
                ack_seq,
            },
        )
    }

    pub(super) fn handle_msg_ack(&mut self, id: String, params: MsgAckParams) -> String {
        if self.parse_pane_id(&params.pane_id).is_none() {
            return encode_error(id, "pane_not_found", format!("pane {} not found", params.pane_id));
        }

        let ack_seq = self.state.msg_bus.ack(&params.pane_id, params.up_to_seq);

        encode_success(id, ResponseResult::MsgAck { ack_seq })
    }

    pub(super) fn handle_msg_group_join(&mut self, id: String, params: MsgGroupJoinParams) -> String {
        if self.parse_pane_id(&params.pane_id).is_none() {
            return encode_error(id, "pane_not_found", format!("pane {} not found", params.pane_id));
        }

        match self.state.msg_bus.group_join(&params.pane_id, &params.group) {
            Ok(groups) => encode_success(id, ResponseResult::MsgGroup { groups }),
            Err(e) => {
                let code = match e {
                    crate::msg::MsgError::MessageTooLarge => "message_too_large",
                    crate::msg::MsgError::InvalidGroupName(_) => "invalid_group_name",
                };
                encode_error(id, code, format!("failed to join group: {e:?}"))
            }
        }
    }

    pub(super) fn handle_msg_group_leave(&mut self, id: String, params: MsgGroupLeaveParams) -> String {
        if self.parse_pane_id(&params.pane_id).is_none() {
            return encode_error(id, "pane_not_found", format!("pane {} not found", params.pane_id));
        }

        match self.state.msg_bus.group_leave(&params.pane_id, &params.group) {
            Ok(groups) => encode_success(id, ResponseResult::MsgGroup { groups }),
            Err(e) => {
                let code = match e {
                    crate::msg::MsgError::MessageTooLarge => "message_too_large",
                    crate::msg::MsgError::InvalidGroupName(_) => "invalid_group_name",
                };
                encode_error(id, code, format!("failed to leave group: {e:?}"))
            }
        }
    }

    pub(super) fn handle_msg_who(&mut self, id: String) -> String {
        let mut panes = Vec::new();
        let mut all_groups = std::collections::HashSet::new();

        for (ws_idx, ws) in self.state.workspaces.iter().enumerate() {
            let workspace_id = self.public_workspace_id(ws_idx);
            for &pane_id in ws.public_pane_numbers.keys() {
                if let Some(pane) = ws.pane_state(pane_id) {
                    if let Some(terminal) = self.state.terminals.get(&pane.attached_terminal_id) {
                        let label = terminal.manual_label.clone().unwrap_or_default();
                        if let Some(public_pane_id) = self.public_pane_id(ws_idx, pane_id) {
                            let groups = self.state.msg_bus.groups_of(&public_pane_id);
                            for g in &groups {
                                all_groups.insert(g.clone());
                            }
                            let unread = self.state.msg_bus.unread(&public_pane_id);
                            panes.push(MsgWhoPaneInfo {
                                pane_id: public_pane_id,
                                workspace_id: workspace_id.clone(),
                                label,
                                groups,
                                unread,
                            });
                        }
                    }
                }
            }
        }

        panes.sort_by(|a, b| a.pane_id.cmp(&b.pane_id));

        let mut groups = Vec::new();
        for gname in all_groups {
            let mut members = self.state.msg_bus.group_members(&gname);
            members.sort();
            groups.push(MsgWhoGroupInfo {
                name: gname,
                members,
            });
        }
        groups.sort_by(|a, b| a.name.cmp(&b.name));

        encode_success(id, ResponseResult::MsgWho { panes, groups })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Config, ShellModeConfig};
    use super::super::test_support::exiting_test_command;
    use crate::workspace::Workspace;
    use crate::api::schema::{EventData, SuccessResponse};
    use ratatui::layout::Direction;

    fn app_with_workspace() -> App {
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
        app.state.workspaces = vec![Workspace::test_new("layout")];
        app.state.active = Some(0);
        app.state.selected = 0;
        app.state.ensure_test_terminals();
        app
    }

    #[test]
    fn test_send_list_ack_round_trip() {
        let mut app = app_with_workspace();
        let root = app.state.workspaces[0].tabs[0].root_pane;
        let right = app.state.workspaces[0].test_split(Direction::Horizontal);
        app.state.ensure_test_terminals();

        let root_term_id = app.state.workspaces[0].terminal_id(root).cloned().unwrap();
        let right_term_id = app.state.workspaces[0].terminal_id(right).cloned().unwrap();

        app.state.terminals.get_mut(&root_term_id).unwrap().set_manual_label("orchestrator".to_string());
        app.state.terminals.get_mut(&right_term_id).unwrap().set_manual_label("worker-1".to_string());

        let p1_id = app.public_pane_id(0, root).unwrap();
        let p2_id = app.public_pane_id(0, right).unwrap();

        // 1. Send message from p1 to p2 (worker-1)
        let send_res = app.handle_msg_send(
            "req_1".into(),
            MsgSendParams {
                target: "worker-1".into(),
                body: "hello worker".into(),
                sender_pane_id: Some(p1_id.clone()),
            },
        );
        assert!(send_res.contains("msg_send"));
        let success: SuccessResponse = serde_json::from_str(&send_res).unwrap();
        let ResponseResult::MsgSend { delivered_to, message } = success.result else {
            panic!("Expected MsgSend response");
        };
        assert_eq!(delivered_to, vec![p2_id.clone()]);
        assert_eq!(message.from_pane_id, Some(p1_id.clone()));
        assert_eq!(message.from_label, "orchestrator");
        assert_eq!(message.body, "hello worker");
        assert_eq!(message.seq, 1);

        // Verify MsgReceived event
        let events = app.event_hub.events_after(0);
        let event = events.last().expect("expected event");
        assert_eq!(event.1.event, crate::api::schema::EventKind::MsgReceived);
        let EventData::PaneMsgReceived { pane_id, seq } = &event.1.data else {
            panic!("expected PaneMsgReceived event data");
        };
        assert_eq!(pane_id, &p2_id);
        assert_eq!(*seq, 1);

        // 2. List messages for p2
        let list_res = app.handle_msg_list(
            "req_2".into(),
            MsgListParams {
                pane_id: p2_id.clone(),
                after_seq: None,
                include_read: Some(true),
            },
        );
        let success: SuccessResponse = serde_json::from_str(&list_res).unwrap();
        let ResponseResult::MsgList { messages, unread, dropped, ack_seq } = success.result else {
            panic!("Expected MsgList response");
        };
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].body, "hello worker");
        assert_eq!(unread, 1);
        assert_eq!(dropped, 0);
        assert_eq!(ack_seq, 0);

        // 3. Ack messages up to seq 1
        let ack_res = app.handle_msg_ack(
            "req_3".into(),
            MsgAckParams {
                pane_id: p2_id.clone(),
                up_to_seq: 1,
            },
        );
        let success: SuccessResponse = serde_json::from_str(&ack_res).unwrap();
        let ResponseResult::MsgAck { ack_seq: new_ack_seq } = success.result else {
            panic!("Expected MsgAck response");
        };
        assert_eq!(new_ack_seq, 1);

        // 4. List again with include_read: false
        let list_res_2 = app.handle_msg_list(
            "req_4".into(),
            MsgListParams {
                pane_id: p2_id.clone(),
                after_seq: None,
                include_read: Some(false),
            },
        );
        let success: SuccessResponse = serde_json::from_str(&list_res_2).unwrap();
        let ResponseResult::MsgList { messages: messages_2, unread: unread_2, .. } = success.result else {
            panic!("Expected MsgList response");
        };
        assert_eq!(messages_2.len(), 0);
        assert_eq!(unread_2, 0);
    }

    #[test]
    fn test_ambiguity_error() {
        let mut app = app_with_workspace();
        let _root = app.state.workspaces[0].tabs[0].root_pane;
        let right = app.state.workspaces[0].test_split(Direction::Horizontal);
        app.state.ensure_test_terminals();

        // Workspace 1
        app.state.workspaces.push(Workspace::test_new("other_workspace"));
        let root2 = app.state.workspaces[1].tabs[0].root_pane;
        app.state.ensure_test_terminals();

        let term_right = app.state.workspaces[0].terminal_id(right).cloned().unwrap();
        let term_root2 = app.state.workspaces[1].terminal_id(root2).cloned().unwrap();

        app.state.terminals.get_mut(&term_right).unwrap().set_manual_label("worker-1".to_string());
        app.state.terminals.get_mut(&term_root2).unwrap().set_manual_label("worker-1".to_string());

        // Send message from external to "worker-1" - should be ambiguous
        let send_res = app.handle_msg_send(
            "req_1".into(),
            MsgSendParams {
                target: "worker-1".into(),
                body: "hello worker".into(),
                sender_pane_id: None,
            },
        );
        assert!(send_res.contains("invalid_target"));
        assert!(send_res.contains("target is ambiguous"));
    }

    #[test]
    fn test_external_vs_pane_sender_resolution() {
        let mut app = app_with_workspace();
        let root = app.state.workspaces[0].tabs[0].root_pane;
        let right = app.state.workspaces[0].test_split(Direction::Horizontal);
        app.state.ensure_test_terminals();

        // Workspace 1
        app.state.workspaces.push(Workspace::test_new("other_workspace"));
        let root2 = app.state.workspaces[1].tabs[0].root_pane;
        app.state.ensure_test_terminals();

        let term_right = app.state.workspaces[0].terminal_id(right).cloned().unwrap();
        let term_root2 = app.state.workspaces[1].terminal_id(root2).cloned().unwrap();

        app.state.terminals.get_mut(&term_right).unwrap().set_manual_label("worker-1".to_string());
        app.state.terminals.get_mut(&term_root2).unwrap().set_manual_label("worker-1".to_string());

        let p1_id = app.public_pane_id(0, root).unwrap();
        let p2_id = app.public_pane_id(0, right).unwrap();

        // Send message from p1 (workspace 0) to "worker-1" - should resolve to workspace 0's worker-1 locally
        let send_res = app.handle_msg_send(
            "req_1".into(),
            MsgSendParams {
                target: "worker-1".into(),
                body: "hello local worker".into(),
                sender_pane_id: Some(p1_id.clone()),
            },
        );
        assert!(send_res.contains("msg_send"));
        let success: SuccessResponse = serde_json::from_str(&send_res).unwrap();
        let ResponseResult::MsgSend { delivered_to, .. } = success.result else {
            panic!("Expected MsgSend response");
        };
        assert_eq!(delivered_to, vec![p2_id.clone()]);
    }

    #[test]
    fn test_all_excludes_sender() {
        let mut app = app_with_workspace();
        let root = app.state.workspaces[0].tabs[0].root_pane;
        let right = app.state.workspaces[0].test_split(Direction::Horizontal);
        app.state.ensure_test_terminals();

        let p1_id = app.public_pane_id(0, root).unwrap();
        let p2_id = app.public_pane_id(0, right).unwrap();

        // Send to @all from p1
        let send_res = app.handle_msg_send(
            "req_1".into(),
            MsgSendParams {
                target: "@all".into(),
                body: "broadcast".into(),
                sender_pane_id: Some(p1_id.clone()),
            },
        );
        let success: SuccessResponse = serde_json::from_str(&send_res).unwrap();
        let ResponseResult::MsgSend { delivered_to, .. } = success.result else {
            panic!("Expected MsgSend response");
        };
        // delivered_to should include p2, but exclude p1
        assert!(delivered_to.contains(&p2_id));
        assert!(!delivered_to.contains(&p1_id));
    }

    #[test]
    fn test_oversized_body() {
        let mut app = app_with_workspace();
        let _root = app.state.workspaces[0].tabs[0].root_pane;
        app.state.ensure_test_terminals();

        let body = "a".repeat(65 * 1024); // 65 KiB > 64 KiB cap
        let send_res = app.handle_msg_send(
            "req_1".into(),
            MsgSendParams {
                target: "@all".into(),
                body,
                sender_pane_id: None,
            },
        );
        assert!(send_res.contains("message_too_large"));
    }

    #[test]
    fn test_msg_who_and_groups() {
        let mut app = app_with_workspace();
        let root = app.state.workspaces[0].tabs[0].root_pane;
        let right = app.state.workspaces[0].test_split(Direction::Horizontal);
        app.state.ensure_test_terminals();

        let root_term_id = app.state.workspaces[0].terminal_id(root).cloned().unwrap();
        let right_term_id = app.state.workspaces[0].terminal_id(right).cloned().unwrap();

        app.state.terminals.get_mut(&root_term_id).unwrap().set_manual_label("orchestrator".to_string());
        app.state.terminals.get_mut(&right_term_id).unwrap().set_manual_label("worker-1".to_string());

        let p1_id = app.public_pane_id(0, root).unwrap();
        let p2_id = app.public_pane_id(0, right).unwrap();

        // Join p2 to group "workers"
        let join_res = app.handle_msg_group_join(
            "req_1".into(),
            MsgGroupJoinParams {
                pane_id: p2_id.clone(),
                group: "workers".into(),
            },
        );
        assert!(join_res.contains("msg_group"));

        // Who query
        let who_res = app.handle_msg_who("req_2".into());
        let success: SuccessResponse = serde_json::from_str(&who_res).unwrap();
        let ResponseResult::MsgWho { panes, groups } = success.result else {
            panic!("Expected MsgWho response");
        };

        // Assert panes
        assert_eq!(panes.len(), 2);
        assert_eq!(panes[0].pane_id, p1_id);
        assert_eq!(panes[0].groups.len(), 0);
        assert_eq!(panes[1].pane_id, p2_id);
        assert_eq!(panes[1].groups, vec!["workers".to_string()]);

        // Assert groups
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].name, "workers");
        assert_eq!(groups[0].members, vec![p2_id.clone()]);

        // Leave group
        let leave_res = app.handle_msg_group_leave(
            "req_3".into(),
            MsgGroupLeaveParams {
                pane_id: p2_id.clone(),
                group: "workers".into(),
            },
        );
        assert!(leave_res.contains("msg_group"));
    }
}
