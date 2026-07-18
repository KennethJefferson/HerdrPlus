use crate::api::client::ApiClientError;
use crate::api::schema::{Method, MsgListParams, Request};

struct ReadPeekArgs {
    pane_id: Option<String>,
    after_seq: Option<u64>,
    all: bool,
}

struct AckArgs {
    up_to_seq: u64,
    pane_id: Option<String>,
}

struct WaitArgs {
    timeout_ms: Option<u64>,
    pane_id: Option<String>,
}

struct GroupArgs {
    name: String,
    pane_id: Option<String>,
}

pub(super) fn run_msg_command(args: &[String]) -> std::io::Result<i32> {
    let Some(subcommand) = args.first().map(|arg| arg.as_str()) else {
        print_msg_help();
        return Ok(2);
    };

    match subcommand {
        "send" => msg_send(&args[1..]),
        "read" => msg_read(&args[1..]),
        "peek" => msg_peek(&args[1..]),
        "ack" => msg_ack(&args[1..]),
        "wait" => msg_wait(&args[1..]),
        "group" => msg_group(&args[1..]),
        "who" => msg_who(&args[1..]),
        "help" | "--help" | "-h" => {
            print_msg_help();
            Ok(0)
        }
        _ => {
            print_msg_help();
            Ok(2)
        }
    }
}

fn print_msg_help() {
    eprintln!("herdr msg commands:");
    eprintln!("  herdr msg send <target> <text>");
    eprintln!("  herdr msg read [--all] [--after SEQ] [--pane ID]");
    eprintln!("  herdr msg peek [--all] [--after SEQ] [--pane ID]");
    eprintln!("  herdr msg ack <up-to-seq> [--pane ID]");
    eprintln!("  herdr msg wait [--timeout MS] [--pane ID]");
    eprintln!("  herdr msg group join <name> [--pane ID]");
    eprintln!("  herdr msg group leave <name> [--pane ID]");
    eprintln!("  herdr msg who");
}

fn parse_send_args(args: &[String]) -> Result<(String, String, Option<String>), String> {
    const USAGE: &str = "usage: herdr msg send <target> <text> [--pane ID]";

    let mut pane_id = None;
    let mut positionals = Vec::new();
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--pane" => {
                let Some(value) = args.get(index + 1) else {
                    return Err("missing value for --pane".into());
                };
                pane_id = Some(super::normalize_pane_id(value));
                index += 2;
            }
            other => {
                positionals.push(other.to_string());
                index += 1;
            }
        }
    }

    if positionals.is_empty() {
        return Err(USAGE.into());
    }
    let target = positionals.remove(0);
    if positionals.is_empty() {
        return Err(USAGE.into());
    }
    let text = positionals.join(" ");
    if text.trim().is_empty() {
        return Err(USAGE.into());
    }
    Ok((target, text, pane_id))
}

fn parse_read_peek_args(args: &[String]) -> Result<ReadPeekArgs, String> {
    let mut pane_id = None;
    let mut after_seq = None;
    let mut all = false;
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--pane" => {
                let Some(value) = args.get(index + 1) else {
                    return Err("missing value for --pane".into());
                };
                pane_id = Some(super::normalize_pane_id(value));
                index += 2;
            }
            "--after" => {
                let Some(value) = args.get(index + 1) else {
                    return Err("missing value for --after".into());
                };
                let parsed = value
                    .parse::<u64>()
                    .map_err(|_| format!("invalid value for --after: {value}"))?;
                after_seq = Some(parsed);
                index += 2;
            }
            "--all" => {
                all = true;
                index += 1;
            }
            other => return Err(format!("unknown option: {other}")),
        }
    }
    Ok(ReadPeekArgs { pane_id, after_seq, all })
}

fn parse_ack_args(args: &[String]) -> Result<AckArgs, String> {
    let mut up_to_seq = None;
    let mut pane_id = None;
    let mut index = 0;

    if args.first().is_some_and(|arg| !arg.as_str().starts_with('-')) {
        let val = args.first().unwrap();
        let parsed = val
            .parse::<u64>()
            .map_err(|_| format!("invalid value for up-to-seq: {val}"))?;
        up_to_seq = Some(parsed);
        index = 1;
    }

    while index < args.len() {
        match args[index].as_str() {
            "--pane" => {
                let Some(value) = args.get(index + 1) else {
                    return Err("missing value for --pane".into());
                };
                pane_id = Some(super::normalize_pane_id(value));
                index += 2;
            }
            other => return Err(format!("unknown option: {other}")),
        }
    }

    let Some(up_to_seq) = up_to_seq else {
        return Err("usage: herdr msg ack <up-to-seq> [--pane ID]".into());
    };

    Ok(AckArgs { up_to_seq, pane_id })
}

