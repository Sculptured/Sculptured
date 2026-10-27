use model::{
    AgentAdvanced, AgentKind, ConnectionKind, DEFAULT_GROUP_ID, DraftAgent, DraftConnection,
    DraftIssue, ElementKind, ElementRef, IssueSeverity, ModelDefaults, PermissionPosture,
    ResumeBehaviour, TeamDraft, TeamInput,
};
use serde_json::json;

fn builder() -> DraftAgent {
    DraftAgent {
        id: "builder".into(),
        group_id: None,
        name: "Builder".into(),
        kind: AgentKind::Practice,
        job: "Build it".into(),
        folder: None,
        x: 40,
        y: 40,
        advanced: AgentAdvanced::default(),
        connection: None,
    }
}

fn reviewer() -> DraftAgent {
    DraftAgent {
        id: "reviewer".into(),
        group_id: Some("review".into()),
        name: "Reviewer".into(),
        kind: AgentKind::Claude,
        job: String::new(),
        folder: Some("/home/u/repo".into()),
        x: 300,
        y: 40,
        advanced: AgentAdvanced {
            data_access: None,
            authority: None,
            skills: vec![],
            integrations: vec![],
            turn_limit: None,
            minute_limit: None,
            model: Some("opus".into()),
            permission: PermissionPosture::Restricted,
            resume: ResumeBehaviour::RelaunchFresh,
            token_limit: None,
            check: None,
            briefing: None,
            writes_status: false,
        },
        connection: None,
    }
}

fn draft() -> TeamDraft {
    TeamDraft {
        claude_setup: Default::default(),
        protection: Default::default(),
        safety: vec![],
        components: vec![],
        budget: Default::default(),
        input: None,
        schedules: vec![],
        mandate: Default::default(),
        decision_log: false,
        limits: Default::default(),
        workflow: None,
        folder: None,
        id: "my-team".into(),
        name: "My team".into(),
        groups: vec![],
        agents: vec![builder(), reviewer()],
        connections: vec![DraftConnection {
            id: "c1".into(),
            from: "builder".into(),
            to: "reviewer".into(),
            kind: ConnectionKind::HandsWorkTo,
        }],
        models: ModelDefaults::default(),
        revision: 3,
    }
}

#[test]
fn draft_wire_shape_round_trips() {
    let d = draft();
    let v = serde_json::to_value(&d).unwrap();
    assert_eq!(v["agents"][0]["kind"], "practice");
    assert_eq!(
        v["agents"][0]["advanced"],
        json!({ "model": null, "permission": "unrestricted", "resume": "resume-if-possible" })
    );
    assert_eq!(v["agents"][1]["advanced"]["resume"], "relaunch-fresh");
    assert_eq!(v["connections"][0]["kind"], "hands-work-to");
    assert_eq!(v["revision"], 3);
    assert_eq!(serde_json::from_value::<TeamDraft>(v).unwrap(), d);
}

#[test]
fn optional_parts_take_defaults() {
    let d: TeamDraft = serde_json::from_value(json!({
        "id": "t", "name": "T", "revision": 0,
        "agents": [{ "id": "a", "group_id": null, "name": "A", "kind": "codex", "folder": null, "x": 0, "y": 0 }]
    }))
    .unwrap();
    assert!(d.groups.is_empty() && d.connections.is_empty());
    assert_eq!(d.agents[0].job, "");
    assert_eq!(d.agents[0].advanced, AgentAdvanced::default());
}

#[test]
fn seat_ids_come_from_immutable_ids_not_names() {
    let mut d = draft();
    assert_eq!(
        d.seat_id("builder").as_deref(),
        Some("team.builder@my-team")
    );
    assert_eq!(
        d.seat_id("reviewer").as_deref(),
        Some("review.reviewer@my-team")
    );
    d.agents[0].name = "Lead builder".into();
    d.name = "Renamed team".into();
    assert_eq!(
        d.seat_id("builder").as_deref(),
        Some("team.builder@my-team")
    );
    assert_eq!(d.seat_id("nobody"), None);
    assert_eq!(DEFAULT_GROUP_ID, "team");
}

