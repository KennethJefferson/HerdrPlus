//! Server-side message bus: per-pane inboxes, label addressing, groups.
//!
//! Pure state module — no PTY, sockets, or async. WS2 owns the API layer that
//! stamps timestamps, builds the live directory, and filters `@all` senders.

use std::collections::{HashMap, HashSet, VecDeque};

/// Max messages retained per inbox (oldest dropped first).
const MAX_INBOX_MESSAGES: usize = 500;
/// Max total body bytes retained per inbox (oldest dropped first).
const MAX_INBOX_BODY_BYTES: usize = 4 * 1024 * 1024;
/// Max body size for a single message. Public so the API layer can reject
/// oversized bodies up front (before resolution), independent of fan-out.
pub const MAX_MESSAGE_BODY_BYTES: usize = 64 * 1024;

/// One delivered message stored in a pane inbox.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredMsg {
    pub seq: u64,
    pub from_pane_id: Option<String>,
    pub from_workspace_id: Option<String>,
    pub from_label: String,
    pub to: String,
    pub body: String,
    /// RFC 3339 UTC, caller-supplied (WS2 stamps server clock).
    pub timestamp: String,
}

/// Parsed form of a `msg.send` target expression.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MsgTarget {
    PaneId(String),
    Qualified { workspace_id: String, label: String },
    Label(String),
    Group(String),
    All,
}

/// Resolution failure for a target expression or directory lookup.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolveError {
    NotFound,
    /// Candidates as `(workspace_id, label, pane_id)`.
    Ambiguous(Vec<(String, String, String)>),
    Unaddressable(String),
    EmptyGroup(String),
}

/// Delivery / group-mutation errors.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MsgError {
    MessageTooLarge,
    InvalidGroupName(String),
}

struct Inbox {
    messages: VecDeque<StoredMsg>,
    /// Seq that will be assigned to the next delivered message (starts at 1).
    next_seq: u64,
    /// Highest acked seq; messages with `seq > ack_seq` are unread.
    ack_seq: u64,
    dropped: u64,
    total_body_bytes: usize,
}

impl Inbox {
    fn new() -> Self {
        Self {
            messages: VecDeque::new(),
            next_seq: 1,
            ack_seq: 0,
            dropped: 0,
            total_body_bytes: 0,
        }
    }

    fn unread(&self) -> u64 {
        self.messages
            .iter()
            .filter(|m| m.seq > self.ack_seq)
            .count() as u64
    }

    fn newest_seq(&self) -> u64 {
        self.messages.back().map(|m| m.seq).unwrap_or(0)
    }

    fn drop_oldest(&mut self) {
        if let Some(old) = self.messages.pop_front() {
            self.total_body_bytes = self.total_body_bytes.saturating_sub(old.body.len());
            self.dropped = self.dropped.saturating_add(1);
        }
    }

    fn enforce_caps(&mut self) {
        while self.messages.len() > MAX_INBOX_MESSAGES
            || self.total_body_bytes > MAX_INBOX_BODY_BYTES
        {
            if self.messages.is_empty() {
                break;
            }
            self.drop_oldest();
        }
    }
}

/// In-memory message bus owned by the server.
///
/// `#all` resolution returns every pane id in the provided directory. The API
/// layer (WS2) is responsible for excluding the sender's own pane when the
/// target is `@all` (spec rule 5). This module does not take a sender pane id
/// so pure resolution stays free of send-time policy.
pub struct MsgBus {
    inboxes: HashMap<String, Inbox>,
    /// pane_id → group names
    pane_groups: HashMap<String, HashSet<String>>,
    /// group name → member pane ids
    group_members: HashMap<String, HashSet<String>>,
}

impl MsgBus {
    pub fn new() -> Self {
        Self {
            inboxes: HashMap::new(),
            pane_groups: HashMap::new(),
            group_members: HashMap::new(),
        }
    }

    /// Parse a target expression into a [`MsgTarget`].
    ///
    /// Forms:
    /// - `@all` → [`MsgTarget::All`]
    /// - `@name` → [`MsgTarget::Group`]
    /// - canonical public pane id `w<ENC>:p<ENC>` → [`MsgTarget::PaneId`]
    ///   (e.g. `w1:p3`; `<ENC>` is the public-id alphabet — see
    ///   [`crate::workspace::decode_public_number`])
    /// - `workspace_id/label` → [`MsgTarget::Qualified`]
    /// - else bare [`MsgTarget::Label`]
    ///
    /// A `:` alone does NOT make a pane id: labels like `worker:api` are
    /// legal and route as labels. Only the exact public-pane-id grammar is
    /// classified as [`MsgTarget::PaneId`].
    ///
    /// Labels that are empty, contain `/`, or begin with `@` are unaddressable.
    pub fn parse_target(expr: &str) -> Result<MsgTarget, ResolveError> {
        if expr.is_empty() {
            return Err(ResolveError::Unaddressable(expr.to_string()));
        }

        if let Some(rest) = expr.strip_prefix('@') {
            if rest.is_empty() || rest.contains('/') {
                return Err(ResolveError::Unaddressable(expr.to_string()));
            }
            if rest == "all" {
                return Ok(MsgTarget::All);
            }
            return Ok(MsgTarget::Group(rest.to_string()));
        }

        if is_public_pane_id(expr) {
            return Ok(MsgTarget::PaneId(expr.to_string()));
        }

        if let Some((workspace_id, label)) = expr.split_once('/') {
            if workspace_id.is_empty()
                || label.is_empty()
                || label.contains('/')
                || label.starts_with('@')
            {
                return Err(ResolveError::Unaddressable(expr.to_string()));
            }
            return Ok(MsgTarget::Qualified {
                workspace_id: workspace_id.to_string(),
                label: label.to_string(),
            });
        }

        Ok(MsgTarget::Label(expr.to_string()))
    }