fn parse_wait_args(args: &[String]) -> Result<WaitArgs, String> {
    let mut timeout_ms = None;
    let mut pane_id = None;
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--pane" => {
                let Some(value) = args.get(index + 1) else {
                    return Err("missing value for --pane".into());
                };
                pane_id = Some(super::normalize_pane_id(value));
                index += 2;
            }
            "--timeout" => {
                let Some(value) = args.get(index + 1) else {
                    return Err("missing value for --timeout".into());
                };
                let parsed = value
                    .parse::<u64>()
                    .map_err(|_| format!("invalid value for --timeout: {value}"))?;
                timeout_ms = Some(parsed);
                index += 2;
            }
            other => return Err(format!("unknown option: {other}")),
        }
    }
    Ok(WaitArgs { timeout_ms, pane_id })
}

fn parse_group_args(args: &[String], usage: &str) -> Result<GroupArgs, String> {
    let mut name = None;
    let mut pane_id = None;
    let mut index = 0;

    if args.first().is_some_and(|arg| !arg.as_str().starts_with('-')) {
        name = Some(args.first().unwrap().clone());
        index = 1;
    }

    while index < args.len() {
        match args[index].as_str() {
            "--pane" => {
                let Some(value) = args.get(index + 1) else {
                    return Err("missing value for --pane".into());
                };
                pane_id = Some(super::normalize_pane_id(value));
                index += 2;
            }
            other => return Err(format!("unknown option: {other}")),
        }
    }

    let Some(name) = name else {
        return Err(usage.into());
    };

    Ok(GroupArgs { name, pane_id })
}

fn msg_send(args: &[String]) -> std::io::Result<i32> {
    let env_pane_id = std::env::var("HERDR_PANE_ID")
        .ok()
        .filter(|value| !value.trim().is_empty());
    let (target, text, pane_id) = match parse_send_args(args) {
        Ok(res) => res,
        Err(err) => {
            eprintln!("{err}");
            return Ok(2);
        }
    };

    let params = crate::api::schema::MsgSendParams {
        target,
        body: text,
        sender_pane_id: pane_id.or(env_pane_id),
    };

    super::print_response(&super::send_request(&Request {
        id: "cli:msg:send".into(),
        method: Method::MsgSend(params),
    })?)
}

fn msg_read(args: &[String]) -> std::io::Result<i32> {
    let env_pane_id = std::env::var("HERDR_PANE_ID")
        .ok()
        .filter(|value| !value.trim().is_empty());
    let parsed = match parse_read_peek_args(args) {
        Ok(parsed) => parsed,
        Err(err) => {
            eprintln!("{err}");
            return Ok(2);
        }
    };
    let pane_id = match parsed.pane_id.or(env_pane_id) {
        Some(pane_id) => pane_id,
        None => {
            eprintln!("missing --pane ID or HERDR_PANE_ID env var");
            return Ok(2);
        }
    };

    let response = super::send_request(&Request {
        id: "cli:msg:list".into(),
        method: Method::MsgList(MsgListParams {
            pane_id: pane_id.clone(),
            after_seq: parsed.after_seq,
            include_read: Some(parsed.all),
        }),
    })?;

    if let Some(error) = response.get("error") {
        eprintln!("{}", serde_json::to_string(error).unwrap());
        return Ok(1);
    }

    println!("{}", serde_json::to_string(&response).unwrap());

    let mut highest_seq = 0;
    if let Some(messages) = response["result"]["messages"].as_array() {
        for msg in messages {
            if let Some(seq) = msg["seq"].as_u64() {
                if seq > highest_seq {
                    highest_seq = seq;
                }
            }
        }
    }

    if highest_seq > 0 {
        let ack_res = super::send_request(&Request {
            id: "cli:msg:ack".into(),
            method: Method::MsgAck(crate::api::schema::MsgAckParams {
                pane_id,
                up_to_seq: highest_seq,
            }),
        })?;
        if let Some(error) = ack_res.get("error") {
            eprintln!("{}", serde_json::to_string(error).unwrap());
            return Ok(1);
        }
    }

    Ok(0)
}

