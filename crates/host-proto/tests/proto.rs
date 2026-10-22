use host_proto::*;
use model::*;
use serde_json::Value;

#[test]
fn messages_round_trip_with_tags() {
    let hello = FromHost::Hello {
        host_id: "local".into(),
        protocol: PROTOCOL_VERSION,
        drivers: vec![DriverInfo { name: "stub".into(), caps: Caps::default() }],
        live_seats: vec!["dev.a@r".into()],
        launching_seats: vec![],
        features: vec![features::POSTURE.into()],
        policy: None,
    };
    let s = encode(&hello);
    assert_eq!(serde_json::from_str::<Value>(&s).unwrap()["op"], "hello");
    assert_eq!(decode::<FromHost>(&s).unwrap(), hello);

    let cmd = ToHost::Command {
        command_id: "c1".into(),
        command: HostCommand::Launch {
            seat: "dev.a@r".into(),
            driver: "stub".into(),
            params: LaunchParams { cwd: "/tmp".into(), ..Default::default() },
        },
    };
    let s = encode(&cmd);
    let v: Value = serde_json::from_str(&s).unwrap();
    assert_eq!(v["op"], "command");
    assert_eq!(v["command"]["cmd"], "launch");
    assert_eq!(decode::<ToHost>(&s).unwrap(), cmd);

    let ev = FromHost::Event {
        event: HostEvent {
            host_seq: 4,
            at_ms: 9,
            actor: Actor::System("hostd".into()),
            subject: Subject::Seat("dev.a@r".into()),
            provenance: Provenance::ProtocolObserved,
            body: EventBody::SeatLifecycleChanged { lifecycle: SeatLifecycle::Running, reason: None },
        },
    };
    assert_eq!(decode::<FromHost>(&encode(&ev)).unwrap(), ev);
}

#[test]
fn m2_commands_and_frames_round_trip() {
    let restart = HostCommand::Restart {
        seat: "dev.a@r".into(),
        driver: "stub".into(),
        params: LaunchParams { cwd: "/tmp".into(), seat_tools: Some(SeatToolsParams { approve: true, workflow: true }), ..Default::default() },
        mode: LaunchMode::Resume { native_session_id: "n1".into() },
        cause: TenureCause::Resume,
    };
    let v: Value = serde_json::from_str(&encode(&restart)).unwrap();
    assert_eq!(v["cmd"], "restart");
    assert_eq!(v["mode"]["kind"], "resume");
    assert_eq!(decode::<HostCommand>(&encode(&restart)).unwrap(), restart);

    let answer = HostCommand::Answer { seat: "dev.a@r".into(), question_id: "q1".into(), answer: Answer::Allow };
    assert_eq!(decode::<HostCommand>(&encode(&answer)).unwrap(), answer);

    let call = FromHost::SeatTool { call_id: "c1".into(), seat: "dev.a@r".into(), tool: "report".into(), args: serde_json::json!({ "summary": "ok" }) };
    assert_eq!(decode::<FromHost>(&encode(&call)).unwrap(), call);
    let reply = ToHost::SeatToolReply { call_id: "c1".into(), ok: false, result: Value::Null, error: Some("no".into()) };
    assert_eq!(decode::<ToHost>(&encode(&reply)).unwrap(), reply);
}