    /// Resolve a target to zero-or-more pane ids.
    ///
    /// `directory` entries are `(workspace_id, pane_id, manual_label)`.
    /// Matching is exact and case-sensitive against `manual_label`.
    ///
    /// Rules (spec §Resolution):
    /// 1. Pane id → that pane if present in the directory.
    /// 2. `workspace_id/label` → that workspace only.
    /// 3. Bare label → sender workspace first (`sender_workspace: None` skips
    ///    local phase, i.e. external senders); then global; refuse ambiguity.
    /// 4. `@group` → current members; empty/unknown → [`ResolveError::EmptyGroup`].
    /// 5. `@all` → every directory pane id (WS2 excludes the sender).
    /// 6. Case-sensitive exact match; unaddressable labels rejected at parse.
    pub fn resolve(
        &self,
        target: &MsgTarget,
        sender_workspace: Option<&str>,
        directory: &[(String, String, String)],
    ) -> Result<Vec<String>, ResolveError> {
        match target {
            MsgTarget::PaneId(pane_id) => {
                if directory.iter().any(|(_, id, _)| id == pane_id) {
                    Ok(vec![pane_id.clone()])
                } else {
                    Err(ResolveError::NotFound)
                }
            }
            MsgTarget::Qualified {
                workspace_id,
                label,
            } => {
                let matches: Vec<_> = directory
                    .iter()
                    .filter(|(ws, _, lbl)| ws == workspace_id && lbl == label)
                    .map(|(ws, id, lbl)| (ws.clone(), lbl.clone(), id.clone()))
                    .collect();
                match matches.len() {
                    0 => Err(ResolveError::NotFound),
                    1 => Ok(vec![matches[0].2.clone()]),
                    _ => Err(ResolveError::Ambiguous(matches)),
                }
            }
            MsgTarget::Label(label) => {
                if let Some(ws) = sender_workspace {
                    let local: Vec<_> = directory
                        .iter()
                        .filter(|(w, _, lbl)| w.as_str() == ws && lbl == label)
                        .map(|(w, id, lbl)| (w.clone(), lbl.clone(), id.clone()))
                        .collect();
                    match local.len() {
                        1 => return Ok(vec![local[0].2.clone()]),
                        n if n > 1 => return Err(ResolveError::Ambiguous(local)),
                        _ => {}
                    }
                }

                let global: Vec<_> = directory
                    .iter()
                    .filter(|(_, _, lbl)| lbl == label)
                    .map(|(w, id, lbl)| (w.clone(), lbl.clone(), id.clone()))
                    .collect();
                match global.len() {
                    0 => Err(ResolveError::NotFound),
                    1 => Ok(vec![global[0].2.clone()]),
                    _ => Err(ResolveError::Ambiguous(global)),
                }
            }
            MsgTarget::Group(name) => {
                let members = self.group_members.get(name);
                match members {
                    None => Err(ResolveError::EmptyGroup(name.clone())),
                    Some(set) if set.is_empty() => Err(ResolveError::EmptyGroup(name.clone())),
                    Some(set) => {
                        let mut ids: Vec<String> = set.iter().cloned().collect();
                        ids.sort();
                        Ok(ids)
                    }
                }
            }
            MsgTarget::All => {
                // Spec rule 5: every pane except the sender. Sender exclusion is
                // applied by WS2 so this pure resolver stays directory-only.
                let mut ids: Vec<String> = directory.iter().map(|(_, id, _)| id.clone()).collect();
                ids.sort();
                ids.dedup();
                Ok(ids)
            }
        }
    }

    /// Deliver `msg` into `pane_id`'s inbox.
    ///
    /// Assigns `seq` internally (incoming `msg.seq` is ignored/overwritten).
    /// Enforces 64 KiB body limit and 500-message / 4 MiB inbox caps.
    pub fn deliver(&mut self, pane_id: &str, mut msg: StoredMsg) -> Result<(), MsgError> {
        if msg.body.len() > MAX_MESSAGE_BODY_BYTES {
            return Err(MsgError::MessageTooLarge);
        }

        let inbox = self
            .inboxes
            .entry(pane_id.to_string())
            .or_insert_with(Inbox::new);

        msg.seq = inbox.next_seq;
        inbox.next_seq = inbox.next_seq.saturating_add(1);

        inbox.total_body_bytes = inbox.total_body_bytes.saturating_add(msg.body.len());
        inbox.messages.push_back(msg);
        inbox.enforce_caps();
        Ok(())
    }