fn msg_peek(args: &[String]) -> std::io::Result<i32> {
    let env_pane_id = std::env::var("HERDR_PANE_ID")
        .ok()
        .filter(|value| !value.trim().is_empty());
    let parsed = match parse_read_peek_args(args) {
        Ok(parsed) => parsed,
        Err(err) => {
            eprintln!("{err}");
            return Ok(2);
        }
    };
    let pane_id = match parsed.pane_id.or(env_pane_id) {
        Some(pane_id) => pane_id,
        None => {
            eprintln!("missing --pane ID or HERDR_PANE_ID env var");
            return Ok(2);
        }
    };

    super::print_response(&super::send_request(&Request {
        id: "cli:msg:list".into(),
        method: Method::MsgList(MsgListParams {
            pane_id,
            after_seq: parsed.after_seq,
            include_read: Some(parsed.all),
        }),
    })?)
}

fn msg_ack(args: &[String]) -> std::io::Result<i32> {
    let env_pane_id = std::env::var("HERDR_PANE_ID")
        .ok()
        .filter(|value| !value.trim().is_empty());
    let parsed = match parse_ack_args(args) {
        Ok(parsed) => parsed,
        Err(err) => {
            eprintln!("{err}");
            return Ok(2);
        }
    };
    let pane_id = match parsed.pane_id.or(env_pane_id) {
        Some(pane_id) => pane_id,
        None => {
            eprintln!("missing --pane ID or HERDR_PANE_ID env var");
            return Ok(2);
        }
    };

    super::print_response(&super::send_request(&Request {
        id: "cli:msg:ack".into(),
        method: Method::MsgAck(crate::api::schema::MsgAckParams {
            pane_id,
            up_to_seq: parsed.up_to_seq,
        }),
    })?)
}

fn msg_wait(args: &[String]) -> std::io::Result<i32> {
    let env_pane_id = std::env::var("HERDR_PANE_ID")
        .ok()
        .filter(|value| !value.trim().is_empty());
    let parsed = match parse_wait_args(args) {
        Ok(parsed) => parsed,
        Err(err) => {
            eprintln!("{err}");
            return Ok(2);
        }
    };
    let pane_id = match parsed.pane_id.or(env_pane_id) {
        Some(pane_id) => pane_id,
        None => {
            eprintln!("missing --pane ID or HERDR_PANE_ID env var");
            return Ok(2);
        }
    };

    let initial_list_res = super::send_request(&Request {
        id: "cli:msg:wait:initial".into(),
        method: Method::MsgList(MsgListParams {
            pane_id: pane_id.clone(),
            after_seq: None,
            include_read: Some(true),
        }),
    })?;

    if let Some(error) = initial_list_res.get("error") {
        eprintln!("{}", serde_json::to_string(error).unwrap());
        return Ok(1);
    }

    let mut last_seq = 0;
    if let Some(messages) = initial_list_res["result"]["messages"].as_array() {
        for msg in messages {
            if let Some(seq) = msg["seq"].as_u64() {
                if seq > last_seq {
                    last_seq = seq;
                }
            }
        }
    }

    let read_timeout = parsed.timeout_ms.map(std::time::Duration::from_millis);
    let client = crate::api::client::ApiClient::local();

    let request = Request {
        id: "cli:msg:wait:subscribe".into(),
        method: Method::EventsSubscribe(crate::api::schema::EventsSubscribeParams {
            subscriptions: vec![crate::api::schema::Subscription::PaneMsgReceived {
                pane_id: pane_id.clone(),
            }],
        }),
    };

    super::ensure_server_protocol_compatible(&client, &request.id)?;
    let (ack, mut stream) = client
        .subscribe_value(&request, read_timeout)
        .map_err(super::api_client_error_to_io)?;

    if let Err(err) = crate::api::client::parse_response_value(ack) {
        if let ApiClientError::ErrorResponse(response) = err {
            eprintln!("{}", serde_json::to_string(&response).unwrap());
            return Ok(1);
        }
        return Err(super::api_client_error_to_io(err));
    }

    match stream.next_event() {
        Ok(None) => {
            eprintln!("subscription closed before event arrived");
            Ok(1)
        }
        Ok(Some(_event_value)) => {
            let final_list_res = super::send_request(&Request {
                id: "cli:msg:wait:list".into(),
                method: Method::MsgList(MsgListParams {
                    pane_id: pane_id.clone(),
                    after_seq: Some(last_seq),
                    include_read: Some(true),
                }),
            })?;

            if let Some(error) = final_list_res.get("error") {
                eprintln!("{}", serde_json::to_string(error).unwrap());
                return Ok(1);
            }

            if let Some(messages) = final_list_res["result"]["messages"].as_array() {
                for msg in messages {
                    println!("{}", serde_json::to_string(msg).unwrap());
                }
            }
            Ok(0)
        }
        Err(ApiClientError::Io(err)) if super::api_timeout_error(&err) => {
            eprintln!("timed out waiting for new messages");
            Ok(1)
        }
        Err(err) => Err(super::api_client_error_to_io(err)),
    }
}

