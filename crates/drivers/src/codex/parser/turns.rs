//! Turn endings and token usage.
use super::{CodexParser, Parsed};
use crate::DriverEvent;
use crate::codex::TOOL_UNFINISHED;
use model::{LimitKind, TurnStatus};
use serde_json::{Value, json};

/// Cumulative token counts of one thread.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct Totals {
    input: u64,
    cached: u64,
    cache_write: u64,
    output: u64,
}

impl Totals {
    fn from(v: &Value) -> Self {
        let n = |k: &str| v[k].as_u64().unwrap_or(0);
        Self {
            input: n("inputTokens"),
            cached: n("cachedInputTokens"),
            cache_write: n("cacheWriteInputTokens"),
            output: n("outputTokens"),
        }
    }
    fn since(self, b: Self) -> Self {
        Self {
            input: self.input.saturating_sub(b.input),
            cached: self.cached.saturating_sub(b.cached),
            cache_write: self.cache_write.saturating_sub(b.cache_write),
            output: self.output.saturating_sub(b.output),
        }
    }
}

/// Which limit ended a failed turn, from its `TurnError` (schema 0.160.0/0.160.1; no limit was ever
/// recorded). Never retried by Sculptured.
/// - `usageLimitExceeded`, and `sessionBudgetExceeded` (an exhausted allowance too: a retry would hit
///   it again), are usage limits; `rateLimitExceeded` is a rate limit.
/// - `{responseTooManyFailedAttempts: {httpStatusCode: 429}}`: Codex gave up retrying on HTTP 429
///   (Too Many Requests), which is a rate limit.
/// - Only an error Codex did not classify (`codexErrorInfo` null, `"other"` or an object form) falls
///   back to the Claude driver's reading of the message words, so a limit that arrives that way is
///   still told to the operator. A classified string (`serverOverloaded`, `contextWindowExceeded`, …)
///   already says what went wrong and is never re-read as a limit (Task 13 carry-forward).
///
/// Called for `failed` turns only: an interrupted or completed turn is never a limit.
fn limit_of(error: &Value) -> Option<LimitKind> {
    let info = &error["codexErrorInfo"];
    match info.as_str() {
        Some("usageLimitExceeded" | "sessionBudgetExceeded") => return Some(LimitKind::Usage),
        Some("rateLimitExceeded") => return Some(LimitKind::Rate),
        Some("other") => {}
        Some(_) => return None,
        None => {}
    }
    if info["responseTooManyFailedAttempts"]["httpStatusCode"].as_u64() == Some(429) {
        return Some(LimitKind::Rate);
    }
    if !(info.is_null() || info.is_object() || info == "other") {
        return None;
    }
    crate::claude::limit_kind(error["message"].as_str().unwrap_or(""))
}

fn turn_status(turn: &Value) -> TurnStatus {
    match turn["status"].as_str() {
        Some("completed") => TurnStatus::Completed,
        Some("interrupted") => TurnStatus::Interrupted,
        _ => TurnStatus::Failed,
    }
}

impl CodexParser {
    pub(super) fn token_usage(&mut self, p: &Value, turn: String, out: &mut Parsed) {
        let thread = crate::codex::parser::s(&p["threadId"])
            .or_else(|| self.thread.clone())
            .unwrap_or_default();
        let total = Totals::from(&p["tokenUsage"]["total"]);
        let ours = self.started.contains(&(thread.clone(), turn.clone()));
        let before = self.seen.insert(thread, total).unwrap_or_default();
        let d = total.since(before);
        // `last` is the latest model call: its input (cached part included) is the context it read.
        let last = Totals::from(&p["tokenUsage"]["last"]);
        if ours && last.input > 0 {
            out.events.push(DriverEvent::Context {
                turn_id: Some(turn.clone()),
                used_tokens: last.input,
                window_tokens: p["tokenUsage"]["modelContextWindow"].as_u64(),
                basis: model::ContextBasis::Reported,
            });
        }
        if ours && d != Totals::default() {
            out.events.push(DriverEvent::Usage {
                turn_id: Some(turn),
                input_tokens: d.input.saturating_sub(d.cached),
                output_tokens: d.output,
                cache_read_tokens: d.cached,
                cache_write_tokens: d.cache_write,
                cost: self.model.as_deref().and_then(|m| {
                    model::pricing::openai_cost(
                        m,
                        d.input.saturating_sub(d.cached),
                        d.cached,
                        d.output,
                    )
                }),
                in_turn: false,
            });
        }
    }

    pub(super) fn turn_completed(&mut self, p: &Value, turn: String, out: &mut Parsed) {
        let t = crate::codex::parser::s(&p["turn"]["id"]).unwrap_or(turn);
        self.close_open_calls(&t, out);
        let status = turn_status(&p["turn"]);
        let error = &p["turn"]["error"];
        if !error.is_null() {
            out.events.push(DriverEvent::Unrecognized {
                raw: json!({ "turnError": error }),
            });
            if status == TurnStatus::Failed
                && let Some(kind) = limit_of(error)
            {
                let text = error["message"].as_str().unwrap_or("").to_string();
                out.events.push(DriverEvent::LimitReached {
                    turn_id: Some(t.clone()),
                    kind,
                    resets: model::limit_resets(&text),
                    text,
                });
            }
        }
        self.unknown_items.retain(|_, item_turn| *item_turn != t);
        if self.current.as_deref() == Some(t.as_str()) {
            self.current = None;
        }
        out.events
            .push(DriverEvent::TurnEnded { turn_id: t, status });
        out.turn_ended = true;
    }

    /// Tool calls of turn `t` that never finished get an error result.
    fn close_open_calls(&mut self, t: &str, out: &mut Parsed) {
        let (closing, open): (Vec<_>, Vec<_>) =
            self.open_calls.drain(..).partition(|(_, ct)| ct == t);
        self.open_calls = open;
        for (call_id, _) in closing {
            self.items.remove(&call_id);
            out.events.push(DriverEvent::ToolResult {
                turn_id: Some(t.to_string()),
                call_id,
                output: TOOL_UNFINISHED.into(),
                is_error: true,
            });
        }
    }
}