#[test]
fn issue_wire_shape() {
    let i = DraftIssue {
        element: ElementRef {
            kind: ElementKind::Agent,
            id: "reviewer".into(),
        },
        severity: IssueSeverity::Error,
        code: "folder-missing".into(),
        message: "Reviewer needs a folder to work in".into(),
        detail: None,
        spec_path: None,
    };
    let v = serde_json::to_value(&i).unwrap();
    assert_eq!(v["element"], json!({ "kind": "agent", "id": "reviewer" }));
    assert_eq!(v["severity"], "error");
}

/// C0: a team and each group may set a model per provider. A draft saved before that loads with no
/// defaults (inherit), and empty defaults are left out of the saved JSON, so old drafts stay byte-identical.
#[test]
fn model_defaults_are_optional_and_omitted_when_empty() {
    let old = json!({
        "id": "my-team", "name": "My team", "revision": 3,
        "groups": [{ "id": "g", "label": "G", "x": 0, "y": 0, "w": 1, "h": 1 }],
        "agents": [], "connections": []
    });
    let d: TeamDraft = serde_json::from_value(old.clone()).unwrap();
    assert!(d.models.is_empty());
    assert!(d.groups[0].models.is_empty());
    assert_eq!(d.models, ModelDefaults::default());
    assert_eq!(
        serde_json::to_value(&d).unwrap(),
        old,
        "empty defaults are not written"
    );

    let mut d = d;
    d.models = ModelDefaults {
        claude: Some("sonnet".into()),
        codex: None,
        opencode: None,
    };
    d.groups[0].models = ModelDefaults {
        claude: None,
        codex: Some("gpt-6-sol".into()),
        opencode: None,
    };
    let v = serde_json::to_value(&d).unwrap();
    assert_eq!(v["models"], json!({ "claude": "sonnet", "codex": null }));
    assert_eq!(
        v["groups"][0]["models"],
        json!({ "claude": null, "codex": "gpt-6-sol" })
    );
    assert_eq!(serde_json::from_value::<TeamDraft>(v).unwrap(), d);
    // A defaults object naming one provider only loads the other as inherit.
    let one: ModelDefaults = serde_json::from_value(json!({ "codex": "gpt-6-luna" })).unwrap();
    assert_eq!(
        one,
        ModelDefaults {
            claude: None,
            codex: Some("gpt-6-luna".into()),
            opencode: None,
        }
    );
    assert!(!one.is_empty());
}

#[test]
fn the_input_reaches_the_agents_it_points_to_with_their_job() {
    let mut d = draft();
    assert_eq!(d.job_with_input(&d.agents[0]), "Build it");
    d.input = Some(TeamInput {
        text: " Ship the landing page.\nLinks: example.test ".into(),
        x: 0,
        y: 0,
        to: vec!["builder".into(), "reviewer".into()],
    });
    assert_eq!(
        d.job_with_input(&d.agents[0]),
        "Build it\n\n--- Input ---\nShip the landing page.\nLinks: example.test"
    );
    assert_eq!(
        d.job_with_input(&d.agents[1]),
        "Ship the landing page.\nLinks: example.test",
        "no Job: the Input is the work"
    );
    d.input.as_mut().unwrap().to = vec![];
    assert_eq!(d.job_with_input(&d.agents[1]), "");
    let json = serde_json::to_value(&d).unwrap();
    assert_eq!(json["input"]["to"], json!([]));
    assert_eq!(
        serde_json::from_value::<TeamDraft>(json).unwrap(),
        d,
        "round-trips"
    );
    assert!(
        serde_json::to_value(draft())
            .unwrap()
            .get("input")
            .is_none(),
        "no Input, no field"
    );
}

/// An agent's model connection (a connection id kept by the gateway) is saved with the draft and
/// left out of the JSON when unset, so older drafts stay byte-identical and still load.
#[test]
fn an_agents_model_connection_round_trips_and_is_omitted_when_unset() {
    let d = draft();
    let v = serde_json::to_value(&d).unwrap();
    assert!(
        v["agents"][1].get("connection").is_none(),
        "unset: no field: {}",
        v["agents"][1]
    );
    assert_eq!(
        serde_json::from_value::<TeamDraft>(v).unwrap().agents[1].connection,
        None
    );
    let mut d = draft();
    d.agents[1].connection = Some("conn-7".into());
    let v = serde_json::to_value(&d).unwrap();
    assert_eq!(v["agents"][1]["connection"], "conn-7");
    assert_eq!(serde_json::from_value::<TeamDraft>(v).unwrap(), d);
}
