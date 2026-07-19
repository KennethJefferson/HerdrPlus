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
    eprintln!("  entries with commas or complex quoting: define them in [team.agents] config instead");
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
                    let (label, agent) = split_label(raw);
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
    let cwd = match cwd {
        Some(cwd) => Some(cwd),
        None => Some(
            std::env::current_dir()
                .map_err(|err| format!("cannot resolve current directory for --cwd: {err}"))?
                .to_string_lossy()
                .into_owned(),
        ),
    };
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

fn split_label(raw: &str) -> (Option<String>, String) {
    if let Some((label, agent)) = raw.split_once('=') {
        let is_ident = !label.is_empty()
            && label.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-');
        if is_ident {
            return (Some(label.to_string()), agent.to_string());
        }
    }
    (None, raw.to_string())
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

/// Poll each detectable agent pane until the agent is recognized or agent_status != "unknown", or timeout.
/// Prints a per-pane readiness report; exit 0 all ready, exit 3 on timeout.
fn wait_for_team_ready(spawn_response: &serde_json::Value, timeout_secs: u64) -> std::io::Result<i32> {
    let panes = spawn_response["result"]["panes"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    // waitable = herdr's detect module knows this agent (~21 agents:
    // claude/codex/gemini/pi/.../omp/grok). The orch pane is excluded
    // explicitly, not just because "orch" is absent from the detect registry.
    let waitable = |p: &serde_json::Value| {
        p["agent"]
            .as_str()
            .map(|a| a != "orch" && crate::detect::identify_agent(a).is_some())
            .unwrap_or(false)
    };
    let mut pending: BTreeMap<String, String> = panes
        .iter()
        .filter(|p| waitable(p))
        .filter_map(|p| {
            Some((p["pane_id"].as_str()?.to_string(), p["label"].as_str()?.to_string()))
        })
        .collect();
    let skipped: Vec<String> = panes
        .iter()
        .filter(|p| !waitable(p))
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
            let pane = &response["result"]["pane"];
            let recognized = pane["agent"].as_str().map(|a| !a.is_empty()).unwrap_or(false);
            let status = pane["agent_status"].as_str().unwrap_or("unknown");
            if recognized || status != "unknown" {
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

    #[test]
    fn default_cwd_is_process_cwd() {
        let parsed = parse_spawn_args(&args(&["t", "--agents", "pi"])).unwrap();
        assert_eq!(
            parsed.params.cwd,
            Some(std::env::current_dir().unwrap().to_string_lossy().into_owned())
        );
    }

    #[test]
    fn entry_with_equals_in_command_is_passthrough() {
        let parsed = parse_spawn_args(&args(&["t", "--agents", "claude --model=x"])).unwrap();
        assert_eq!(parsed.params.entries.len(), 1);
        assert_eq!(parsed.params.entries[0].label, None);
        assert_eq!(parsed.params.entries[0].agent, "claude --model=x");
    }

    #[test]
    fn identifier_label_still_parses() {
        let parsed = parse_spawn_args(&args(&["t", "--agents", "ws-1=claude"])).unwrap();
        assert_eq!(parsed.params.entries[0].label.as_deref(), Some("ws-1"));
        assert_eq!(parsed.params.entries[0].agent, "claude");
    }
}
