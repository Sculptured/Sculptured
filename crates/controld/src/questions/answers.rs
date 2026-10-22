//! Answers, expiries and "Send again": recorded first, then delivered to whoever waits.
use crate::error::ApiError;
use crate::hosts::CommandFailure;
use crate::projections::questions::QuestionRow;
use crate::state::AppState;
use crate::views::QuestionView;
use host_proto::HostCommand;
use model::{
    Answer, DecisionSource, EventBody, MessageIntent, Provenance, QuestionSource, QuestionState,
};

/// Why an answer (or expiry) did not reach the agent.
enum Undelivered {
    /// Certainly not delivered: the host is not connected, or the waiting tool call is gone.
    NotSent(String),
    /// The host or the agent's program replied that it did not take it (its words, verbatim).
    Refused(String),
    /// Sent, but no reply came: it may have reached the agent.
    Unconfirmed(String),
    /// Certainly not delivered: the host's agent runner is too old to pass answers on (`older_runner`).
    OlderRunner(String),
}

impl Undelivered {
    fn reason(&self) -> &str {
        match self {
            Self::NotSent(r) | Self::Refused(r) | Self::Unconfirmed(r) | Self::OlderRunner(r) => r,
        }
    }
}

/// Gives the answer to whoever waits: a seat-tool call (`request_input`, `approve`), else the agent's session.
async fn deliver(st: &AppState, q: &QuestionRow, answer: Answer) -> Result<(), Undelivered> {
    let waiter = st
        .seat_waits
        .lock()
        .expect("seat_waits mutex must not be poisoned")
        .remove(&q.id);
    if let Some(tx) = waiter {
        return tx.send(answer).map_err(|_| {
            Undelivered::NotSent("the agent stopped waiting for this answer".to_string())
        });
    }
    let host =
        crate::launch::seat_host(st, &q.seat).map_err(|e| Undelivered::NotSent(e.message))?;
    // An older hostd has no `HostCommand::Answer`: say so plainly, not "unreachable".
    st.hosts
        .check_features(&host, &[host_proto::features::ANSWER])
        .map_err(Undelivered::OlderRunner)?;
    let command = HostCommand::Answer {
        seat: q.seat.clone(),
        question_id: q.id.clone(),
        answer,
    };
    st.hosts
        .command_checked(&host, command)
        .await
        .map_err(|f| match f {
            CommandFailure::NotConnected(r) => Undelivered::NotSent(r),
            CommandFailure::Refused(r) => Undelivered::Refused(r),
            CommandFailure::NoReply(r) => Undelivered::Unconfirmed(r),
            CommandFailure::OlderRunner(r) => Undelivered::OlderRunner(r),
        })
}

pub(super) fn check_answer(q: &QuestionRow, a: &Answer) -> Result<(), ApiError> {
    let fits = match (q.source, a) {
        (_, Answer::Expired) => false,
        (QuestionSource::NativePermission, Answer::Allow | Answer::Deny { .. }) => true,
        (QuestionSource::NativePermission, Answer::Text { .. }) => false,
        (_, Answer::Text { text }) if text.trim().is_empty() => false,
        (_, Answer::Text { text }) => {
            q.prompt.allow_other
                || q.prompt.options.is_empty()
                || q.prompt.options.iter().any(|o| &o.label == text)
        }
        (_, Answer::Allow | Answer::Deny { .. }) => false,
    };
    if fits {
        Ok(())
    } else {
        Err(ApiError::bad_request(
            "bad_answer",
            "the answer does not fit the question",
        ))
    }
}

