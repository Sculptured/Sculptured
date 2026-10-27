//! `propose_change`: an agent asks before changing a business system (an order, a reschedule, a record in
//! the ERP). Sculptured checks the team's rules itself (`model::mandate`), then approves the change when
//! every rule that applies passes and the team allows that, or else asks a person in Needs you. Nobody
//! answering in time means not approved. The proposal changes nothing by itself; the reply tells the agent
//! whether it may.
use super::{Outcome, ToolError, ask, invalid, string_arg};
use crate::state::AppState;
use model::{
    Actor, Answer, ChangeDecider, EventBody, Mandate, Provenance, QuestionPrompt, QuestionSource,
    RuleResult, Subject,
};
use serde_json::{Value, json};

/// The most a proposal's payload may hold.
pub const MAX_PAYLOAD: usize = 32 * 1024;

struct Proposal {
    id: String,
    action: String,
    system: Option<String>,
    summary: String,
    payload: Value,
    reason: Option<String>,
}

fn optional(args: &Value, key: &str) -> Option<String> {
    args.get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

fn parse(args: &Value) -> Result<Proposal, ToolError> {
    let action = string_arg(args, "action")?;
    if action.chars().count() > 80 {
        return Err(invalid("action must be at most 80 characters"));
    }
    let payload = args
        .get("payload")
        .filter(|p| p.is_object())
        .cloned()
        .ok_or_else(|| invalid("payload must be an object: the change's fields"))?;
    if payload.to_string().len() > MAX_PAYLOAD {
        return Err(invalid("payload must be at most 32 KB"));
    }
    Ok(Proposal {
        id: format!("chg-{}", uuid::Uuid::new_v4().simple()),
        action: action.to_string(),
        system: optional(args, "system"),
        summary: super::tool_calls::cut(string_arg(args, "summary")?, 300),
        payload,
        reason: optional(args, "reason").map(|r| super::tool_calls::cut(&r, 2000)),
    })
}

/// Records an event about the proposal on the team.
fn record(
    st: &AppState,
    team: &str,
    actor: Actor,
    provenance: Provenance,
    body: EventBody,
) -> Result<(), ToolError> {
    st.local_event(actor, Subject::Rig(team.to_string()), provenance, body)
        .map(|_| ())
        .map_err(|e| ToolError::Failed(format!("Sculptured could not record this: {e}")))
}

fn mandate_of(st: &AppState, team: &str) -> Mandate {
    st.store
        .lock()
        .expect("store mutex must not be poisoned")
        .draft(team)
        .ok()
        .flatten()
        .map(|d| d.mandate)
        .unwrap_or_default()
}

/// The question a person answers: the change, the agent's reason and every rule's result.
fn approval_prompt(p: &Proposal, results: &[RuleResult]) -> QuestionPrompt {
    QuestionPrompt {
        urgency: None,
        header: Some("Approve a change".into()),
        text: p.summary.clone(),
        options: vec![],
        allow_other: false,
        tool: Some(p.action.clone()),
        tool_input: Some(json!({
            "proposal_id": p.id, "system": p.system, "payload": p.payload,
            "reason": p.reason, "checks": results,
        })),
    }
}

/// The reply: what was decided and why, and what the agent may do now.
fn reply(p: &Proposal, results: &[RuleResult], approved: bool, why: &str) -> Value {
    let next = if approved {
        "Carry out exactly this change, then say what you did."
    } else {
        "Do not make this change. Say so in your reply, and propose another if it helps."
    };
    let checks: Vec<String> = results
        .iter()
        .map(|r| format!("{} {}", if r.passed { "✓" } else { "✗" }, r.words))
        .collect();
    let text = format!(
        "{why}. {next} (proposal {}; rules: {})",
        p.id,
        if checks.is_empty() {
            "none apply".to_string()
        } else {
            checks.join("; ")
        }
    );
    json!({ "text": text, "proposal_id": p.id, "approved": approved, "checks": results })
}

pub(super) async fn propose_change(
    st: &AppState,
    seat: &str,
    args: &Value,
) -> Result<Outcome, ToolError> {
    let team = crate::seat_ids::rig_of(seat)
        .ok_or_else(|| invalid("unknown agent"))?
        .to_string();
    let p = parse(args)?;
    let mandate = mandate_of(st, &team);
    let results = model::mandate::evaluate(&mandate, &p.action, &p.payload);
    let me = Actor::Seat(seat.to_string());
    let sculptured = Actor::System("controld".into());
    let proposed = EventBody::ChangeProposed {
        proposal_id: p.id.clone(),
        action: p.action.clone(),
        system: p.system.clone(),
        summary: p.summary.clone(),
        payload: p.payload.clone(),
        reason: p.reason.clone(),
    };
    record(st, &team, me, Provenance::AgentReported, proposed)?;
    let checked = EventBody::ChangeChecked {
        proposal_id: p.id.clone(),
        results: results.clone(),
    };
    record(st, &team, sculptured.clone(), Provenance::Verified, checked)?;
    // No rule applying is no evidence: only a change that passes rules is approved without a person.
    if mandate.approve_within && !results.is_empty() && results.iter().all(|r| r.passed) {
        decide(st, &team, &p.id, (true, ChangeDecider::Mandate, None))?;
        return Ok((
            reply(&p, &results, true, "Approved by the team's rules"),
            None,
        ));
    }
    let (qid, answer) = ask::ask(
        st,
        seat,
        QuestionSource::Approval,
        approval_prompt(&p, &results),
    )
    .await?;
    let decision = decision_of(&answer);
    let why = match (&decision.1, decision.0, &decision.2) {
        (ChangeDecider::NobodyInTime, ..) => "Not approved: nobody answered in time".to_string(),
        (_, true, _) => "Approved by a person".to_string(),
        (_, false, Some(note)) => format!("Not approved: {note}"),
        (_, false, None) => "Not approved by a person".to_string(),
    };
    let approved = decision.0;
    decide(st, &team, &p.id, decision)?;
    Ok((reply(&p, &results, approved, &why), Some((qid, answer))))
}

type Decision = (bool, ChangeDecider, Option<String>);

fn decision_of(answer: &Answer) -> Decision {
    match answer {
        Answer::Allow => (true, ChangeDecider::Person, None),
        Answer::Deny { reason } => (false, ChangeDecider::Person, reason.clone()),
        Answer::Text { text } => (false, ChangeDecider::Person, Some(text.clone())),
        Answer::Expired => (false, ChangeDecider::NobodyInTime, None),
    }
}

fn decide(
    st: &AppState,
    team: &str,
    id: &str,
    (approved, by, note): Decision,
) -> Result<(), ToolError> {
    let provenance = match by {
        ChangeDecider::Person => Provenance::Operator,
        ChangeDecider::Mandate | ChangeDecider::NobodyInTime => Provenance::Verified,
    };
    let body = EventBody::ChangeDecided {
        proposal_id: id.to_string(),
        approved,
        by,
        note,
    };
    record(st, team, Actor::System("controld".into()), provenance, body)
}
