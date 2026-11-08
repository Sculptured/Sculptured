//! The OpenCode driver against a real `opencode` and a real model on OpenRouter: one turn, for a
//! check before a release (a fraction of a cent). A live test: it needs `OPENCODE_LIVE_PROGRAM`,
//! `OPENCODE_LIVE_REAL_MODEL` (OpenCode's `openrouter/…` name) and `OPENROUTER_API_KEY`, and is
//! skipped without them. Run it with `cargo test -p drivers --test opencode_live -- --ignored live_`.
use drivers::opencode::OpencodeAcpDriver;
use drivers::{Driver, DriverEvent, LaunchSpec, OutboundMessage};
use futures::StreamExt;
use futures::stream::BoxStream;
use model::{MessageIntent, PermissionPosture, TurnStatus};
use std::time::Duration;

/// A launch in `dir`, with OpenCode's home there too, so the person's own setup is not read.
fn spec(dir: &std::path::Path, program: &str, model: &str, key: &str) -> LaunchSpec {
    let mut s = LaunchSpec::new("dev.a@r".parse().unwrap(), dir.to_path_buf());
    s.program = Some(program.into());
    s.model = Some(model.into());
    s.posture = PermissionPosture::Unrestricted;
    let home = dir.join("home");
    std::fs::create_dir_all(home.join(".config/opencode")).unwrap();
    s.env = [
        ("HOME", home.clone()),
        ("XDG_CONFIG_HOME", home.join(".config")),
        ("XDG_DATA_HOME", home.join(".local/share")),
        ("XDG_CACHE_HOME", home.join(".cache")),
        ("XDG_STATE_HOME", home.join(".local/state")),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_string(), v.display().to_string()))
    .chain([("OPENROUTER_API_KEY".to_string(), key.to_string())])
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
        let e = tokio::time::timeout(Duration::from_secs(240), events.next())
            .await
            .expect("an event in time")
            .expect("the stream goes on");
        let done = want(&e);
        seen.push(e);
        if done {
            return seen;
        }
    }
}

#[tokio::test]
#[ignore = "live: real opencode CLI and a real model on OpenRouter; run with --ignored"]
async fn live_a_real_model_on_openrouter_answers_and_reports_its_cost() {
    let (Ok(program), Ok(model), Ok(key)) = (
        std::env::var("OPENCODE_LIVE_PROGRAM"),
        std::env::var("OPENCODE_LIVE_REAL_MODEL"),
        std::env::var("OPENROUTER_API_KEY"),
    ) else {
        eprintln!(
            "skipped: set OPENCODE_LIVE_PROGRAM, OPENCODE_LIVE_REAL_MODEL and OPENROUTER_API_KEY"
        );
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let mut session = OpencodeAcpDriver::default()
        .launch(spec(dir.path(), &program, &model, &key))
        .await
        .expect("starts");
    let mut events = session.take_events().unwrap();
    session
        .send(msg(
            "m1",
            "Run the shell command `echo sculptured-live` and reply with its output only.",
        ))
        .await
        .unwrap();
    let turn = until(&mut events, |e| matches!(e, DriverEvent::TurnEnded { .. })).await;
    session.stop(Duration::from_secs(5)).await.unwrap();
    eprintln!("{turn:#?}");
    assert!(turn.contains(&DriverEvent::TurnEnded {
        turn_id: "turn-m1".into(),
        status: TurnStatus::Completed
    }));
    assert!(turn.iter().any(|e| matches!(e, DriverEvent::ToolResult { output, .. } if output.contains("sculptured-live"))));
    // A `:free` model costs nothing, so it reports no cost.
    if !model.ends_with(":free") {
        assert!(
            turn.iter()
                .any(|e| matches!(e, DriverEvent::Usage { cost: Some(c), .. } if c.usd > 0.0)),
            "a cost was reported"
        );
    }
}