#[test]
fn m1_launch_params_still_decode() {
    let old = r#"{"cmd":"launch","seat":"dev.a@r","driver":"stub","params":{"cwd":"/tmp","model":null,"program":null,"env":[],"extra_args":[]}}"#;
    match decode::<HostCommand>(old).unwrap() {
        HostCommand::Launch { params, .. } => {
            assert_eq!(params.posture, PermissionPosture::Unrestricted);
            assert!(params.seat_tools.is_none() && params.usage_baseline.is_none());
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn seat_tool_client_round_trips_one_line() {
    use host_proto::seat_tools::{call, ToolReply, ToolRequest};
    use std::io::{BufRead, BufReader, Write};
    let dir = tempfile::tempdir().unwrap();
    let sock = dir.path().join("t.sock");
    let listener = std::os::unix::net::UnixListener::bind(&sock).unwrap();
    let server = std::thread::spawn(move || {
        let (s, _) = listener.accept().unwrap();
        let mut line = String::new();
        BufReader::new(&s).read_line(&mut line).unwrap();
        let req: ToolRequest = serde_json::from_str(line.trim()).unwrap();
        let reply = ToolReply { ok: true, result: serde_json::json!({ "echo": req.tool, "token": req.token }), error: None };
        (&s).write_all(format!("{}\n", serde_json::to_string(&reply).unwrap()).as_bytes()).unwrap();
    });
    let got = call(&sock, "tok", "report", serde_json::json!({})).unwrap();
    server.join().unwrap();
    assert_eq!(got, serde_json::json!({ "echo": "report", "token": "tok" }));
}

#[test]
fn seat_tool_client_reports_a_missing_socket_plainly() {
    let dir = tempfile::tempdir().unwrap();
    let err = host_proto::seat_tools::call(&dir.path().join("none.sock"), "tok", "report", serde_json::json!({})).unwrap_err();
    assert!(err.starts_with("cannot reach Sculptured's agent runner"), "{err}");
}

#[test]
fn an_m1_hello_advertises_no_features_and_no_launching_seats() {
    let old = r#"{"op":"hello","host_id":"local","protocol":1,"drivers":[],"live_seats":["dev.a@r"]}"#;
    match decode::<FromHost>(old).unwrap() {
        FromHost::Hello { features, launching_seats, live_seats, .. } => {
            assert!(features.is_empty() && launching_seats.is_empty());
            assert_eq!(live_seats, vec!["dev.a@r".to_string()]);
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn commands_name_the_host_features_they_need() {
    let launch = |params: LaunchParams| HostCommand::Launch { seat: "dev.a@r".into(), driver: "stub".into(), params };
    let plain = LaunchParams { cwd: "/tmp".into(), ..Default::default() };
    assert!(launch(plain.clone()).required_features().is_empty(), "an M1 launch needs nothing new");
    let restricted = LaunchParams { posture: PermissionPosture::Restricted, ..plain.clone() };
    assert_eq!(launch(restricted).required_features(), vec![features::POSTURE]);
    let full = LaunchParams {
        posture: PermissionPosture::Restricted,
        seat_tools: Some(SeatToolsParams { approve: false, workflow: true }),
        usage_baseline: Some([1, 2, 3, 4]),
        ..plain.clone()
    };
    assert_eq!(launch(full).required_features(), vec![features::POSTURE, features::SEAT_TOOLS, features::USAGE_BASELINE]);
    let restart = HostCommand::Restart {
        seat: "dev.a@r".into(),
        driver: "stub".into(),
        params: plain,
        mode: LaunchMode::Fresh,
        cause: TenureCause::Handover,
    };
    assert_eq!(restart.required_features(), vec![features::RESTART]);
    let answer = HostCommand::Answer { seat: "dev.a@r".into(), question_id: "q".into(), answer: Answer::Allow };
    assert_eq!(answer.required_features(), vec![features::ANSWER]);
    assert!(HostCommand::Interrupt { seat: "dev.a@r".into() }.required_features().is_empty());
}

#[test]
fn a_stop_without_hard_keeps_the_grace_path_and_old_runners_read_a_hard_one() {
    let old: host_proto::HostCommand = serde_json::from_value(serde_json::json!({ "cmd": "stop", "seat": "a@r", "grace_ms": 5000 })).unwrap();
    assert_eq!(old, host_proto::HostCommand::Stop { seat: "a@r".into(), grace_ms: 5000, hard: false });
    let soft = serde_json::to_value(&old).unwrap();
    assert!(soft.get("hard").is_none(), "a grace stop is sent exactly as before: {soft}");
    let hard = serde_json::to_value(host_proto::HostCommand::Stop { seat: "a@r".into(), grace_ms: 5000, hard: true }).unwrap();
    assert_eq!(hard["hard"], true);
    assert_eq!(hard["grace_ms"], 5000, "an older runner, which ignores `hard`, still stops within the grace");
}

#[test]
fn launch_settings_never_print_a_key_when_debug_formatted() {
    let p = host_proto::LaunchParams { env: vec![("ANTHROPIC_API_KEY".to_string(), "sk-ant-secret-123".to_string())].into(), ..Default::default() };
    let shown = format!("{p:?}");
    assert!(shown.contains("ANTHROPIC_API_KEY") && !shown.contains("sk-ant-secret-123"), "{shown}");
    let json = serde_json::to_value(&p).unwrap();
    assert_eq!(json["env"], serde_json::json!([["ANTHROPIC_API_KEY", "sk-ant-secret-123"]]), "the wire format is unchanged");
}