fn msg_group(args: &[String]) -> std::io::Result<i32> {
    let Some(subcommand) = args.first().map(|arg| arg.as_str()) else {
        print_msg_help();
        return Ok(2);
    };
    match subcommand {
        "join" => msg_group_join(&args[1..]),
        "leave" => msg_group_leave(&args[1..]),
        _ => {
            print_msg_help();
            Ok(2)
        }
    }
}

fn msg_group_join(args: &[String]) -> std::io::Result<i32> {
    let env_pane_id = std::env::var("HERDR_PANE_ID")
        .ok()
        .filter(|value| !value.trim().is_empty());
    let parsed = match parse_group_args(args, "usage: herdr msg group join <name> [--pane ID]") {
        Ok(parsed) => parsed,
        Err(err) => {
            eprintln!("{err}");
            return Ok(2);
        }
    };
    let pane_id = match parsed.pane_id.or(env_pane_id) {
        Some(pane_id) => pane_id,
        None => {
            eprintln!("missing --pane ID or HERDR_PANE_ID env var");
            return Ok(2);
        }
    };

    super::print_response(&super::send_request(&Request {
        id: "cli:msg:group_join".into(),
        method: Method::MsgGroupJoin(crate::api::schema::MsgGroupJoinParams {
            pane_id,
            group: parsed.name,
        }),
    })?)
}

fn msg_group_leave(args: &[String]) -> std::io::Result<i32> {
    let env_pane_id = std::env::var("HERDR_PANE_ID")
        .ok()
        .filter(|value| !value.trim().is_empty());
    let parsed = match parse_group_args(args, "usage: herdr msg group leave <name> [--pane ID]") {
        Ok(parsed) => parsed,
        Err(err) => {
            eprintln!("{err}");
            return Ok(2);
        }
    };
    let pane_id = match parsed.pane_id.or(env_pane_id) {
        Some(pane_id) => pane_id,
        None => {
            eprintln!("missing --pane ID or HERDR_PANE_ID env var");
            return Ok(2);
        }
    };

    super::print_response(&super::send_request(&Request {
        id: "cli:msg:group_leave".into(),
        method: Method::MsgGroupLeave(crate::api::schema::MsgGroupLeaveParams {
            pane_id,
            group: parsed.name,
        }),
    })?)
}