    /// Pure list of messages for a pane. Never moves the ack cursor.
    ///
    /// Returns `(messages, unread, dropped, ack_seq)`.
    /// - `after_seq: Some(n)` keeps only messages with `seq > n`
    /// - `include_read: false` keeps only messages with `seq > ack_seq`
    pub fn list(
        &self,
        pane_id: &str,
        after_seq: Option<u64>,
        include_read: bool,
    ) -> (Vec<StoredMsg>, u64, u64, u64) {
        let Some(inbox) = self.inboxes.get(pane_id) else {
            return (Vec::new(), 0, 0, 0);
        };

        let messages: Vec<StoredMsg> = inbox
            .messages
            .iter()
            .filter(|m| after_seq.map(|n| m.seq > n).unwrap_or(true))
            .filter(|m| include_read || m.seq > inbox.ack_seq)
            .cloned()
            .collect();

        (messages, inbox.unread(), inbox.dropped, inbox.ack_seq)
    }

    /// Advance the ack cursor to `up_to_seq` (clamped to newest).
    /// Behind-cursor values are a no-op. Returns the new `ack_seq`.
    pub fn ack(&mut self, pane_id: &str, up_to_seq: u64) -> u64 {
        let Some(inbox) = self.inboxes.get_mut(pane_id) else {
            return 0;
        };
        let newest = inbox.newest_seq();
        let clamped = up_to_seq.min(newest);
        if clamped > inbox.ack_seq {
            inbox.ack_seq = clamped;
        }
        inbox.ack_seq
    }

    /// Unread count for a pane. Returns 0 for unknown panes (WS3 badge path).
    pub fn unread(&self, pane_id: &str) -> u64 {
        self.inboxes.get(pane_id).map(|i| i.unread()).unwrap_or(0)
    }

    /// Seq the next delivered message to this pane will receive.
    pub fn next_seq(&self, pane_id: &str) -> u64 {
        self.inboxes.get(pane_id).map(|i| i.next_seq).unwrap_or(1)
    }

    /// Join `pane_id` to `group`. Creates the group on first join.
    /// Returns the pane's group list after the join.
    pub fn group_join(&mut self, pane_id: &str, group: &str) -> Result<Vec<String>, MsgError> {
        validate_group_name(group)?;
        self.group_members
            .entry(group.to_string())
            .or_default()
            .insert(pane_id.to_string());
        self.pane_groups
            .entry(pane_id.to_string())
            .or_default()
            .insert(group.to_string());
        Ok(self.groups_of(pane_id))
    }

    /// Leave `group`. Removes the group when the last member leaves.
    /// Returns the pane's remaining groups.
    pub fn group_leave(&mut self, pane_id: &str, group: &str) -> Result<Vec<String>, MsgError> {
        validate_group_name(group)?;
        if let Some(members) = self.group_members.get_mut(group) {
            members.remove(pane_id);
            if members.is_empty() {
                self.group_members.remove(group);
            }
        }
        if let Some(groups) = self.pane_groups.get_mut(pane_id) {
            groups.remove(group);
            if groups.is_empty() {
                self.pane_groups.remove(pane_id);
            }
        }
        Ok(self.groups_of(pane_id))
    }

    /// Groups the pane currently belongs to (sorted).
    pub fn groups_of(&self, pane_id: &str) -> Vec<String> {
        let mut groups: Vec<String> = self
            .pane_groups
            .get(pane_id)
            .map(|s| s.iter().cloned().collect())
            .unwrap_or_default();
        groups.sort();
        groups
    }

    /// Members of a group (sorted). Empty for unknown groups.
    pub fn group_members(&self, group: &str) -> Vec<String> {
        let mut members: Vec<String> = self
            .group_members
            .get(group)
            .map(|s| s.iter().cloned().collect())
            .unwrap_or_default();
        members.sort();
        members
    }

    /// Re-key all bus state from `old` to `new` when a pane's canonical
    /// public id changes (cross-workspace move). Moves the inbox intact —
    /// seq counter, ack cursor and dropped count are preserved — and
    /// rewrites every group-index entry from `old` to `new`. Any stale state
    /// already under `new` is discarded first (a freshly assigned canonical
    /// id must never expose another pane's leftovers).
    pub fn rekey_pane(&mut self, old: &str, new: &str) {
        if old == new {
            return;
        }
        self.remove_pane(new);
        if let Some(inbox) = self.inboxes.remove(old) {
            self.inboxes.insert(new.to_string(), inbox);
        }
        if let Some(groups) = self.pane_groups.remove(old) {
            for group in &groups {
                if let Some(members) = self.group_members.get_mut(group) {
                    members.remove(old);
                    members.insert(new.to_string());
                }
            }
            self.pane_groups.insert(new.to_string(), groups);
        }
    }