/// The overriding instruction for a late answer; None when there is nothing to override (a late "allow").
pub(crate) fn late_answer_text(q: &QuestionRow, a: &Answer) -> Option<String> {
    let asked = &q.prompt.text;
    match a {
        Answer::Text { text } => Some(format!(
            "Late answer from the operator to your question \"{asked}\": {text}. This overrides the decision you made when the question timed out."
        )),
        Answer::Deny { reason } => Some(format!(
            "Late answer from the operator: \"{asked}\" should not have been allowed{}. This overrides the automatic approval; undo what it did if you can.",
            reason
                .as_deref()
                .map(|r| format!(" ({r})"))
                .unwrap_or_default()
        )),
        Answer::Allow | Answer::Expired => None,
    }
}

fn busy() -> ApiError {
    ApiError::conflict(
        "question_busy",
        "Someone else is answering this question right now; try again in a moment.",
    )
}

fn closed() -> ApiError {
    ApiError::conflict(
        "question_closed",
        "the question was already answered or withdrawn",
    )
}

/// The 502 for a delivery nobody confirmed. The answer stays recorded, the doubt is in `QuestionView.unconfirmed`.
fn unconfirmed_error(reason: &str) -> ApiError {
    ApiError::bad_gateway(
        "host_unreachable",
        format!(
            "{reason}. The answer is recorded, but it may not have reached the agent; Send again re-sends it"
        ),
    )
}

/// Answers a question. Every answer and the timeout take the question's claim in turn, so exactly
/// one of them resolves an open question: an answer that waited behind the timeout finds the question
/// expired and becomes a late, overriding instruction (once); one that waited behind a failed answer
/// finds it open again.
pub async fn answer(st: &AppState, id: &str, a: Answer) -> Result<QuestionView, ApiError> {
    check_answer(&crate::questions::load(st, id)?, &a)?;
    let Some(_held) = crate::questions::wait_for_claim(id).await else {
        return Err(busy());
    };
    let q = crate::questions::load(st, id)?;
    match q.state {
        QuestionState::Open => answer_open(st, q, a).await,
        QuestionState::Expired if q.late_answer.is_none() => answer_late(st, q, a).await,
        _ => Err(closed()),
    }
}

/// Records the answer, then delivers it (ruling Q1): the log reads raised → answered → the agent's reply,
/// and an expiry that comes later finds the question answered. An answer that certainly did not arrive
/// opens the question again; one whose delivery is unconfirmed stays answered and is recorded as in doubt.
async fn answer_open(st: &AppState, q: QuestionRow, a: Answer) -> Result<QuestionView, ApiError> {
    let answered = EventBody::QuestionAnswered {
        question_id: q.id.clone(),
        answer: a.clone(),
        late: false,
    };
    crate::questions::record(
        st,
        &q.seat,
        crate::questions::human(),
        Provenance::Operator,
        answered,
    )?;
    // The projection may have closed it in between (its process ended, its turn was interrupted):
    // then nothing waits for the answer, so nothing is sent.
    if crate::questions::load(st, &q.id)?.state != QuestionState::Answered {
        return Err(closed());
    }
    match deliver(st, &q, a).await {
        Ok(()) => Ok(crate::questions::view(
            st,
            crate::questions::load(st, &q.id)?,
        )),
        Err(Undelivered::Unconfirmed(reason)) => {
            note_unconfirmed(st, &q, &reason)?;
            Err(unconfirmed_error(&reason))
        }
        Err(failed) => Err(answer_failed(st, &q, failed)?),
    }
}

/// An answer that certainly did not arrive: recorded as failed (the question opens again); returns the refusal.
fn answer_failed(
    st: &AppState,
    q: &QuestionRow,
    failed: Undelivered,
) -> Result<ApiError, ApiError> {
    let reason = failed.reason().to_string();
    let body = EventBody::QuestionAnswerFailed {
        question_id: q.id.clone(),
        reason: reason.clone(),
    };
    crate::questions::record(
        st,
        &q.seat,
        crate::hosts::controld(),
        Provenance::ProtocolObserved,
        body,
    )?;
    Ok(match failed {
        Undelivered::Refused(_) => ApiError::conflict("answer_refused", reason),
        Undelivered::OlderRunner(_) => ApiError::conflict("older_runner", reason),
        _ => ApiError::bad_gateway("host_unreachable", reason),
    })
}

