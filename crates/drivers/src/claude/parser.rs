//! stream-json lines → driver events. Pure, so it is unit-tested against real M0 recordings.
use crate::DriverEvent;
use model::{LimitKind, TurnStatus};
use serde_json::Value;
use std::collections::VecDeque;

/// Session-cumulative token sums from `result.modelUsage`, summed over every model.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct UsageTotals {
    input: u64,
    output: u64,
    cache_read: u64,
    cache_write: u64,
    /// Claude Code's own `costUSD`, summed over models; None when it reported none.
    cost: Option<f64>,
}

impl UsageTotals {
    /// None when the line has no `modelUsage`; usage is then unknown, not zero.
    fn from_model_usage(model_usage: &Value) -> Option<Self> {
        let models = model_usage.as_object()?;
        let mut t = Self::default();
        for m in models.values() {
            let n = |k: &str| m[k].as_u64().unwrap_or(0);
            t.input += n("inputTokens");
            t.output += n("outputTokens");
            t.cache_read += n("cacheReadInputTokens");
            t.cache_write += n("cacheCreationInputTokens");
            if let Some(c) = m["costUSD"].as_f64() {
                t.cost = Some(t.cost.unwrap_or(0.0) + c);
            }
        }
        Some(t)
    }

    /// Usage of one API message, from `message.usage` (no cost).
    fn from_message_usage(u: &Value) -> Option<Self> {
        let n = |k: &str| u[k].as_u64().unwrap_or(0);
        u.is_object().then(|| Self {
            input: n("input_tokens"),
            output: n("output_tokens"),
            cache_read: n("cache_read_input_tokens"),
            cache_write: n("cache_creation_input_tokens"),
            cost: None,
        })
    }

    fn max(self, o: Self) -> Self {
        Self {
            input: self.input.max(o.input),
            output: self.output.max(o.output),
            cache_read: self.cache_read.max(o.cache_read),
            cache_write: self.cache_write.max(o.cache_write),
            cost: None,
        }
    }

    fn plus(self, o: Self) -> Self {
        Self {
            input: self.input + o.input,
            output: self.output + o.output,
            cache_read: self.cache_read + o.cache_read,
            cache_write: self.cache_write + o.cache_write,
            cost: None,
        }
    }

    fn is_zero(&self) -> bool {
        self.input + self.output + self.cache_read + self.cache_write == 0
    }

    fn since(self, base: Self) -> Self {
        Self {
            input: self.input.saturating_sub(base.input),
            output: self.output.saturating_sub(base.output),
            cache_read: self.cache_read.saturating_sub(base.cache_read),
            cache_write: self.cache_write.saturating_sub(base.cache_write),
            cost: self.cost.map(|c| (c - base.cost.unwrap_or(0.0)).max(0.0)),
        }
    }
}

/// Turns stream-json lines into DriverEvents. Pure, so it is unit-tested
/// against real M0 recordings.
#[derive(Default)]
pub struct ClaudeParser {
    turn_seq: u32,
    current_turn: Option<String>,
    pending: VecDeque<String>,
    interrupt_requested: bool,
    /// The current turn's last main-thread agent text, read when the turn fails (limit words).
    last_text: Option<String>,
    /// The last cumulative `modelUsage` sums seen in this process. A turn's usage
    /// is the difference from this. It starts at zero for a fresh process.
    /// `modelUsage` carries across `--resume` (claude-resume-2), so a resumed
    /// session seeds it with the previous tenure's last cumulative sums through
    /// `ClaudeParser::with_usage_baseline`.
    /// Resumed with a token baseline only: the next turn's cost can't be told apart from earlier ones.
    cost_unknown: bool,
    usage_seen: UsageTotals,
    /// This turn's usage already reported from its API messages, and each message's largest usage.
    in_turn: UsageTotals,
    messages: std::collections::HashMap<String, UsageTotals>,
}

fn result_text(content: &Value) -> String {
    match content {
        Value::String(s) => s.clone(),
        Value::Array(parts) => parts
            .iter()
            .filter_map(|p| p["text"].as_str())
            .collect::<Vec<_>>()
            .join("\n"),
        Value::Null => String::new(),
        other => other.to_string(),
    }
}