    /// Tear down inbox and group membership for a closed pane.
    pub fn remove_pane(&mut self, pane_id: &str) {
        self.inboxes.remove(pane_id);
        let groups: Vec<String> = self
            .pane_groups
            .remove(pane_id)
            .map(|s| s.into_iter().collect())
            .unwrap_or_default();
        for group in groups {
            if let Some(members) = self.group_members.get_mut(&group) {
                members.remove(pane_id);
                if members.is_empty() {
                    self.group_members.remove(&group);
                }
            }
        }
    }
}

impl Default for MsgBus {
    fn default() -> Self {
        Self::new()
    }
}

/// True when `expr` matches the canonical public pane id grammar
/// `w<ENC>:p<ENC>` produced by [`crate::workspace::public_pane_id_for_number`]
/// (`<ENC>` = public-id alphabet, decoded by
/// [`crate::workspace::decode_public_number`]). Anything else — including
/// labels that merely contain a `:` such as `worker:api` — is not a pane id.
fn is_public_pane_id(expr: &str) -> bool {
    let Some((workspace, pane)) = expr.split_once(':') else {
        return false;
    };
    let Some(workspace_number) = workspace.strip_prefix('w') else {
        return false;
    };
    let Some(pane_number) = pane.strip_prefix('p') else {
        return false;
    };
    !workspace_number.is_empty()
        && !pane_number.is_empty()
        && crate::workspace::decode_public_number(workspace_number).is_some()
        && crate::workspace::decode_public_number(pane_number).is_some()
}

fn validate_group_name(group: &str) -> Result<(), MsgError> {
    // Group names: nonempty, no `/`, no leading `@` (the `@` is the address-form sigil).
    if group.is_empty() || group.contains('/') || group.starts_with('@') {
        return Err(MsgError::InvalidGroupName(group.to_string()));
    }
    Ok(())
}