/// Records that what Sculptured sent for `q` may not have reached the agent (`reason`: verbatim).
fn note_unconfirmed(st: &AppState, q: &QuestionRow, reason: &str) -> Result<(), ApiError> {
    let body = EventBody::QuestionAnswerUnconfirmed {
        question_id: q.id.clone(),
        reason: reason.to_string(),
    };
    crate::questions::record(
        st,
        &q.seat,
        crate::hosts::controld(),
        Provenance::ProtocolObserved,
        body,
    )
}

async fn answer_late(st: &AppState, q: QuestionRow, a: Answer) -> Result<QuestionView, ApiError> {
    if let Some(text) = late_answer_text(&q, &a) {
        crate::reconciler::send_message_as(
            st,
            &q.seat,
            text,
            MessageIntent::Instruct,
            false,
            crate::questions::human(),
        )
        .await?;
    }
    crate::questions::record(
        st,
        &q.seat,
        crate::questions::human(),
        Provenance::Operator,
        EventBody::QuestionAnswered {
            question_id: q.id.clone(),
            answer: a,
            late: true,
        },
    )?;
    Ok(crate::questions::view(
        st,
        crate::questions::load(st, &q.id)?,
    ))
}

/// "Send again": re-sends the recorded answer (or, for an expired question, the expiry) whose delivery is
/// in doubt, under the question's claim. It records no new answer. Confirmed, the doubt clears; refused,
/// the question stays answered (never reopened) and the refusal, verbatim, becomes the doubt's reason;
/// unconfirmed again, the new reason replaces the old.
pub async fn send_again(st: &AppState, id: &str) -> Result<QuestionView, ApiError> {
    crate::questions::load(st, id)?;
    let Some(_held) = crate::questions::wait_for_claim(id).await else {
        return Err(busy());
    };
    let q = crate::questions::load(st, id)?;
    let resend = answer_to_resend(&q)?;
    match deliver(st, &q, resend).await {
        Ok(()) => {
            crate::questions::record(
                st,
                &q.seat,
                crate::hosts::controld(),
                Provenance::ProtocolObserved,
                EventBody::QuestionAnswerConfirmed {
                    question_id: q.id.clone(),
                },
            )?;
            allowed_on_timeout(st, &q.id);
            Ok(crate::questions::view(
                st,
                crate::questions::load(st, &q.id)?,
            ))
        }
        Err(Undelivered::Unconfirmed(reason)) => {
            note_unconfirmed(st, &q, &reason)?;
            Err(unconfirmed_error(&reason))
        }
        Err(Undelivered::Refused(reason)) => {
            note_unconfirmed(st, &q, &reason)?;
            Err(ApiError::conflict("answer_refused", reason))
        }
        Err(Undelivered::OlderRunner(reason)) => Err(ApiError::conflict("older_runner", reason)),
        Err(Undelivered::NotSent(reason)) => Err(ApiError::bad_gateway("host_unreachable", reason)),
    }
}

/// What "Send again" sends: the recorded answer, or the expiry. Refused for an open question
/// and for one the agent already has.
fn answer_to_resend(q: &QuestionRow) -> Result<Answer, ApiError> {
    match (q.state, &q.answer) {
        (QuestionState::Open, _) => Err(ApiError::conflict(
            "question_open",
            "answer the question instead",
        )),
        (QuestionState::Answered | QuestionState::Expired, _) if q.unconfirmed.is_none() => {
            Err(ApiError::conflict(
                "answer_confirmed",
                "the agent has this answer; there is nothing to send again",
            ))
        }
        (QuestionState::Answered, Some(a)) => Ok(a.clone()),
        (QuestionState::Expired, _) => Ok(Answer::Expired),
        _ => Err(closed()),
    }
}

