use model::{
    ActionClass, Answer, ContextBasis, DecisionSource, EventBody, Finding, MAX_FINDING,
    MAX_FINDINGS, QuestionOption, QuestionPrompt, QuestionSource, ReportVerdict, RestoreDecision,
    RiskFlag, SessionMode, ThinkingForm,
};
use serde_json::json;

#[test]
fn answers_and_sources_use_kebab_tags() {
    assert_eq!(
        serde_json::to_value(Answer::Deny { reason: None }).unwrap(),
        json!({ "kind": "deny", "reason": null })
    );
    assert_eq!(
        serde_json::to_value(Answer::Text {
            text: "Green".into()
        })
        .unwrap(),
        json!({ "kind": "text", "text": "Green" })
    );
    assert_eq!(
        serde_json::to_value(Answer::Expired).unwrap(),
        json!({ "kind": "expired" })
    );
    assert_eq!(
        serde_json::to_value(QuestionSource::NativePermission).unwrap(),
        json!("native-permission")
    );
    assert_eq!(
        serde_json::to_value(DecisionSource::AllowedOnTimeout).unwrap(),
        json!("allowed-on-timeout")
    );
    assert_eq!(
        serde_json::to_value(SessionMode::Fork).unwrap(),
        json!("fork")
    );
    assert_eq!(
        serde_json::to_value(RestoreDecision::Leave).unwrap(),
        json!("leave")
    );
    assert_eq!(
        serde_json::to_value(ReportVerdict::ProblemsFound).unwrap(),
        json!("problems-found")
    );
}

#[test]
fn action_classes_are_tagged_by_kind() {
    assert_eq!(
        serde_json::to_value(ActionClass::ShellTestRun).unwrap(),
        json!({ "kind": "shell-test-run" })
    );
    assert_eq!(
        serde_json::to_value(ActionClass::Mcp {
            server: "rig".into(),
            tool: "report".into()
        })
        .unwrap(),
        json!({ "kind": "mcp", "server": "rig", "tool": "report" })
    );
}

/// One body of each kind M2 added.
fn new_event_bodies() -> Vec<EventBody> {
    vec![
        EventBody::QuestionRaised {
            question_id: "q1".into(),
            source: QuestionSource::RequestInput,
            prompt: QuestionPrompt {
                urgency: None,
                header: None,
                text: "Which?".into(),
                options: vec![],
                allow_other: true,
                tool: None,
                tool_input: None,
            },
            turn_id: Some("turn-1".into()),
        },
        EventBody::QuestionDecided {
            question_id: "q1".into(),
            decision: "Blue".into(),
            rationale: None,
            source: DecisionSource::NextMessage,
        },
        EventBody::FindingsReported {
            turn_id: Some("turn-2".into()),
            summary: "2 problems".into(),
            findings: vec![Finding {
                text: "missing test".into(),
                location: Some("src/a.rs:3".into()),
            }],
            verdict: ReportVerdict::ProblemsFound,
        },
        EventBody::ActionInterpreted {
            call_id: "c1".into(),
            class: ActionClass::ShellGit,
            paths: vec![],
            risks: vec![RiskFlag {
                code: "force-push".into(),
                text: "force-pushed to origin main".into(),
            }],
        },
        EventBody::NewSessionRequested {
            mode: SessionMode::Fresh,
            reason: "context is full".into(),
        },
        EventBody::RestoreNeeded {
            reason: "resume failed".into(),
            can_resume: true,
        },
        EventBody::RestoreDecided {
            decision: RestoreDecision::Fresh,
        },
    ]
}

#[test]
fn new_event_bodies_round_trip() {
    for b in new_event_bodies() {
        let v = serde_json::to_value(&b).unwrap();
        assert_eq!(serde_json::from_value::<EventBody>(v).unwrap(), b);
    }
    assert_eq!(
        serde_json::to_value(EventBody::QuestionExpired {
            question_id: "q".into()
        })
        .unwrap()["type"],
        "question-expired"
    );
    assert_eq!(
        serde_json::to_value(EventBody::ReportRejected {
            tool: "report".into(),
            reason: "x".into(),
            raw: json!({})
        })
        .unwrap()["type"],
        "report-rejected"
    );
}

