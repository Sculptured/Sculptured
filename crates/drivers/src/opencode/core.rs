//! The queue rules: one prompt at a time, the next message sent when the running prompt answers, and an
//! interrupt that cancels the running prompt. Pure, so it is unit-tested without a process.
use super::parser::{AcpParser, Parsed};
use crate::{DriverEvent, OutboundMessage};
use serde_json::{Value, json};
use std::collections::VecDeque;

/// What a call produced: events to emit and lines to write.
#[derive(Debug, Default)]
pub(super) struct Out {
    pub(super) events: Vec<DriverEvent>,
    pub(super) writes: Vec<String>,
}

pub(super) struct Core {
    pub(super) parser: AcpParser,
    session: String,
    queue: VecDeque<OutboundMessage>,
    next_id: u64,
}

/// A message's turn id: one per message, so a resumed session's turns never repeat an earlier id.
pub(super) fn turn_id(msg: &OutboundMessage) -> String {
    format!("turn-{}", msg.message_id)
}

impl Core {
    pub(super) fn new(parser: AcpParser, session: String, next_id: u64) -> Self {
        Self {
            parser,
            session,
            queue: VecDeque::new(),
            next_id,
        }
    }

    /// A new message: its prompt now, or nothing while another prompt runs.
    pub(super) fn send(&mut self, msg: OutboundMessage) -> Out {
        self.queue.push_back(msg);
        self.next_prompt()
    }

    fn next_prompt(&mut self) -> Out {
        if self.parser.busy() {
            return Out::default();
        }
        let Some(msg) = self.queue.pop_front() else {
            return Out::default();
        };
        self.next_id += 1;
        let turn = turn_id(&msg);
        self.parser.on_prompt_sent(self.next_id, turn.clone());
        let line = json!({ "jsonrpc": "2.0", "id": self.next_id, "method": "session/prompt",
            "params": { "sessionId": self.session, "prompt": [{ "type": "text", "text": msg.text }] } });
        Out {
            events: vec![
                DriverEvent::TurnStarted {
                    turn_id: turn.clone(),
                },
                DriverEvent::MessageConsumed {
                    message_id: msg.message_id,
                    turn_id: turn,
                },
            ],
            writes: vec![line.to_string()],
        }
    }

    /// One stdout line; when it ends the running prompt, the next message's prompt goes.
    pub(super) fn on_line(&mut self, line: &str) -> Out {
        let Parsed {
            events,
            replies,
            turn_ended,
        } = self.parser.parse_line(line);
        let mut out = Out {
            events,
            writes: replies.iter().map(Value::to_string).collect(),
        };
        if turn_ended {
            let next = self.next_prompt();
            out.events.extend(next.events);
            out.writes.extend(next.writes);
        }
        out
    }

    /// `session/cancel` for the running prompt, with its open permission requests answered "cancelled".
    /// The prompt then answers `stopReason: cancelled` (opencode-interrupt). Nothing when no prompt runs.
    pub(super) fn interrupt(&mut self) -> Out {
        if !self.parser.busy() {
            return Out::default();
        }
        let mut parsed = Parsed::default();
        let replies = self.parser.cancel_asked(&mut parsed);
        let cancel = json!({ "jsonrpc": "2.0", "method": "session/cancel", "params": { "sessionId": self.session } });
        let mut writes: Vec<String> = replies.iter().map(Value::to_string).collect();
        writes.push(cancel.to_string());
        Out {
            events: parsed.events,
            writes,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use model::{MessageIntent, TurnStatus};

    fn msg(id: &str) -> OutboundMessage {
        OutboundMessage {
            message_id: id.into(),
            intent: MessageIntent::Inform,
            text: format!("text of {id}"),
        }
    }

    fn core() -> Core {
        Core::new(AcpParser::new(true), "ses_1".into(), 3)
    }

    fn line(s: &str) -> Value {
        serde_json::from_str(s).unwrap()
    }

    #[test]
    fn a_message_waits_for_the_running_prompt_then_goes() {
        let mut c = core();
        let first = c.send(msg("m1"));
        assert_eq!(
            line(&first.writes[0])["params"]["prompt"][0]["text"],
            "text of m1"
        );
        assert_eq!(
            first.events,
            [
                DriverEvent::TurnStarted {
                    turn_id: "turn-m1".into()
                },
                DriverEvent::MessageConsumed {
                    message_id: "m1".into(),
                    turn_id: "turn-m1".into()
                },
            ]
        );
        assert!(c.send(msg("m2")).writes.is_empty(), "m1's prompt runs");
        let done = c.on_line(r#"{"jsonrpc":"2.0","id":4,"result":{"stopReason":"end_turn"}}"#);
        assert!(done.events.contains(&DriverEvent::TurnEnded {
            turn_id: "turn-m1".into(),
            status: TurnStatus::Completed
        }));
        assert_eq!(done.writes.len(), 1);
        assert_eq!(
            line(&done.writes[0])["params"]["prompt"][0]["text"],
            "text of m2"
        );
        assert_eq!(line(&done.writes[0])["id"], 5);
    }

    #[test]
    fn an_interrupt_cancels_the_running_prompt_and_nothing_else() {
        let mut c = core();
        assert!(c.interrupt().writes.is_empty(), "nothing runs");
        c.send(msg("m1"));
        let out = c.interrupt();
        assert_eq!(line(out.writes.last().unwrap())["method"], "session/cancel");
        assert_eq!(
            line(out.writes.last().unwrap())["params"]["sessionId"],
            "ses_1"
        );
    }

    #[test]
    fn an_interrupt_answers_an_open_permission_request_cancelled() {
        let mut c = core();
        c.send(msg("m1"));
        let asked = c.on_line(r#"{"jsonrpc":"2.0","id":0,"method":"session/request_permission","params":{"sessionId":"ses_1","toolCall":{"toolCallId":"call_1","title":"echo hi","kind":"execute","rawInput":{"command":"echo hi"}},"options":[{"optionId":"once","kind":"allow_once","name":"Allow once"}]}}"#);
        assert!(matches!(
            asked.events[0],
            DriverEvent::QuestionRaised { .. }
        ));
        let out = c.interrupt();
        assert_eq!(
            line(&out.writes[0]),
            json!({ "jsonrpc": "2.0", "id": 0, "result": { "outcome": { "outcome": "cancelled" } } })
        );
        assert_eq!(
            out.events,
            [DriverEvent::QuestionResolved {
                question_id: "perm-0".into()
            }]
        );
    }
}
