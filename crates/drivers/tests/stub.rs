use drivers::stub::StubDriver;

use drivers::{Driver, DriverError, DriverEvent, LaunchSpec, OutboundMessage};
use futures::StreamExt;
use model::{MessageIntent, TurnStatus};
use std::time::Duration;

fn spec(dir: &std::path::Path) -> LaunchSpec {
    LaunchSpec::new("dev.a@r".parse().unwrap(), dir.to_path_buf())
}
fn msg(id: &str, text: &str) -> OutboundMessage {
    OutboundMessage {
        message_id: id.into(),
        intent: MessageIntent::Inform,
        text: text.into(),
    }
}
async fn next(s: &mut futures::stream::BoxStream<'static, DriverEvent>) -> DriverEvent {
    tokio::time::timeout(Duration::from_secs(2), s.next())
        .await
        .expect("event in time")
        .expect("stream open")
}

#[tokio::test]
async fn stub_turn_lifecycle() {
    let dir = tempfile::tempdir().unwrap();
    let mut session = StubDriver.launch(spec(dir.path())).await.unwrap();
    let mut ev = session.take_events().unwrap();
    assert!(session.take_events().is_none(), "events can be taken once");
    assert_eq!(
        next(&mut ev).await,
        DriverEvent::SessionReady {
            native_session_id: "stub-dev.a@r".into()
        }
    );

    session.send(msg("m1", "hello")).await.unwrap();
    assert_eq!(
        next(&mut ev).await,
        DriverEvent::TurnStarted {
            turn_id: "turn-1".into()
        }
    );
    assert_eq!(
        next(&mut ev).await,
        DriverEvent::MessageConsumed {
            message_id: "m1".into(),
            turn_id: "turn-1".into()
        }
    );
    assert_eq!(
        next(&mut ev).await,
        DriverEvent::Text {
            turn_id: Some("turn-1".into()),
            text: "stub received: hello".into()
        }
    );
    assert!(matches!(
        next(&mut ev).await,
        DriverEvent::Usage {
            input_tokens: 10,
            output_tokens: 5,
            ..
        }
    ));
    assert_eq!(
        next(&mut ev).await,
        DriverEvent::TurnEnded {
            turn_id: "turn-1".into(),
            status: TurnStatus::Completed
        }
    );
}

#[tokio::test]
async fn stub_interrupt_then_queued_message_runs() {
    let dir = tempfile::tempdir().unwrap();
    let mut session = StubDriver.launch(spec(dir.path())).await.unwrap();
    let mut ev = session.take_events().unwrap();
    next(&mut ev).await; // SessionReady
    session.send(msg("m1", "hang")).await.unwrap();
    session.send(msg("m2", "after")).await.unwrap(); // queued behind the hanging turn
    next(&mut ev).await; // TurnStarted turn-1
    next(&mut ev).await; // MessageConsumed m1
    session.interrupt().await.unwrap();
    assert_eq!(
        next(&mut ev).await,
        DriverEvent::TurnEnded {
            turn_id: "turn-1".into(),
            status: TurnStatus::Interrupted
        }
    );
    assert_eq!(
        next(&mut ev).await,
        DriverEvent::TurnStarted {
            turn_id: "turn-2".into()
        }
    );
    assert_eq!(
        next(&mut ev).await,
        DriverEvent::MessageConsumed {
            message_id: "m2".into(),
            turn_id: "turn-2".into()
        }
    );
}

#[tokio::test]
async fn stub_crash_and_stop() {
    let dir = tempfile::tempdir().unwrap();
    let mut a = StubDriver.launch(spec(dir.path())).await.unwrap();
    let mut ev = a.take_events().unwrap();
    next(&mut ev).await;
    a.send(msg("m1", "crash")).await.unwrap();
    next(&mut ev).await; // TurnStarted
    next(&mut ev).await; // MessageConsumed
    assert!(matches!(
        next(&mut ev).await,
        DriverEvent::Exited {
            exit_code: Some(3),
            ..
        }
    ));
    assert!(ev.next().await.is_none(), "stream ends after Exited");

    let mut b = StubDriver.launch(spec(dir.path())).await.unwrap();
    let mut ev = b.take_events().unwrap();
    next(&mut ev).await;
    b.stop(Duration::from_secs(1)).await.unwrap();
    assert!(matches!(
        next(&mut ev).await,
        DriverEvent::Exited {
            exit_code: Some(0),
            ..
        }
    ));
    assert!(matches!(
        b.send(msg("m2", "late")).await,
        Err(DriverError::Closed)
    ));
}

#[tokio::test]
async fn stub_rejects_missing_cwd() {
    let missing = std::path::PathBuf::from("/definitely/not/here");
    let err = StubDriver
        .launch(spec(&missing))
        .await
        .err()
        .expect("launch must fail");
    assert!(err.to_string().contains("/definitely/not/here"), "{err}");
}
