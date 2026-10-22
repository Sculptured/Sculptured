use model::{
    ClaudeSetup, ContinueMode, PlanBudget, PlanPause, RunBudget, SoftHard, TeamDraft, TokenLimit,
};
use serde_json::{from_value, json};

#[test]
fn soft_defaults_to_85_percent_of_hard() {
    assert_eq!(
        SoftHard {
            soft: None,
            hard: 1000u64
        }
        .soft_value(),
        850
    );
    assert_eq!(
        SoftHard {
            soft: Some(10u32),
            hard: 60
        }
        .soft_value(),
        10
    );
    assert_eq!(
        SoftHard {
            soft: None,
            hard: 1u32
        }
        .soft_value(),
        1,
        "a one-turn limit does not wrap up before it starts"
    );
    let p = PlanBudget {
        provider: "claude".into(),
        window: "five_hour".into(),
        soft: None,
        hard: 30.0,
        pause: None,
    };
    assert!((p.soft_value() - 25.5).abs() < 1e-9);
}

#[test]
fn soft_above_hard_and_nonsense_are_refused() {
    let mut b = RunBudget::default();
    assert!(b.problems().is_empty(), "an empty budget is fine");
    b.tokens = Some(SoftHard {
        soft: Some(2000),
        hard: 1000,
    });
    b.plan.push(PlanBudget {
        provider: "gemini".into(),
        window: "five_hour".into(),
        soft: None,
        hard: 30.0,
        pause: None,
    });
    b.plan.push(PlanBudget {
        provider: "claude".into(),
        window: "daily".into(),
        soft: None,
        hard: 30.0,
        pause: None,
    });
    b.plan.push(PlanBudget {
        provider: "codex".into(),
        window: "seven_day".into(),
        soft: None,
        hard: 130.0,
        pause: None,
    });
    b.max_working = Some(0);
    let p = b.problems().join(" | ");
    for want in [
        "soft value is above the hard",
        "gemini",
        "daily",
        "130",
        "at least 1",
    ] {
        assert!(p.contains(want), "{want}: {p}");
    }
    assert!(
        SoftHard {
            soft: Some(5u32),
            hard: 0
        }
        .problem("Turns")
        .is_some(),
        "a zero hard value means nothing can run"
    );
}

#[test]
fn old_token_limit_loads_with_no_soft() {
    let t: TokenLimit = serde_json::from_value(json!({ "tokens": 5000 })).unwrap();
    assert_eq!((t.tokens, t.soft, t.soft_value()), (5000, None, 4250));
}

#[test]
fn run_budget_round_trips_and_is_omitted_when_empty() {
    let b = RunBudget {
        plan: vec![PlanBudget {
            provider: "claude".into(),
            window: "five_hour".into(),
            soft: Some(25.0),
            hard: 30.0,
            pause: None,
        }],
        tokens: None,
        minutes: Some(SoftHard {
            soft: None,
            hard: 60,
        }),
        max_working: Some(6),
    };
    let v = serde_json::to_value(&b).unwrap();
    assert_eq!(
        v,
        json!({ "plan": [{ "provider": "claude", "window": "five_hour", "soft": 25.0, "hard": 30.0 }], "minutes": { "hard": 60 }, "max_working": 6 })
    );
    assert_eq!(from_value::<RunBudget>(v).unwrap(), b);
    assert!(RunBudget::default().is_empty());
}

#[test]
fn claude_setup_flags_leave_out_what_the_team_turned_off() {
    let flags = |global, project| ClaudeSetup { global, project }.claude_args();
    assert_eq!(
        ClaudeSetup::default(),
        ClaudeSetup {
            global: false,
            project: true
        },
        "global off, project on by default"
    );
    assert_eq!(
        flags(false, true),
        ["--setting-sources", "project,local", "--strict-mcp-config"]
    );
    assert_eq!(flags(true, false), ["--setting-sources", "user"]);
    assert_eq!(
        flags(false, false),
        ["--setting-sources", "", "--strict-mcp-config"]
    );
    let d: TeamDraft =
        serde_json::from_value(json!({ "id": "t", "name": "T", "revision": 0 })).unwrap();
    assert_eq!(
        d.claude_setup,
        ClaudeSetup::default(),
        "a team saved before the setting gets the default"
    );
    assert!(
        serde_json::to_value(&d)
            .unwrap()
            .get("claude_setup")
            .is_none(),
        "the default is not written"
    );
    let d: TeamDraft = serde_json::from_value(
        json!({ "id": "t", "name": "T", "revision": 0, "claude_setup": { "global": true } }),
    )
    .unwrap();
    assert_eq!(
        d.claude_setup,
        ClaudeSetup {
            global: true,
            project: true
        }
    );
}

#[test]
fn a_plan_budget_can_pause_until_its_window_resets() {
    let v = json!({ "provider": "claude", "window": "five_hour", "hard": 30.0, "pause": {} });
    let p: PlanBudget = serde_json::from_value(v).unwrap();
    assert_eq!(
        p.pause,
        Some(PlanPause {
            continue_with: ContinueMode::Notes,
            max_windows: 3
        }),
        "from notes, at most 3 windows, unless chosen"
    );
    let old: PlanBudget = serde_json::from_value(
        json!({ "provider": "claude", "window": "five_hour", "hard": 30.0 }),
    )
    .unwrap();
    assert_eq!(old.pause, None, "a budget saved before stops, as it did");
    assert_eq!(
        serde_json::to_value(PlanPause {
            continue_with: ContinueMode::Resume,
            max_windows: 2
        })
        .unwrap(),
        json!({ "continue_with": "resume", "max_windows": 2 })
    );
    let b = RunBudget {
        plan: vec![PlanBudget {
            pause: Some(PlanPause {
                continue_with: ContinueMode::Notes,
                max_windows: 0,
            }),
            ..p
        }],
        ..Default::default()
    };
    assert!(
        b.problems().join(" ").contains("at least 1 window"),
        "{:?}",
        b.problems()
    );
}