fn sample_msg(body: impl Into<String>) -> StoredMsg {
    StoredMsg {
        seq: 0, // overwritten by deliver
        from_pane_id: Some("w1:p1".to_string()),
        from_workspace_id: Some("w1".to_string()),
        from_label: "orchestrator".to_string(),
        to: "worker-1".to_string(),
        body: body.into(),
        timestamp: "2026-07-18T00:00:00Z".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Directory: (workspace_id, pane_id, manual_label)
    fn dir() -> Vec<(String, String, String)> {
        vec![
            (
                "w1".to_string(),
                "w1:p1".to_string(),
                "orchestrator".to_string(),
            ),
            (
                "w1".to_string(),
                "w1:p2".to_string(),
                "worker-1".to_string(),
            ),
            (
                "w1".to_string(),
                "w1:p3".to_string(),
                "worker-2".to_string(),
            ),
            (
                "w2".to_string(),
                "w2:p1".to_string(),
                "worker-1".to_string(),
            ), // same label, other ws
            ("w2".to_string(), "w2:p2".to_string(), "solo".to_string()),
        ]
    }

    // --- parse_target -------------------------------------------------------

    #[test]
    fn parse_target_all_five_forms() {
        assert_eq!(MsgBus::parse_target("@all"), Ok(MsgTarget::All));
        assert_eq!(
            MsgBus::parse_target("@devs"),
            Ok(MsgTarget::Group("devs".to_string()))
        );
        assert_eq!(
            MsgBus::parse_target("w1:p3"),
            Ok(MsgTarget::PaneId("w1:p3".to_string()))
        );
        assert_eq!(
            MsgBus::parse_target("w1/worker-1"),
            Ok(MsgTarget::Qualified {
                workspace_id: "w1".to_string(),
                label: "worker-1".to_string(),
            })
        );
        assert_eq!(
            MsgBus::parse_target("worker-1"),
            Ok(MsgTarget::Label("worker-1".to_string()))
        );
    }

    #[test]
    fn parse_target_colon_label_routes_as_label_unless_pane_id_grammar() {
        // Legal labels may contain `:`; only the canonical public-pane-id
        // grammar (`w<ENC>:p<ENC>`) is classified as a PaneId.
        assert_eq!(
            MsgBus::parse_target("worker:api"),
            Ok(MsgTarget::Label("worker:api".to_string()))
        );
        assert_eq!(
            MsgBus::parse_target("w1:x2"),
            Ok(MsgTarget::Label("w1:x2".to_string()))
        );
        // Lowercase is not in the public-id alphabet.
        assert_eq!(
            MsgBus::parse_target("w1:pab"),
            Ok(MsgTarget::Label("w1:pab".to_string()))
        );
        assert_eq!(
            MsgBus::parse_target("a:b:c"),
            Ok(MsgTarget::Label("a:b:c".to_string()))
        );
        // Real pane ids still classify as PaneId.
        assert_eq!(
            MsgBus::parse_target("w1:p3"),
            Ok(MsgTarget::PaneId("w1:p3".to_string()))
        );
        assert_eq!(
            MsgBus::parse_target("wA:pZ"),
            Ok(MsgTarget::PaneId("wA:pZ".to_string()))
        );
        // Qualified form with a colon-bearing label.
        assert_eq!(
            MsgBus::parse_target("w1/worker:api"),
            Ok(MsgTarget::Qualified {
                workspace_id: "w1".to_string(),
                label: "worker:api".to_string(),
            })
        );
    }

    #[test]
    fn parse_target_unaddressable_labels() {
        assert_eq!(
            MsgBus::parse_target(""),
            Err(ResolveError::Unaddressable("".to_string()))
        );
        // multi-segment / empty parts → unaddressable (labels may not contain `/`)
        assert_eq!(
            MsgBus::parse_target("a/b/c"),
            Err(ResolveError::Unaddressable("a/b/c".to_string()))
        );
        assert_eq!(
            MsgBus::parse_target("/label"),
            Err(ResolveError::Unaddressable("/label".to_string()))
        );
        assert_eq!(
            MsgBus::parse_target("ws/"),
            Err(ResolveError::Unaddressable("ws/".to_string()))
        );
        // bare `@` or `@` with slash is unaddressable, not a group
        assert_eq!(
            MsgBus::parse_target("@"),
            Err(ResolveError::Unaddressable("@".to_string()))
        );
        assert_eq!(
            MsgBus::parse_target("@foo/bar"),
            Err(ResolveError::Unaddressable("@foo/bar".to_string()))
        );
        // qualified whose label starts with `@`
        assert_eq!(
            MsgBus::parse_target("w1/@bad"),
            Err(ResolveError::Unaddressable("w1/@bad".to_string()))
        );
    }

    // --- resolve rules 1–6 --------------------------------------------------

    #[test]
    fn resolve_pane_id_direct() {
        let bus = MsgBus::new();
        let d = dir();
        let target = MsgTarget::PaneId("w1:p2".to_string());
        assert_eq!(
            bus.resolve(&target, Some("w1"), &d),
            Ok(vec!["w1:p2".to_string()])
        );
    }

    #[test]
    fn resolve_pane_id_not_found() {
        let bus = MsgBus::new();
        let d = dir();
        let target = MsgTarget::PaneId("w9:missing".to_string());
        assert_eq!(
            bus.resolve(&target, Some("w1"), &d),
            Err(ResolveError::NotFound)
        );
    }

    #[test]
    fn resolve_qualified_workspace_only() {
        let bus = MsgBus::new();
        let d = dir();
        let target = MsgTarget::Qualified {
            workspace_id: "w2".to_string(),
            label: "worker-1".to_string(),
        };
        assert_eq!(
            bus.resolve(&target, Some("w1"), &d),
            Ok(vec!["w2:p1".to_string()])
        );
    }

    #[test]
    fn resolve_qualified_not_found() {
        let bus = MsgBus::new();
        let d = dir();
        let target = MsgTarget::Qualified {
            workspace_id: "w1".to_string(),
            label: "nope".to_string(),
        };
        assert_eq!(
            bus.resolve(&target, Some("w1"), &d),
            Err(ResolveError::NotFound)
        );
    }

    #[test]
    fn resolve_label_local_first() {
        let bus = MsgBus::new();
        let d = dir();
        // worker-1 exists in w1 and w2; sender in w1 → local hit
        let target = MsgTarget::Label("worker-1".to_string());
        assert_eq!(
            bus.resolve(&target, Some("w1"), &d),
            Ok(vec!["w1:p2".to_string()])
        );
    }

    #[test]
    fn resolve_label_global_fallback() {
        let bus = MsgBus::new();
        let d = dir();
        // "solo" only in w2; sender in w1 → local miss → global hit
        let target = MsgTarget::Label("solo".to_string());
        assert_eq!(
            bus.resolve(&target, Some("w1"), &d),
            Ok(vec!["w2:p2".to_string()])
        );
    }

    #[test]
    fn resolve_label_global_ambiguity() {
        let bus = MsgBus::new();
        let d = dir();
        // external sender (None) skips local phase → both worker-1s → ambiguous
        let target = MsgTarget::Label("worker-1".to_string());
        let err = bus.resolve(&target, None, &d).unwrap_err();
        match err {
            ResolveError::Ambiguous(cands) => {
                assert_eq!(cands.len(), 2);
                assert!(cands.iter().any(|c| c.2 == "w1:p2"));
                assert!(cands.iter().any(|c| c.2 == "w2:p1"));
            }
            other => panic!("expected Ambiguous, got {other:?}"),
        }
    }

    #[test]
    fn resolve_label_external_sender_skips_local_phase() {
        let bus = MsgBus::new();
        let d = dir();
        // external → no local phase; unique label still resolves globally
        let target = MsgTarget::Label("solo".to_string());
        assert_eq!(
            bus.resolve(&target, None, &d),
            Ok(vec!["w2:p2".to_string()])
        );
    }

    #[test]
    fn resolve_label_local_ambiguity() {
        // two panes same label in same workspace
        let bus = MsgBus::new();
        let d = vec![
            ("w1".to_string(), "w1:p1".to_string(), "dup".to_string()),
            ("w1".to_string(), "w1:p2".to_string(), "dup".to_string()),
        ];
        let target = MsgTarget::Label("dup".to_string());
        let err = bus.resolve(&target, Some("w1"), &d).unwrap_err();
        match err {
            ResolveError::Ambiguous(cands) => assert_eq!(cands.len(), 2),
            other => panic!("expected Ambiguous, got {other:?}"),
        }
    }

    #[test]
    fn resolve_group_members() {
        let mut bus = MsgBus::new();
        bus.group_join("w1:p2", "devs").unwrap();
        bus.group_join("w1:p3", "devs").unwrap();
        let target = MsgTarget::Group("devs".to_string());
        let ids = bus.resolve(&target, Some("w1"), &dir()).unwrap();
        assert_eq!(ids, vec!["w1:p2".to_string(), "w1:p3".to_string()]);
    }

    #[test]
    fn resolve_empty_or_unknown_group() {
        let bus = MsgBus::new();
        let target = MsgTarget::Group("ghosts".to_string());
        assert_eq!(
            bus.resolve(&target, Some("w1"), &dir()),
            Err(ResolveError::EmptyGroup("ghosts".to_string()))
        );
    }

    #[test]
    fn resolve_all_returns_every_directory_pane() {
        // Policy: resolve(All) returns every directory pane id.
        // WS2 filters the sender out of @all deliveries (spec rule 5).
        let bus = MsgBus::new();
        let d = dir();
        let ids = bus.resolve(&MsgTarget::All, Some("w1"), &d).unwrap();
        assert_eq!(
            ids,
            vec![
                "w1:p1".to_string(),
                "w1:p2".to_string(),
                "w1:p3".to_string(),
                "w2:p1".to_string(),
                "w2:p2".to_string(),
            ]
        );
    }

    #[test]
    fn resolve_label_not_found() {
        let bus = MsgBus::new();
        assert_eq!(
            bus.resolve(&MsgTarget::Label("nope".to_string()), Some("w1"), &dir()),
            Err(ResolveError::NotFound)
        );
    }

    #[test]
    fn resolve_matching_is_case_sensitive() {
        let bus = MsgBus::new();
        assert_eq!(
            bus.resolve(
                &MsgTarget::Label("Worker-1".to_string()),
                Some("w1"),
                &dir()
            ),
            Err(ResolveError::NotFound)
        );
    }

    // --- deliver / list / ack -----------------------------------------------

    #[test]
    fn deliver_assigns_seq_from_one_and_overwrites_input_seq() {
        let mut bus = MsgBus::new();
        let mut msg = sample_msg("hello");
        msg.seq = 999; // must be ignored
        bus.deliver("w1:p2", msg).unwrap();
        assert_eq!(bus.next_seq("w1:p2"), 2);

        let (msgs, unread, dropped, ack) = bus.list("w1:p2", None, true);
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0].seq, 1);
        assert_eq!(msgs[0].body, "hello");
        assert_eq!(unread, 1);
        assert_eq!(dropped, 0);
        assert_eq!(ack, 0);
    }

    #[test]
    fn list_is_pure_and_ack_advances_cursor() {
        let mut bus = MsgBus::new();
        bus.deliver("w1:p2", sample_msg("a")).unwrap();
        bus.deliver("w1:p2", sample_msg("b")).unwrap();
        bus.deliver("w1:p2", sample_msg("c")).unwrap();

        let (msgs, unread, _, ack) = bus.list("w1:p2", None, true);
        assert_eq!(msgs.len(), 3);
        assert_eq!(unread, 3);
        assert_eq!(ack, 0);

        // list again — still pure
        let (msgs2, unread2, _, _) = bus.list("w1:p2", None, false);
        assert_eq!(msgs2.len(), 3);
        assert_eq!(unread2, 3);

        let new_ack = bus.ack("w1:p2", 2);
        assert_eq!(new_ack, 2);
        assert_eq!(bus.unread("w1:p2"), 1);

        let (unread_only, unread, _, ack) = bus.list("w1:p2", None, false);
        assert_eq!(unread_only.len(), 1);
        assert_eq!(unread_only[0].seq, 3);
        assert_eq!(unread, 1);
        assert_eq!(ack, 2);

        let (all, _, _, _) = bus.list("w1:p2", None, true);
        assert_eq!(all.len(), 3);
    }

    #[test]
    fn list_after_seq_filter() {
        let mut bus = MsgBus::new();
        bus.deliver("w1:p2", sample_msg("a")).unwrap();
        bus.deliver("w1:p2", sample_msg("b")).unwrap();
        bus.deliver("w1:p2", sample_msg("c")).unwrap();

        let (msgs, _, _, _) = bus.list("w1:p2", Some(1), true);
        assert_eq!(msgs.iter().map(|m| m.seq).collect::<Vec<_>>(), vec![2, 3]);
    }

    #[test]
    fn ack_clamps_to_newest_and_behind_cursor_is_noop() {
        let mut bus = MsgBus::new();
        bus.deliver("w1:p2", sample_msg("a")).unwrap();
        bus.deliver("w1:p2", sample_msg("b")).unwrap();

        assert_eq!(bus.ack("w1:p2", 99), 2); // clamp
        assert_eq!(bus.ack("w1:p2", 1), 2); // behind cursor → no-op
        assert_eq!(bus.unread("w1:p2"), 0);
    }

    #[test]
    fn unread_unknown_pane_is_zero() {
        let bus = MsgBus::new();
        assert_eq!(bus.unread("missing"), 0);
        assert_eq!(bus.next_seq("missing"), 1);
        let (msgs, unread, dropped, ack) = bus.list("missing", None, true);
        assert!(msgs.is_empty());
        assert_eq!(unread, 0);
        assert_eq!(dropped, 0);
        assert_eq!(ack, 0);
    }

    #[test]
    fn ack_unknown_pane_returns_zero() {
        let mut bus = MsgBus::new();
        assert_eq!(bus.ack("missing", 5), 0);
    }

    // --- caps ---------------------------------------------------------------

    #[test]
    fn deliver_rejects_body_over_64kib() {
        let mut bus = MsgBus::new();
        let big = "x".repeat(MAX_MESSAGE_BODY_BYTES + 1);
        assert_eq!(
            bus.deliver("w1:p2", sample_msg(big)),
            Err(MsgError::MessageTooLarge)
        );
        assert_eq!(bus.unread("w1:p2"), 0);
    }

    #[test]
    fn deliver_accepts_body_exactly_64kib() {
        let mut bus = MsgBus::new();
        let body = "x".repeat(MAX_MESSAGE_BODY_BYTES);
        bus.deliver("w1:p2", sample_msg(body)).unwrap();
        assert_eq!(bus.unread("w1:p2"), 1);
    }

    #[test]
    fn inbox_count_cap_drops_oldest_and_bumps_dropped() {
        let mut bus = MsgBus::new();
        for i in 0..(MAX_INBOX_MESSAGES + 1) {
            bus.deliver("w1:p2", sample_msg(format!("m{i}"))).unwrap();
        }
        let (msgs, _, dropped, _) = bus.list("w1:p2", None, true);
        assert_eq!(msgs.len(), MAX_INBOX_MESSAGES);
        assert_eq!(dropped, 1);
        // oldest (seq 1) gone; newest is seq 501
        assert_eq!(msgs.first().unwrap().seq, 2);
        assert_eq!(msgs.last().unwrap().seq, (MAX_INBOX_MESSAGES as u64) + 1);
    }

    #[test]
    fn inbox_byte_cap_drops_oldest() {
        let mut bus = MsgBus::new();
        // 64 KiB bodies (at the per-message max): 64 fit exactly in 4 MiB;
        // the 65th forces oldest-drop under the byte cap.
        let chunk = "y".repeat(MAX_MESSAGE_BODY_BYTES);
        let n = (MAX_INBOX_BODY_BYTES / MAX_MESSAGE_BODY_BYTES) + 1; // 65
        for _ in 0..n {
            bus.deliver("w1:p2", sample_msg(chunk.clone())).unwrap();
        }
        let (msgs, _, dropped, _) = bus.list("w1:p2", None, true);
        assert!(msgs.len() < n);
        assert!(dropped >= 1);
        let total: usize = msgs.iter().map(|m| m.body.len()).sum();
        assert!(total <= MAX_INBOX_BODY_BYTES);
    }

    // --- groups -------------------------------------------------------------

    #[test]
    fn group_join_leave_and_auto_remove_on_empty() {
        let mut bus = MsgBus::new();
        assert_eq!(
            bus.group_join("w1:p2", "devs").unwrap(),
            vec!["devs".to_string()]
        );
        assert_eq!(
            bus.group_join("w1:p3", "devs").unwrap(),
            vec!["devs".to_string()]
        );
        assert_eq!(
            bus.group_members("devs"),
            vec!["w1:p2".to_string(), "w1:p3".to_string()]
        );

        assert_eq!(
            bus.group_leave("w1:p2", "devs").unwrap(),
            Vec::<String>::new()
        );
        assert_eq!(bus.group_members("devs"), vec!["w1:p3".to_string()]);

        // last member leaves → group removed
        assert_eq!(
            bus.group_leave("w1:p3", "devs").unwrap(),
            Vec::<String>::new()
        );
        assert!(bus.group_members("devs").is_empty());
        assert_eq!(
            bus.resolve(&MsgTarget::Group("devs".to_string()), None, &dir()),
            Err(ResolveError::EmptyGroup("devs".to_string()))
        );
    }

    #[test]
    fn group_invalid_names() {
        let mut bus = MsgBus::new();
        assert_eq!(
            bus.group_join("w1:p2", ""),
            Err(MsgError::InvalidGroupName("".to_string()))
        );
        assert_eq!(
            bus.group_join("w1:p2", "a/b"),
            Err(MsgError::InvalidGroupName("a/b".to_string()))
        );
        assert_eq!(
            bus.group_join("w1:p2", "@devs"),
            Err(MsgError::InvalidGroupName("@devs".to_string()))
        );
        assert_eq!(
            bus.group_leave("w1:p2", "@x"),
            Err(MsgError::InvalidGroupName("@x".to_string()))
        );
    }

    #[test]
    fn group_join_is_idempotent_for_same_pane() {
        let mut bus = MsgBus::new();
        bus.group_join("w1:p2", "devs").unwrap();
        bus.group_join("w1:p2", "devs").unwrap();
        assert_eq!(bus.group_members("devs"), vec!["w1:p2".to_string()]);
        assert_eq!(bus.groups_of("w1:p2"), vec!["devs".to_string()]);
    }

    #[test]
    fn groups_of_sorted_multiple() {
        let mut bus = MsgBus::new();
        bus.group_join("w1:p2", "zeta").unwrap();
        bus.group_join("w1:p2", "alpha").unwrap();
        assert_eq!(
            bus.groups_of("w1:p2"),
            vec!["alpha".to_string(), "zeta".to_string()]
        );
    }

    // --- remove_pane --------------------------------------------------------

    #[test]
    fn remove_pane_clears_inbox_and_membership() {
        let mut bus = MsgBus::new();
        bus.deliver("w1:p2", sample_msg("hi")).unwrap();
        bus.group_join("w1:p2", "devs").unwrap();
        bus.group_join("w1:p3", "devs").unwrap();

        bus.remove_pane("w1:p2");

        assert_eq!(bus.unread("w1:p2"), 0);
        assert!(bus.groups_of("w1:p2").is_empty());
        assert_eq!(bus.group_members("devs"), vec!["w1:p3".to_string()]);

        // last remaining member removed via pane close → group gone
        bus.remove_pane("w1:p3");
        assert!(bus.group_members("devs").is_empty());
    }

    #[test]
    fn remove_pane_unknown_is_noop() {
        let mut bus = MsgBus::new();
        bus.remove_pane("missing");
    }

    // --- rekey_pane ---------------------------------------------------------

    #[test]
    fn rekey_pane_moves_inbox_and_group_membership() {
        let mut bus = MsgBus::new();
        bus.group_join("w1:p2", "devs").unwrap();
        bus.group_join("w1:p3", "devs").unwrap();
        bus.deliver("w1:p2", sample_msg("a")).unwrap();
        bus.deliver("w1:p2", sample_msg("b")).unwrap();
        bus.deliver("w1:p2", sample_msg("c")).unwrap();
        bus.ack("w1:p2", 1);

        bus.rekey_pane("w1:p2", "w2:p5");

        // New id owns the inbox with seq/ack counters intact.
        let (msgs, unread, dropped, ack) = bus.list("w2:p5", None, true);
        assert_eq!(msgs.len(), 3);
        assert_eq!(unread, 2);
        assert_eq!(dropped, 0);
        assert_eq!(ack, 1);
        assert_eq!(bus.next_seq("w2:p5"), 4);
        assert_eq!(bus.groups_of("w2:p5"), vec!["devs".to_string()]);
        assert_eq!(
            bus.group_members("devs"),
            vec!["w1:p3".to_string(), "w2:p5".to_string()]
        );

        // Old id has nothing left.
        assert_eq!(bus.next_seq("w1:p2"), 1);
        assert_eq!(bus.unread("w1:p2"), 0);
        assert!(bus.groups_of("w1:p2").is_empty());
        let (old_msgs, _, _, _) = bus.list("w1:p2", None, true);
        assert!(old_msgs.is_empty());
    }

    #[test]
    fn rekey_pane_same_id_or_unknown_old_is_noop() {
        let mut bus = MsgBus::new();
        bus.deliver("w1:p2", sample_msg("a")).unwrap();
        bus.rekey_pane("w1:p2", "w1:p2");
        assert_eq!(bus.unread("w1:p2"), 1);

        bus.rekey_pane("missing", "w9:p9");
        assert_eq!(bus.unread("w9:p9"), 0);
        assert_eq!(bus.next_seq("w9:p9"), 1);
    }

    #[test]
    fn pane_id_reuse_starts_fresh_inbox() {
        // Spec testing note: closed pane teardown clears state; a later pane
        // that reuses the same id (if ever) must not see old messages/acks.
        let mut bus = MsgBus::new();
        bus.deliver("w1:p2", sample_msg("old")).unwrap();
        bus.ack("w1:p2", 1);
        bus.remove_pane("w1:p2");

        bus.deliver("w1:p2", sample_msg("new")).unwrap();
        let (msgs, unread, dropped, ack) = bus.list("w1:p2", None, true);
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0].seq, 1);
        assert_eq!(msgs[0].body, "new");
        assert_eq!(unread, 1);
        assert_eq!(dropped, 0);
        assert_eq!(ack, 0);
    }
}
