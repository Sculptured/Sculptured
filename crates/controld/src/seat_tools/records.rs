//! `record_decision` and `report`: what an agent reports, recorded as agent-reported evidence.
use super::ToolError;
use crate::projections::questions::QuestionRow;
use crate::state::AppState;
use model::{
    DecisionSource, EventBody, Finding, Provenance, QuestionSource, QuestionState, ReportVerdict,
};
use serde_json::{Value, json};

const SUMMARY_CHARS: usize = 1000;
const DECISION_CHARS: usize = 500;

/// The agent's own decision on a question it asked that timed out.
pub(super) fn record_decision(st: &AppState, seat: &str, args: &Value) -> Result<Value, ToolError> {
    let id = crate::seat_tools::string_arg(args, "question_id")?;
    let choice = crate::seat_tools::string_arg(args, "choice")?;
    let q = crate::projections::questions::question(
        st.store
            .lock()
            .expect("store mutex must not be poisoned")
            .connection(),
        id,
    )
    .map_err(|e| ToolError::Failed(e.to_string()))?
    .filter(|q| q.seat == seat)
    .ok_or_else(|| {
        crate::seat_tools::invalid(format!(
            "{:?} is not one of your questions",
            crate::seat_tools::cut(id, 200)
        ))
    })?;
    check_decidable(&q)?;
    let rationale = args
        .get("rationale")
        .and_then(Value::as_str)
        .map(|r| crate::seat_tools::cut(r, DECISION_CHARS));
    let decision = crate::seat_tools::cut(choice, DECISION_CHARS);
    let body = EventBody::QuestionDecided {
        question_id: q.id,
        decision,
        rationale,
        source: DecisionSource::RecordDecision,
    };
    crate::seat_tools::record(st, seat, Provenance::AgentReported, body)?;
    Ok(json!({ "text": "Recorded your decision." }))
}

/// Only the agent's own question (not a permission prompt, which Sculptured decides), only once it
/// timed out, and never over a decision Sculptured observed or recorded itself (allowed on timeout,
/// or unknown).
fn check_decidable(q: &QuestionRow) -> Result<(), ToolError> {
    if !matches!(
        q.source,
        QuestionSource::RequestInput | QuestionSource::NativeQuestion
    ) {
        return Err(crate::seat_tools::invalid(format!(
            "question {:?} is a permission prompt; Sculptured records what happened to it",
            q.id
        )));
    }
    if q.state != QuestionState::Expired {
        return Err(crate::seat_tools::invalid(format!(
            "question {:?} did not time out, so there is no decision to record",
            q.id
        )));
    }
    if q.decision.as_ref().is_some_and(|d| {
        matches!(
            d.source,
            DecisionSource::AllowedOnTimeout | DecisionSource::Unknown
        )
    }) {
        return Err(crate::seat_tools::invalid(format!(
            "Sculptured already recorded what happened to question {:?}",
            q.id
        )));
    }
    Ok(())
}

fn finding_of(v: &Value) -> Result<Finding, ToolError> {
    let bad = || {
        crate::seat_tools::invalid("each finding must be a non-empty string or {text, location}")
    };
    match v {
        Value::String(s) if !s.trim().is_empty() => Ok(Finding {
            text: s.trim().to_string(),
            location: None,
        }),
        Value::Object(o) => match o.get("text").and_then(Value::as_str).map(str::trim) {
            Some(t) if !t.is_empty() => Ok(Finding {
                text: t.to_string(),
                location: o
                    .get("location")
                    .and_then(Value::as_str)
                    .map(str::to_string),
            }),
            _ => Err(bad()),
        },
        _ => Err(bad()),
    }
}

/// Findings about the agent's work, on the turn that is running.
pub(super) fn report(st: &AppState, seat: &str, args: &Value) -> Result<Value, ToolError> {
    let summary = crate::seat_tools::cut(
        crate::seat_tools::string_arg(args, "summary")?,
        SUMMARY_CHARS,
    );
    let findings = match args.get("findings") {
        None | Some(Value::Null) => vec![],
        Some(Value::Array(a)) => a.iter().map(finding_of).collect::<Result<Vec<_>, _>>()?,
        Some(_) => return Err(crate::seat_tools::invalid("findings must be a list")),
    };
    let verdict = match args.get("verdict").and_then(Value::as_str) {
        None if findings.is_empty() => ReportVerdict::NoProblems,
        None => ReportVerdict::ProblemsFound,
        Some(v) => serde_json::from_value(Value::String(v.to_string())).map_err(|_| {
            crate::seat_tools::invalid("verdict must be no-problems, problems-found or unsure")
        })?,
    };
    let body = EventBody::FindingsReported {
        turn_id: crate::seat_tools::current_turn(st, seat),
        summary,
        findings: model::cap_findings(findings),
        verdict,
    };
    crate::seat_tools::record(st, seat, Provenance::AgentReported, body)?;
    Ok(json!({ "text": "Recorded your report." }))
}