#[test]
fn prompt_is_capped() {
    let p = QuestionPrompt {
        urgency: None,
        header: Some("h".repeat(5000)),
        text: "é".repeat(10_000),
        options: (0..500)
            .map(|i| QuestionOption {
                label: format!("{i}{}", "x".repeat(400)),
                description: Some("d".repeat(5000)),
            })
            .collect(),
        allow_other: false,
        tool: Some("B".repeat(5000)),
        tool_input: Some(json!({ "command": "c".repeat(200_000), "description": "keep" })),
    }
    .capped();
    // A permission prompt is cut in the middle, never at the end: see `an_approval_prompt_keeps_a_dangerous_tail`.
    assert!(p.text.len() <= QuestionPrompt::MAX_TEXT, "{}", p.text.len());
    assert!(p.text.contains(NOT_STORED), "{}", p.text);
    assert_eq!(p.options.len(), QuestionPrompt::MAX_OPTIONS);
    assert!(p.options[0].label.len() <= QuestionPrompt::MAX_LABEL + 4);
    assert!(p.header.unwrap().len() <= QuestionPrompt::MAX_LABEL + 4);
    let d = p.options[0].description.as_ref().unwrap();
    assert!(d.len() <= QuestionPrompt::MAX_TEXT / 4 + 4, "{}", d.len());
    assert!(p.tool.unwrap().len() <= QuestionPrompt::MAX_LABEL + 4);
    // The tool input is bounded like a captured tool call (`ToolCalled`): long leaves cut, keys kept.
    let input = p.tool_input.unwrap();
    assert!(
        input.to_string().len() <= model::cap::MAX_TEXT,
        "{}",
        input.to_string().len()
    );
    assert_eq!(input["description"], "keep");
    assert!(input["command"].as_str().unwrap().contains(NOT_STORED));

    // An agent question (no tool) is cut at the end, marked with `…`.
    let q = QuestionPrompt {
        urgency: None,
        header: None,
        text: "é".repeat(10_000),
        options: vec![],
        allow_other: true,
        tool: None,
        tool_input: None,
    }
    .capped();
    assert!(
        q.text.len() <= QuestionPrompt::MAX_TEXT + 4,
        "{}",
        q.text.len()
    );
    assert!(q.text.ends_with('…'));
}

const APPROVAL_GAP: &str = "bytes not shown, read the full command before approving]";
/// Above the tool-input cap the middle is not kept anywhere, so the marker cannot ask to read it.
const NOT_STORED: &str = "bytes not stored; this command is too long to show in full, deny it unless you trust the agent]";
const DANGER: &str = "; rm -rf ~/work";

fn approval(command: &str) -> QuestionPrompt {
    QuestionPrompt {
        urgency: None,
        header: None,
        text: format!("Allow Bash to run: {command}"),
        options: vec![],
        allow_other: false,
        tool: Some("Bash".into()),
        tool_input: Some(json!({ "command": command })),
    }
    .capped()
}