/// D8 for an expired permission prompt whose expiry is now known to have reached the program (a "Send
/// again", or the program's own resolved notice): it was allowed. Only when no decision is recorded yet.
pub(super) fn allowed_on_timeout(st: &AppState, id: &str) {
    let Ok(q) = crate::questions::load(st, id) else {
        return;
    };
    if q.state != QuestionState::Expired
        || q.source != QuestionSource::NativePermission
        || q.decision.is_some()
    {
        return;
    }
    let body = EventBody::QuestionDecided {
        question_id: q.id.clone(),
        decision: "allowed automatically: nobody answered in time".to_string(),
        rationale: None,
        source: DecisionSource::AllowedOnTimeout,
    };
    crate::questions::record_or_log(
        st,
        &q.seat,
        (
            crate::questions::timeout_actor(),
            Provenance::ProtocolObserved,
        ),
        body,
        id,
        "cannot record the decision",
    );
}

/// D8: on expiry an agent question is told to decide; a permission prompt is allowed and that is recorded.
/// The expiry is recorded before it is delivered: the agent's reply (its decision) must come after it
/// in the event order, and hostd forwards a command's events before the command's result. An expiry
/// whose delivery is unconfirmed is recorded as in doubt ("may not have reached the agent") and decides
/// nothing yet: the agent's own words, a "Send again", or the end of its turn decide it later.
pub async fn expire(st: &AppState, id: &str) {
    let Some(_held) = crate::questions::claim(id) else {
        return;
    };
    let Ok(q) = crate::questions::load(st, id) else {
        return;
    };
    if q.state != QuestionState::Open {
        return;
    }
    #[cfg(feature = "test-seams")]
    crate::seams::run(&crate::seams::IN_EXPIRE_BEFORE_RECORD, id);
    let expired = EventBody::QuestionExpired {
        question_id: id.into(),
    };
    if let Err(e) = crate::questions::record(
        st,
        &q.seat,
        crate::questions::timeout_actor(),
        Provenance::ProtocolObserved,
        expired,
    ) {
        tracing::error!(question = %id, error = %e.message, "cannot record the expiry");
        return;
    }
    let delivered = deliver(st, &q, Answer::Expired).await;
    expiry_delivered(st, &q, delivered);
}

/// After an expiry's delivery: a permission prompt is allowed (for a question, the decision watcher
/// reads the agent's own words next); a doubtful one is noted; one that failed decides "unknown".
fn expiry_delivered(st: &AppState, q: &QuestionRow, delivered: Result<(), Undelivered>) {
    match delivered {
        Ok(()) => allowed_on_timeout(st, &q.id),
        Err(Undelivered::Unconfirmed(reason)) => {
            if let Err(e) = note_unconfirmed(st, q, &reason) {
                tracing::error!(question = %q.id, error = %e.message, "cannot record the doubt");
            }
        }
        Err(e) => {
            let body = EventBody::QuestionDecided {
                question_id: q.id.clone(),
                decision: "unknown".to_string(),
                rationale: Some(format!(
                    "the timeout could not reach the agent: {}",
                    e.reason()
                )),
                source: DecisionSource::Unknown,
            };
            crate::questions::record_or_log(
                st,
                &q.seat,
                (crate::questions::timeout_actor(), Provenance::Heuristic),
                body,
                &q.id,
                "cannot record the decision",
            );
        }
    }
}

pub fn acknowledge(st: &AppState, id: &str) -> Result<QuestionView, ApiError> {
    let q = crate::questions::load(st, id)?;
    if q.state == QuestionState::Open {
        return Err(ApiError::conflict(
            "question_open",
            "answer the question instead",
        ));
    }
    if !q.acknowledged {
        crate::questions::record(
            st,
            &q.seat,
            crate::questions::human(),
            Provenance::Operator,
            EventBody::QuestionAcknowledged {
                question_id: id.into(),
            },
        )?;
    }
    Ok(crate::questions::view(st, crate::questions::load(st, id)?))
}
