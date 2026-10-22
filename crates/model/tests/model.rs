use model::{
    Actor, EventBody, EventRecord, OutcomeEvidence, Provenance, SeatActivity, SeatId,
    SeatLifecycle, Subject, TurnStatus,
};
use serde_json::json;

#[test]
fn seat_id_round_trips() {
    let id: SeatId = "dev.impl@my-rig".parse().unwrap();
    assert_eq!(id.pod, "dev");
    assert_eq!(id.member, "impl");
    assert_eq!(id.rig, "my-rig");
    assert_eq!(id.to_string(), "dev.impl@my-rig");
    assert_eq!(id.logical(), "dev.impl");
    assert_eq!(serde_json::to_value(&id).unwrap(), json!("dev.impl@my-rig"));
}

#[test]
fn seat_id_rejects_bad_shapes() {
    for bad in [
        "dev.impl",
        "dev@r",
        "a.b.c@r",
        "dev.im pl@r",
        ".impl@r",
        "dev.impl@",
        "dev.impl@r@x",
    ] {
        assert!(bad.parse::<SeatId>().is_err(), "{bad} should be rejected");
    }
}

#[test]
fn status_values_are_kebab_case() {
    assert_eq!(
        serde_json::to_value(SeatActivity::WaitingInput).unwrap(),
        json!("waiting-input")
    );
    assert_eq!(
        serde_json::to_value(OutcomeEvidence::ReportedComplete).unwrap(),
        json!("reported-complete")
    );
    assert_eq!(
        serde_json::to_value(SeatLifecycle::Attention).unwrap(),
        json!("attention")
    );
    assert_eq!(
        serde_json::to_value(Provenance::ProtocolObserved).unwrap(),
        json!("protocol-observed")
    );
}

#[test]
fn event_record_wire_shape() {
    let ev = EventRecord {
        seq: 7,
        host: "local".into(),
        host_seq: 3,
        at_ms: 1,
        actor: Actor::Seat("dev.impl@r".into()),
        subject: Subject::Seat("dev.impl@r".into()),
        provenance: Provenance::ProtocolObserved,
        body: EventBody::TurnEnded {
            turn_id: "turn-1".into(),
            status: TurnStatus::Interrupted,
        },
    };
    let v = serde_json::to_value(&ev).unwrap();
    assert_eq!(v["body"]["type"], "turn-ended");
    assert_eq!(v["body"]["status"], "interrupted");
    assert_eq!(v["actor"], json!({ "kind": "seat", "id": "dev.impl@r" }));
    assert_eq!(ev.subject.key(), "seat:dev.impl@r");
    assert_eq!(ev.actor.to_string(), "seat:dev.impl@r");
    let back: EventRecord = serde_json::from_value(v).unwrap();
    assert_eq!(back, ev);
}