fn msg_who(args: &[String]) -> std::io::Result<i32> {
    if !args.is_empty() {
        eprintln!("usage: herdr msg who");
        return Ok(2);
    }
    super::print_response(&super::send_request(&Request {
        id: "cli:msg:who".into(),
        method: Method::MsgWho(crate::api::schema::EmptyParams::default()),
    })?)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_string()).collect()
    }

    #[test]
    fn test_parse_send_args_success() {
        let (target, text, pane_id) =
            parse_send_args(&args(&["w1/worker-1", "hello", "world"])).unwrap();
        assert_eq!(target, "w1/worker-1");
        assert_eq!(text, "hello world");
        assert_eq!(pane_id, None);
    }

    #[test]
    fn test_parse_send_args_errors() {
        assert!(parse_send_args(&args(&[])).is_err());
        assert!(parse_send_args(&args(&["w1/worker-1"])).is_err());
        assert!(parse_send_args(&args(&["w1/worker-1", "   "])).is_err());
    }

    #[test]
    fn test_parse_send_args_pane_flag_after_positionals() {
        let (target, text, pane_id) = parse_send_args(&args(&[
            "worker-1",
            "build the API first",
            "--pane",
            "w1:p13",
        ]))
        .unwrap();
        assert_eq!(target, "worker-1");
        assert_eq!(text, "build the API first");
        assert_eq!(pane_id, Some("w1:p13".to_string()));
        assert!(!text.contains("--pane"));
    }

    #[test]
    fn test_parse_send_args_pane_flag_before_positionals() {
        let (target, text, pane_id) = parse_send_args(&args(&[
            "--pane",
            "w1:p13",
            "worker-2",
            "text",
        ]))
        .unwrap();
        assert_eq!(target, "worker-2");
        assert_eq!(text, "text");
        assert_eq!(pane_id, Some("w1:p13".to_string()));
        assert!(!text.contains("--pane"));
    }

    #[test]
    fn test_parse_send_args_pane_flag_between_positionals() {
        let (target, text, pane_id) = parse_send_args(&args(&[
            "worker-1",
            "--pane",
            "w1:p13",
            "build the API first",
        ]))
        .unwrap();
        assert_eq!(target, "worker-1");
        assert_eq!(text, "build the API first");
        assert_eq!(pane_id, Some("w1:p13".to_string()));
        assert!(!text.contains("--pane"));
    }

    #[test]
    fn test_parse_send_args_no_flag_is_external() {
        let (target, text, pane_id) =
            parse_send_args(&args(&["worker-1", "hello there"])).unwrap();
        assert_eq!(target, "worker-1");
        assert_eq!(text, "hello there");
        assert_eq!(pane_id, None);
        assert!(!text.contains("--pane"));
    }

    #[test]
    fn test_parse_read_peek_args() {
        let res = parse_read_peek_args(&args(&[])).unwrap();
        assert_eq!(res.pane_id, None);
        assert_eq!(res.after_seq, None);
        assert!(!res.all);

        let res = parse_read_peek_args(&args(&["--pane", "p123", "--after", "45", "--all"])).unwrap();
        assert_eq!(res.pane_id, Some("p123".to_string()));
        assert_eq!(res.after_seq, Some(45));
        assert!(res.all);
    }

    #[test]
    fn test_parse_read_peek_args_errors() {
        assert!(parse_read_peek_args(&args(&["--pane"])).is_err());
        assert!(parse_read_peek_args(&args(&["--after", "invalid"])).is_err());
        assert!(parse_read_peek_args(&args(&["--unknown"])).is_err());
    }

    #[test]
    fn test_parse_ack_args() {
        let res = parse_ack_args(&args(&["42", "--pane", "p456"])).unwrap();
        assert_eq!(res.up_to_seq, 42);
        assert_eq!(res.pane_id, Some("p456".to_string()));
    }

    #[test]
    fn test_parse_ack_args_errors() {
        assert!(parse_ack_args(&args(&[])).is_err());
        assert!(parse_ack_args(&args(&["invalid"])).is_err());
        assert!(parse_ack_args(&args(&["42", "--pane"])).is_err());
    }

    #[test]
    fn test_parse_wait_args() {
        let res = parse_wait_args(&args(&["--timeout", "1000", "--pane", "p789"])).unwrap();
        assert_eq!(res.timeout_ms, Some(1000));
        assert_eq!(res.pane_id, Some("p789".to_string()));
    }

    #[test]
    fn test_parse_wait_args_errors() {
        assert!(parse_wait_args(&args(&["--timeout"])).is_err());
        assert!(parse_wait_args(&args(&["--timeout", "abc"])).is_err());
    }

    #[test]
    fn test_parse_group_args() {
        let res = parse_group_args(&args(&["mygroup", "--pane", "p1"]), "usage").unwrap();
        assert_eq!(res.name, "mygroup");
        assert_eq!(res.pane_id, Some("p1".to_string()));
    }

    #[test]
    fn test_parse_group_args_errors() {
        assert!(parse_group_args(&args(&[]), "usage").is_err());
        assert!(parse_group_args(&args(&["--pane", "p1"]), "usage").is_err());
    }
}