/// The limit a failed turn hit, with its words. The user's real run: "You've hit your session
/// limit · resets 4:30am (America/New_York)", as the turn's agent text or the result's words, then
/// a failed turn. The result and its errors are the CLI's own words (`limit_kind`); the agent's last
/// text counts only when it starts with the CLI's limit message (`cli_limit_message`), so an agent
/// writing about rate limits is never taken for one. Kept verbatim; nothing retries the turn.
fn limit_words(v: &Value, last_text: Option<String>) -> Option<(LimitKind, String)> {
    std::iter::once(v["result"].as_str().unwrap_or(""))
        .chain(
            v["errors"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(Value::as_str),
        )
        .find_map(|t| crate::claude::limits::limit_kind(t).map(|kind| (kind, t.to_string())))
        .or_else(|| {
            last_text
                .and_then(|t| crate::claude::limits::cli_limit_message(&t).map(|kind| (kind, t)))
        })
}

impl ClaudeParser {
    /// A parser for a resumed session whose cumulative usage so far is `b` (input, output, cache read,
    /// cache write). `modelUsage` carries across `--resume` (claude-resume-2), so without this the first
    /// turn after a resume would count the whole session again.
    #[must_use]
    pub fn with_usage_baseline(b: [u64; 4]) -> Self {
        // The cost so far isn't known, so the first turn's cost is unknown rather than the whole session's.
        Self {
            usage_seen: UsageTotals {
                input: b[0],
                output: b[1],
                cache_read: b[2],
                cache_write: b[3],
                cost: None,
            },
            cost_unknown: true,
            ..Self::default()
        }
    }

    pub fn on_sent(&mut self, message_id: String) {
        self.pending.push_back(message_id);
    }

    /// A no-op when no turn is running: there is nothing to interrupt.
    pub fn on_interrupt(&mut self) {
        if self.current_turn.is_some() {
            self.interrupt_requested = true;
        }
    }

    fn ensure_turn(&mut self, out: &mut Vec<DriverEvent>) -> String {
        if let Some(t) = &self.current_turn {
            return t.clone();
        }
        self.turn_seq += 1;
        let t = format!("turn-{}", self.turn_seq);
        self.current_turn = Some(t.clone());
        out.push(DriverEvent::TurnStarted { turn_id: t.clone() });
        t
    }

    pub fn parse_line(&mut self, line: &str) -> Vec<DriverEvent> {
        let mut out = Vec::new();
        let v: Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(_) => {
                return vec![DriverEvent::Unrecognized {
                    raw: Value::String(line.to_string()),
                }];
            }
        };
        match v["type"].as_str().unwrap_or("") {
            "system" => Self::system(v, &mut out),
            // control_response is nested `response.response` (claude-interrupt@2355);
            // nothing in M1 reads it.
            "rate_limit_event" => out.extend(
                model::PlanUsage::from_claude(&v["rate_limit_info"]).map(DriverEvent::PlanUsage),
            ),
            "control_response" => {}
            "user" => self.user(&v, &mut out),
            "assistant" => self.assistant(&v, &mut out),
            "result" => self.result(&v, &mut out),
            _ => out.push(DriverEvent::Unrecognized { raw: v }),
        }
        out
    }

    fn system(v: Value, out: &mut Vec<DriverEvent>) {
        match v["subtype"].as_str().unwrap_or("") {
            // init never arrives before the first user message and repeats at the
            // start of every turn (claude-queue@927 and @2143). SessionReady is
            // emitted at spawn, so init must neither signal readiness nor start a turn.
            "init" | "hook_started" | "hook_response" | "thinking_tokens" => {}
            // Subagent bookkeeping. The `Agent` tool call and the child lines that
            // carry `parent_tool_use_id` already give M1 what it shows.
            "task_started" | "task_progress" | "task_updated" | "task_notification" => {}
            _ => out.push(DriverEvent::Unrecognized { raw: v }),
        }
    }

    fn user(&mut self, v: &Value, out: &mut Vec<DriverEvent>) {
        if v["isReplay"] == true && v["parent_tool_use_id"].is_null() {
            // A replayed operator message: the agent has taken it in.
            let turn = self.ensure_turn(out);
            if let Some(id) = self.pending.pop_front() {
                out.push(DriverEvent::MessageConsumed {
                    message_id: id,
                    turn_id: turn,
                });
            }
            return;
        }
        // Tool results, including subagent children and the synthetic
        // `is_error` result after an interrupt. Other user lines, such as the
        // interrupt marker (claude-interrupt@2431) and a subagent's prompt
        // (claude-subagent@3277), consume nothing and yield nothing.
        let content = &v["message"]["content"];
        let matching_content = content
            .as_array()
            .into_iter()
            .flatten()
            .filter(|b| b["type"] == "tool_result");
        for b in matching_content {
            out.push(DriverEvent::ToolResult {
                turn_id: self.current_turn.clone(),
                call_id: b["tool_use_id"].as_str().unwrap_or_default().to_string(),
                output: result_text(&b["content"]),
                is_error: b["is_error"].as_bool().unwrap_or(false),
            });
        }
    }

    fn assistant(&mut self, v: &Value, out: &mut Vec<DriverEvent>) {
        let parent = v["parent_tool_use_id"].as_str().map(str::to_string);
        let turn = self.ensure_turn(out);
        // A native subagent's calls count as they happen too (`modelUsage` at the end includes them).
        self.message_usage(&v["message"], &turn, parent.is_none(), out);
        let matching_items = v["message"]["content"]
            .as_array()
            .cloned()
            .unwrap_or_default();
        for b in matching_items {
            match b["type"].as_str().unwrap_or("") {
                "text" => {
                    let text = b["text"].as_str().unwrap_or_default().to_string();
                    if parent.is_none() {
                        // Kept for a failed ending: the CLI may say its limit words as agent text.
                        self.last_text =
                            Some(model::cap::truncate(text.clone(), model::cap::MAX_TEXT));
                    }
                    out.push(DriverEvent::Text {
                        turn_id: Some(turn.clone()),
                        text,
                    });
                }
                "tool_use" => out.push(DriverEvent::ToolCall {
                    turn_id: Some(turn.clone()),
                    call_id: b["id"].as_str().unwrap_or_default().to_string(),
                    name: b["name"].as_str().unwrap_or_default().to_string(),
                    input: b["input"].clone(),
                    parent_call_id: parent.clone(),
                }),
                "thinking" | "redacted_thinking" => {}
                _ => out.push(DriverEvent::Unrecognized { raw: b }),
            }
        }
    }

    /// `message.usage` is per API message and repeats on every line of that message (output growing),
    /// so each message's largest values are kept and only their growth is reported, while the turn runs.
    /// The first line of each of the main conversation's calls also reports that call's context size.
    fn message_usage(&mut self, m: &Value, turn: &str, main: bool, out: &mut Vec<DriverEvent>) {
        let (Some(id), Some(now)) = (
            m["id"].as_str(),
            UsageTotals::from_message_usage(&m["usage"]),
        ) else {
            return;
        };
        if main && !self.messages.contains_key(id) {
            out.push(DriverEvent::Context {
                turn_id: Some(turn.to_string()),
                used_tokens: now.input + now.cache_read + now.cache_write,
                window_tokens: None,
                basis: model::ContextBasis::Reported,
            });
        }
        let was = self.messages.get(id).copied().unwrap_or_default();
        let top = was.max(now);
        let d = top.since(was);
        self.messages.insert(id.to_string(), top);
        if d.is_zero() {
            return;
        }
        self.in_turn = self.in_turn.plus(d);
        out.push(DriverEvent::Usage {
            turn_id: Some(turn.to_string()),
            input_tokens: d.input,
            output_tokens: d.output,
            cache_read_tokens: d.cache_read,
            cache_write_tokens: d.cache_write,
            cost: None,
            in_turn: true,
        });
    }

    fn result(&mut self, v: &Value, out: &mut Vec<DriverEvent>) {
        let turn = self.ensure_turn(out);
        self.usage(v, &turn, out);
        let status = self.status(v);
        let last_text = self.last_text.take();
        if status == TurnStatus::Failed
            && let Some((kind, text)) = limit_words(v, last_text)
        {
            let resets = model::limit_resets(&text);
            out.push(DriverEvent::LimitReached {
                turn_id: Some(turn.clone()),
                kind,
                resets,
                text,
            });
        }
        out.push(DriverEvent::TurnEnded {
            turn_id: turn,
            status,
        });
        self.current_turn = None;
        self.interrupt_requested = false;
    }

    /// Per-turn usage is the delta of the session-cumulative `modelUsage`, summed over models.
    /// `result.usage` covers the main thread only and leaves out subagents (claude-subagent@10782),
    /// and `total_cost_usd` is cumulative, so neither is read. The cost is likewise the delta of the summed
    /// per-model `costUSD`.
    fn usage(&mut self, v: &Value, turn: &str, out: &mut Vec<DriverEvent>) {
        let counted = std::mem::take(&mut self.in_turn);
        self.messages.clear();
        let Some(totals) = UsageTotals::from_model_usage(&v["modelUsage"]) else {
            return;
        };
        let turn_total = totals.since(self.usage_seen);
        // What the in-turn reports have not covered yet (a field they over-counted adds nothing).
        let d = UsageTotals {
            cost: turn_total.cost,
            ..turn_total.since(counted)
        };
        let known = !std::mem::replace(&mut self.cost_unknown, false);
        self.usage_seen = totals;
        if d.is_zero() && (d.cost.is_none() || !known) {
            return; // the in-turn reports already said it all
        }
        out.push(DriverEvent::Usage {
            turn_id: Some(turn.to_string()),
            input_tokens: d.input,
            output_tokens: d.output,
            cache_read_tokens: d.cache_read,
            cache_write_tokens: d.cache_write,
            cost: d.cost.filter(|_| known).map(|usd| model::Cost {
                usd,
                source: model::CostSource::ClaudeCode,
            }),
            in_turn: false,
        });
    }

    /// A real interrupt ends with `subtype:"error_during_execution"` and
    /// `terminal_reason:"aborted_tools"` (claude-interrupt@2434).
    fn status(&self, v: &Value) -> TurnStatus {
        let aborted = v["terminal_reason"] == "aborted_tools";
        if self.interrupt_requested || aborted {
            TurnStatus::Interrupted
        } else if v["subtype"] == "success" && !v["is_error"].as_bool().unwrap_or(false) {
            TurnStatus::Completed
        } else {
            TurnStatus::Failed
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn result_with_cost(cost: f64, input: u64) -> String {
        json!({ "type": "result", "subtype": "success", "is_error": false,
                "modelUsage": { "claude-x": { "inputTokens": input, "outputTokens": 1, "costUSD": cost } } }).to_string()
    }

    fn costs(p: &mut ClaudeParser, line: &str) -> Vec<Option<model::Cost>> {
        p.parse_line(line)
            .into_iter()
            .filter_map(|e| match e {
                DriverEvent::Usage { cost, .. } => Some(cost),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn a_turns_cost_is_claude_codes_figure_minus_the_last_one() {
        let mut p = ClaudeParser::default();
        let first = costs(&mut p, &result_with_cost(0.10, 100))[0].unwrap();
        assert!((first.usd - 0.10).abs() < 1e-9 && first.source == model::CostSource::ClaudeCode);
        assert!((costs(&mut p, &result_with_cost(0.25, 300))[0].unwrap().usd - 0.15).abs() < 1e-9);
    }

    #[test]
    fn the_first_turn_after_a_resume_has_no_cost_rather_than_the_whole_session() {
        let mut p = ClaudeParser::with_usage_baseline([100, 1, 0, 0]);
        assert_eq!(costs(&mut p, &result_with_cost(0.40, 200)), vec![None]);
        assert!((costs(&mut p, &result_with_cost(0.50, 300))[0].unwrap().usd - 0.10).abs() < 1e-9);
    }

    #[test]
    fn init_before_a_replay_starts_no_turn() {
        let mut p = ClaudeParser::default();
        p.on_sent("m1".into());
        let init = json!({ "type": "system", "subtype": "init", "session_id": "s" });
        assert!(p.parse_line(&init.to_string()).is_empty());
        let replay = json!({ "type": "user", "isReplay": true, "parent_tool_use_id": null,
                             "message": { "role": "user", "content": [{ "type": "text", "text": "hi" }] } });
        let ev = p.parse_line(&replay.to_string());
        assert_eq!(
            ev[0],
            DriverEvent::TurnStarted {
                turn_id: "turn-1".into()
            }
        );
        assert_eq!(
            ev[1],
            DriverEvent::MessageConsumed {
                message_id: "m1".into(),
                turn_id: "turn-1".into()
            }
        );
    }

    #[test]
    fn replay_with_a_parent_does_not_consume_a_pending_id() {
        let mut p = ClaudeParser::default();
        p.on_sent("m1".into());
        let child = json!({ "type": "user", "isReplay": true, "parent_tool_use_id": "toolu_x",
                            "message": { "role": "user", "content": [{ "type": "text", "text": "ls" }] } });
        assert!(p.parse_line(&child.to_string()).is_empty());
        let replay = json!({ "type": "user", "isReplay": true, "parent_tool_use_id": null,
                             "message": { "role": "user", "content": [{ "type": "text", "text": "hi" }] } });
        let ev = p.parse_line(&replay.to_string());
        assert!(ev.contains(&DriverEvent::MessageConsumed {
            message_id: "m1".into(),
            turn_id: "turn-1".into()
        }));
    }
}