#[test]
fn an_approval_prompt_keeps_a_dangerous_tail() {
    // About 5 KB: over the prompt cap, under the tool-input cap.
    let command = format!(
        "{}{DANGER}",
        "echo ok && ".repeat(456).trim_end_matches(" && ")
    );
    assert!(
        command.len() > QuestionPrompt::MAX_TEXT && command.len() < model::cap::MAX_TEXT,
        "{}",
        command.len()
    );
    let p = approval(&command);
    assert!(p.text.len() <= QuestionPrompt::MAX_TEXT, "{}", p.text.len());
    assert!(
        p.text.starts_with("Allow Bash to run: echo ok"),
        "the head stays"
    );
    assert!(
        p.text.ends_with(DANGER),
        "the tail stays visible: {}",
        &p.text[p.text.len() - 80..]
    );
    let gap = p.text.find(APPROVAL_GAP).expect("the cut says so");
    let hidden: usize = p.text[..gap]
        .rsplit('[')
        .next()
        .unwrap()
        .trim()
        .parse()
        .expect("the gap names its size");
    assert!(hidden > 0);
    assert_eq!(
        p.tool_input.unwrap()["command"],
        command,
        "the full command stays in the tool input"
    );

    // A command too long even for the tool input keeps its head and its tail there too.
    let huge = format!("{}{DANGER}", "y".repeat(200_000));
    let p = approval(&huge);
    assert!(p.text.ends_with(DANGER));
    assert!(
        p.text.contains(NOT_STORED) && !p.text.contains(APPROVAL_GAP),
        "{}",
        p.text
    );
    let kept = p.tool_input.unwrap()["command"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(
        kept.starts_with("yyyy") && kept.ends_with(DANGER) && kept.contains(NOT_STORED),
        "{}",
        kept.len()
    );
}

#[test]
fn findings_are_capped() {
    let many: Vec<Finding> = (0..200)
        .map(|i| Finding {
            text: format!("{i}{}", "y".repeat(5000)),
            location: Some("l".repeat(5000)),
        })
        .collect();
    let capped = model::cap_findings(many);
    assert_eq!(capped.len(), MAX_FINDINGS);
    assert!(capped[0].text.len() <= MAX_FINDING + 4);
    assert!(capped[0].location.as_ref().unwrap().len() <= QuestionPrompt::MAX_LABEL + 4);
}

#[test]
fn timeout_instruction_matches_the_spec() {
    assert_eq!(
        model::timeout_instruction(600_000),
        "No operator answer within 10m. Proceed with your best judgment and state the decision you made."
    );
    assert_eq!(
        model::timeout_instruction(4_000),
        "No operator answer within 4s. Proceed with your best judgment and state the decision you made."
    );
}

fn model_observed(requested: Option<&str>, observed: Option<&str>) -> EventBody {
    EventBody::ModelObserved {
        requested: requested.map(Into::into),
        observed: observed.map(Into::into),
        provider: None,
        effort: None,
        rerouted_from: None,
        reason: None,
    }
}

fn thinking(form: ThinkingForm, text: &str) -> EventBody {
    EventBody::ThinkingRecorded {
        turn_id: None,
        item_id: None,
        parent_call_id: None,
        form,
        text: text.into(),
        estimated_tokens: None,
    }
}

fn context(used_tokens: u64, window_tokens: Option<u64>, basis: ContextBasis) -> EventBody {
    EventBody::ContextReported {
        turn_id: Some("turn-1".into()),
        used_tokens,
        window_tokens,
        basis,
    }
}

/// C0 (priority briefs): the model, thinking and context records survive a round trip, every field set or not.
#[test]
fn model_thinking_and_context_bodies_round_trip() {
    let codex = EventBody::ModelObserved {
        requested: None,
        observed: Some("gpt-6-sol".into()),
        provider: Some("openai".into()),
        effort: Some("high".into()),
        rerouted_from: Some("gpt-6-astra".into()),
        reason: Some("capacity".into()),
    };
    let summary = EventBody::ThinkingRecorded {
        turn_id: Some("turn-1".into()),
        item_id: Some("rs_1".into()),
        parent_call_id: Some("toolu_1".into()),
        form: ThinkingForm::Summary,
        text: "Checking the tests first.".into(),
        estimated_tokens: Some(201),
    };
    let bodies = [
        model_observed(Some("opus"), Some("claude-opus-5-5")),
        codex,
        summary,
        thinking(ThinkingForm::Hidden, ""),
        context(18_013, Some(258_400), ContextBasis::Reported),
        context(5, None, ContextBasis::Estimated),
    ];
    for b in bodies {
        assert_eq!(
            serde_json::from_value::<EventBody>(serde_json::to_value(&b).unwrap()).unwrap(),
            b
        );
    }
}

#[test]
fn model_thinking_and_context_use_kebab_tags() {
    let tag = |b: EventBody| serde_json::to_value(b).unwrap()["type"].clone();
    assert_eq!(tag(model_observed(None, None)), "model-observed");
    assert_eq!(tag(thinking(ThinkingForm::Full, "x")), "thinking-recorded");
    assert_eq!(
        tag(context(1, None, ContextBasis::Reported)),
        "context-reported"
    );
    assert_eq!(
        serde_json::to_value(ThinkingForm::Summary).unwrap(),
        json!("summary")
    );
    assert_eq!(
        serde_json::to_value(ThinkingForm::Full).unwrap(),
        json!("full")
    );
    assert_eq!(
        serde_json::to_value(ThinkingForm::Hidden).unwrap(),
        json!("hidden")
    );
    assert_eq!(
        serde_json::to_value(ContextBasis::Reported).unwrap(),
        json!("reported")
    );
    assert_eq!(
        serde_json::to_value(ContextBasis::Estimated).unwrap(),
        json!("estimated")
    );
}

/// A record with only the required fields loads; the optional ones are unknown (None).
#[test]
fn model_and_thinking_records_without_optional_fields_load() {
    let load = |v: serde_json::Value| serde_json::from_value::<EventBody>(v).unwrap();
    assert_eq!(
        load(json!({ "type": "model-observed", "requested": null, "observed": "claude-opus-5-5" })),
        model_observed(None, Some("claude-opus-5-5"))
    );
    assert_eq!(
        load(json!({ "type": "thinking-recorded", "turn_id": null, "form": "hidden", "text": "" })),
        thinking(ThinkingForm::Hidden, "")
    );
}
