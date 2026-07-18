use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct MsgInfo {
    pub seq: u64,
    pub from_pane_id: Option<String>,
    pub from_workspace_id: Option<String>,
    pub from_label: String,
    pub to: String,
    pub body: String,
    pub timestamp: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct MsgSendParams {
    pub target: String,
    pub body: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sender_pane_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct MsgSendResult {
    pub delivered_to: Vec<String>,
    /// `None` for zero-recipient sends (empty fan-out): nothing was
    /// delivered, so there is no message (and no meaningful seq).
    pub message: Option<MsgInfo>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct MsgListParams {
    pub pane_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub after_seq: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub include_read: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct MsgListResult {
    pub messages: Vec<MsgInfo>,
    pub unread: u64,
    pub dropped: u64,
    pub ack_seq: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct MsgAckParams {
    pub pane_id: String,
    pub up_to_seq: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct MsgAckResult {
    pub ack_seq: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct MsgGroupJoinParams {
    pub pane_id: String,
    pub group: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct MsgGroupLeaveParams {
    pub pane_id: String,
    pub group: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct MsgGroupResult {
    pub groups: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct MsgWhoPaneInfo {
    pub pane_id: String,
    pub workspace_id: String,
    pub label: String,
    pub groups: Vec<String>,
    pub unread: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct MsgWhoGroupInfo {
    pub name: String,
    pub members: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct MsgWhoResult {
    pub panes: Vec<MsgWhoPaneInfo>,
    pub groups: Vec<MsgWhoGroupInfo>,
}
