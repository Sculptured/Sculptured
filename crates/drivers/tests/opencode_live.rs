//! The OpenCode driver against a real `opencode` and a stand-in for OpenRouter's chat API that answers
//! by a word in the prompt (`scenario-perm`, `scenario-slow`, else a greeting), as the recordings were
//! made. Not a routine test: it needs both, named by `OPENCODE_LIVE_PROGRAM` and `OPENCODE_LIVE_BASE_URL`
//! (OpenRouter's `/api/v1` address on the stand-in), and is skipped without them.
use drivers::opencode::OpencodeAcpDriver;
use drivers::{Driver, DriverEvent, LaunchSpec, OutboundMessage};
use futures::StreamExt;
use futures::stream::BoxStream;
use model::{Answer, MessageIntent, PermissionPosture, TurnStatus};
use std::time::Duration;

fn live() -> Option<(String, String)> {
    Some((
        std::env::var("OPENCODE_LIVE_PROGRAM").ok()?,
        std::env::var("OPENCODE_LIVE_BASE_URL").ok()?,
    ))
}

/// A launch in `dir`, with OpenCode's home there too, pointed at the stand-in by its global config.
fn spec(dir: &std::path::Path, program: &str, base: &str) -> LaunchSpec {
    let config = dir.join("home/.config/opencode");
    std::fs::create_dir_all(&config).unwrap();
    let global =
        serde_json::json!({ "provider": { "openrouter": { "options": { "baseURL": base } } } });
    std::fs::write(config.join("opencode.json"), global.to_string()).unwrap();
    let mut s = LaunchSpec::new("dev.a@r".parse().unwrap(), dir.to_path_buf());
    s.program = Some(program.into());
    s.model = Some("openrouter/deepseek/deepseek-chat-v3.1".into());
    s.posture = PermissionPosture::Restricted;
    let home = dir.join("home");
    s.env = [
        ("HOME", home.clone()),
        ("XDG_CONFIG_HOME", home.join(".config")),
        ("XDG_DATA_HOME", home.join(".local/share")),
        ("XDG_CACHE_HOME", home.join(".cache")),
        ("XDG_STATE_HOME", home.join(".local/state")),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_string(), v.display().to_string()))
    .chain([(
        "OPENROUTER_API_KEY".to_string(),
        "sk-or-stand-in".to_string(),
    )])
    .collect();
    s
}

fn msg(id: &str, text: &str) -> OutboundMessage {
    OutboundMessage {
        message_id: id.into(),
        intent: MessageIntent::Inform,
        text: text.into(),
    }
}

/// Reads events until `want` matches one; all the events read are returned.
async fn until(
    events: &mut BoxStream<'static, DriverEvent>,
    want: impl Fn(&DriverEvent) -> bool,
) -> Vec<DriverEvent> {
    let mut seen = vec![];
    loop {
        let e = tokio::time::timeout(Duration::from_secs(60), events.next())
            .await
            .expect("an event within a minute")
            .expect("the stream goes on");
        let done = want(&e);
        seen.push(e);
        if done {
            return seen;
        }
    }
}

fn ended(status: TurnStatus) -> impl Fn(&DriverEvent) -> bool {
    move |e| matches!(e, DriverEvent::TurnEnded { status: s, .. } if *s == status)
}

#[tokio::test]
async fn a_restricted_agent_asks_runs_is_interrupted_and_resumes() {
    let Some((program, base)) = live() else {
        eprintln!("skipped: set OPENCODE_LIVE_PROGRAM and OPENCODE_LIVE_BASE_URL");
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let driver = OpencodeAcpDriver::default();
    let mut s = driver
        .launch(spec(dir.path(), &program, &base))
        .await
        .expect("starts");
    let mut events = s.take_events().unwrap();
    let ready = until(&mut events, |e| {
        matches!(e, DriverEvent::ModelObserved { .. })
    })
    .await;
    let native = ready.iter().find_map(|e| match e {
        DriverEvent::SessionReady { native_session_id } => Some(native_session_id.clone()),
        _ => None,
    });
    let observed = ready.iter().find_map(|e| match e {
        DriverEvent::ModelObserved { observed, .. } => observed.clone(),
        _ => None,
    });
    assert_eq!(
        observed.as_deref(),
        Some("openrouter/deepseek/deepseek-chat-v3.1")
    );

    s.send(msg("m1", "Run echo. scenario-perm")).await.unwrap();
    let asked = until(&mut events, |e| {
        matches!(e, DriverEvent::QuestionRaised { .. })
    })
    .await;
    let Some(DriverEvent::QuestionRaised {
        question_id,
        prompt,
        ..
    }) = asked.last()
    else {
        unreachable!()
    };
    assert_eq!(prompt.text, "Allow OpenCode to run `echo hello-from-tool`?");
    s.answer(question_id, Answer::Allow).await.unwrap();
    let turn = until(&mut events, ended(TurnStatus::Completed)).await;
    assert!(turn.iter().any(|e| matches!(e, DriverEvent::ToolResult { output, .. } if output.contains("hello-from-tool"))), "{turn:?}");

    s.send(msg("m2", "Take your time. scenario-slow"))
        .await
        .unwrap();
    until(&mut events, |e| {
        matches!(e, DriverEvent::TurnStarted { .. })
    })
    .await;
    tokio::time::sleep(Duration::from_secs(2)).await;
    s.interrupt().await.unwrap();
    until(&mut events, ended(TurnStatus::Interrupted)).await;
    s.stop(Duration::from_secs(5)).await.unwrap();
    until(&mut events, |e| matches!(e, DriverEvent::Exited { .. })).await;

    let native = native.expect("a session id");
    let mut s = driver
        .resume(spec(dir.path(), &program, &base), &native)
        .await
        .expect("resumes");
    let mut events = s.take_events().unwrap();
    s.send(msg("m3", "Say hello. scenario-basic"))
        .await
        .unwrap();
    let turn = until(&mut events, ended(TurnStatus::Completed)).await;
    assert!(
        turn.iter().any(
            |e| matches!(e, DriverEvent::Text { text, .. } if text == "Hello from the model.")
        ),
        "{turn:?}"
    );
    s.stop(Duration::from_secs(5)).await.unwrap();
}
