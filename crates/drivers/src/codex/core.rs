//! The queue rules: one message per turn, refused starts sent again after a pause, and an
//! interrupt that waits for the turn id. Pure (time is passed in), so it is unit-tested without a process.
use super::parser::CodexParser;
use super::{MAX_START_ATTEMPTS, RESEND_PAUSE};
use crate::{DriverEvent, OutboundMessage};
use serde_json::{Value, json};
use std::collections::{HashMap, VecDeque};
use std::time::Instant;

/// A message waiting for, or sent in, a `turn/start`, with the number of times it was sent.
struct Queued {
    msg: OutboundMessage,
    attempts: u32,
}

/// What the session and its reader share: the parser, the messages waiting for a turn and the
/// ones whose `turn/start` is in flight. Pure (time is passed in), so the queue rules are
/// unit-tested without a process.
pub(super) struct Core {
    pub(super) parser: CodexParser,
    queue: VecDeque<Queued>,
    /// Sent and not yet consumed or refused, by message id.
    in_flight: HashMap<String, Queued>,
    next_id: u64,
    /// No `turn/start` before this: the pause after a refused start.
    hold_until: Option<Instant>,
    /// `interrupt()` came while a `turn/start` waited for its answer: sent once the turn id arrives.
    interrupt_pending: bool,
}

/// Interrupts that stop nothing say so in the transcript, kept as evidence like the driver's other notes.
const NOT_APPLIED: &str = "interrupt not applied";
/// Why: the interrupt came while a message Codex refused waited out its pause.
pub(super) const PAUSED: &str =
    "no turn was running; the message Codex refused will be sent again after a short pause";
/// Why: the interrupt waited for a turn that Codex then refused to start.
pub(super) const REFUSED: &str = "Codex refused to start the turn, so nothing was running";
/// Why, as `REFUSED`, when that message goes again after a pause.
pub(super) const REFUSED_RESENT: &str = "Codex refused to start the turn, so nothing was running; the message will be sent again after a short pause";

/// The transcript note for an interrupt that did not stop anything, and why.
pub(super) fn not_applied(why: &str) -> DriverEvent {
    DriverEvent::Unrecognized {
        raw: json!({ "note": format!("{NOT_APPLIED}: {why}") }),
    }
}

/// What became of an interrupt remembered while its turn was starting.
enum Held {
    Nothing,
    Sent(String),
    Dropped,
}

/// What one stdout line produced: events to emit, lines to write, and when to try the queue again.
#[derive(Debug, Default)]
pub(super) struct Out {
    pub(super) events: Vec<DriverEvent>,
    pub(super) writes: Vec<String>,
    /// A refused message waits for its pause: call `next_turn` at this time.
    pub(super) wake: Option<Instant>,
}

impl Core {
    pub(super) fn new(parser: CodexParser, next_id: u64) -> Self {
        Self {
            parser,
            queue: VecDeque::new(),
            in_flight: HashMap::new(),
            next_id,
            hold_until: None,
            interrupt_pending: false,
        }
    }

    fn interrupt_request(&mut self, turn: String) -> String {
        let thread = self.parser.thread_id().unwrap_or_default().to_string();
        self.request(
            "turn/interrupt",
            json!({ "threadId": thread, "turnId": turn }),
        )
    }

    /// `turn/interrupt` for the running turn. While a `turn/start` waits for its answer there is no
    /// turn id yet: the interrupt is remembered and sent when the id arrives (`on_line`).
    pub(super) fn interrupt(&mut self) -> Option<String> {
        if let Some(turn) = self.parser.current_turn().map(str::to_string) {
            return Some(self.interrupt_request(turn));
        }
        if self.parser.busy() {
            self.interrupt_pending = true;
        }
        None
    }

    /// The note for an interrupt that comes during a re-send pause: no turn runs, and the
    /// refused message is sent again when the pause ends.
    pub(super) fn paused_note(&self) -> Option<DriverEvent> {
        let paused = !self.parser.busy() && self.hold_until.is_some() && !self.queue.is_empty();
        paused.then(|| not_applied(PAUSED))
    }

    fn request(&mut self, method: &str, params: Value) -> String {
        self.next_id += 1;
        json!({ "jsonrpc": "2.0", "id": self.next_id, "method": method, "params": params })
            .to_string()
    }

    fn turn_start(&mut self, mut q: Queued) -> String {
        let thread = self.parser.thread_id().unwrap_or_default().to_string();
        let line = self.request(
            "turn/start",
            json!({ "threadId": thread, "input": [{ "type": "text", "text": q.msg.text }] }),
        );
        self.parser
            .on_turn_start_sent(self.next_id, q.msg.message_id.clone());
        q.attempts += 1;
        self.in_flight.insert(q.msg.message_id.clone(), q);
        line
    }

    /// The next queued message's `turn/start`, when no turn runs or is being started and no
    /// re-send pause is running.
    pub(super) fn next_turn(&mut self, now: Instant) -> Option<String> {
        if self.parser.busy() || self.hold_until.is_some_and(|h| now < h) {
            return None;
        }
        self.hold_until = None;
        let q = self.queue.pop_front()?;
        Some(self.turn_start(q))
    }

    /// A new message: its `turn/start` now, or None when it must wait (a turn runs, is being
    /// started, earlier messages wait, or a refused one is pausing).
    pub(super) fn send(&mut self, msg: OutboundMessage, now: Instant) -> Option<String> {
        self.queue.push_back(Queued { msg, attempts: 0 });
        if self.queue.len() > 1 {
            None
        } else {
            self.next_turn(now)
        }
    }

    /// One stdout line: the events to emit, the lines to write and when to try the queue again.
    pub(super) fn on_line(&mut self, line: &str, now: Instant) -> Out {
        let parsed = self.parser.parse_line(line);
        let writes = parsed.replies.iter().map(Value::to_string).collect();
        let mut out = Out {
            events: parsed.events,
            writes,
            wake: None,
        };
        for e in &out.events {
            if let DriverEvent::MessageConsumed { message_id, .. } = e {
                self.in_flight.remove(message_id);
            }
        }
        let held = self.take_pending_interrupt();
        if let Held::Sent(line) = &held {
            out.writes.push(line.clone());
        }
        if parsed.turn_ended {
            self.hold_until = None;
        }
        if let Some(id) = &parsed.refused_start {
            self.on_refused_start(id, now, &mut out);
        }
        if let Held::Dropped = held {
            out.events.push(not_applied(if out.wake.is_some() {
                REFUSED_RESENT
            } else {
                REFUSED
            }));
        }
        if parsed.turn_ended || parsed.refused_start.is_some() {
            out.writes.extend(self.next_turn(now));
        }
        out
    }

    /// The remembered interrupt's `turn/interrupt`, once the turn id is known. When the start was
    /// refused no turn runs, so there is nothing to interrupt and it is dropped.
    fn take_pending_interrupt(&mut self) -> Held {
        if !self.interrupt_pending {
            return Held::Nothing;
        }
        if let Some(turn) = self.parser.current_turn().map(str::to_string) {
            self.interrupt_pending = false;
            return Held::Sent(self.interrupt_request(turn));
        }
        if self.parser.busy() {
            return Held::Nothing;
        }
        self.interrupt_pending = false;
        Held::Dropped
    }

    /// A refused message goes back to the front of the queue after a pause, or fails with the
    /// server's words once it has been sent `MAX_START_ATTEMPTS` times.
    fn on_refused_start(&mut self, id: &str, now: Instant, out: &mut Out) {
        let Some(q) = self.in_flight.remove(id) else {
            return;
        };
        if q.attempts >= MAX_START_ATTEMPTS {
            let reason = format!(
                "Codex did not start a turn for this message ({} tries): {}",
                q.attempts,
                refusal_words(&out.events)
            );
            out.events.push(DriverEvent::MessageFailed {
                message_id: id.to_string(),
                reason,
            });
        } else {
            let until = now + RESEND_PAUSE * q.attempts;
            self.hold_until = Some(until);
            out.wake = Some(until);
            self.queue.push_front(q);
        }
    }
}

/// Why the server refused the start, from the last refusal among `events`.
fn refusal_words(events: &[DriverEvent]) -> String {
    events
        .iter()
        .rev()
        .find_map(|e| match e {
            DriverEvent::Unrecognized { raw } if raw["error"]["message"].is_string() => {
                raw["error"]["message"].as_str().map(str::to_string)
            }
            DriverEvent::Unrecognized { raw }
                if raw.get("turnStartAnsweredWithEndedTurn").is_some() =>
            {
                Some("it answered with a turn that had already ended".to_string())
            }
            _ => None,
        })
        .unwrap_or_else(|| "unknown".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use model::MessageIntent;

    fn msg(id: &str) -> OutboundMessage {
        OutboundMessage {
            message_id: id.into(),
            intent: MessageIntent::Inform,
            text: format!("text of {id}"),
        }
    }

    fn core() -> Core {
        let mut p = CodexParser::default();
        p.set_thread("thr-1");
        Core::new(p, 2)
    }

    fn id_of(line: &str) -> u64 {
        serde_json::from_str::<Value>(line).unwrap()["id"]
            .as_u64()
            .unwrap()
    }

    fn text_of(line: &str) -> String {
        serde_json::from_str::<Value>(line).unwrap()["params"]["input"][0]["text"]
            .as_str()
            .unwrap()
            .to_string()
    }

    fn method_of(line: &str) -> String {
        serde_json::from_str::<Value>(line).unwrap()["method"]
            .as_str()
            .unwrap_or_default()
            .to_string()
    }

    fn refuse(c: &mut Core, line: &str, now: Instant) -> Out {
        let err = json!({ "id": id_of(line), "error": { "code": -32600, "message": "a turn is already in progress" } });
        c.on_line(&err.to_string(), now)
    }

    fn started(c: &mut Core, line: &str, turn: &str) -> Out {
        c.on_line(&json!({ "id": id_of(line), "result": { "turn": { "id": turn, "status": "inProgress" } } }).to_string(), Instant::now())
    }

    fn failed(out: &Out) -> bool {
        out.events
            .iter()
            .any(|e| matches!(e, DriverEvent::MessageFailed { .. }))
    }

    /// Refuses m1's start, and each re-send once its pause is over, until the message fails.
    /// Returns that last output and the number of sends.
    fn refuse_until_failed(c: &mut Core, mut line: String, mut now: Instant) -> (Out, u32) {
        let mut sends = 1;
        loop {
            let out = refuse(c, &line, now);
            if failed(&out) {
                return (out, sends);
            }
            assert!(
                out.writes.is_empty(),
                "not re-sent at once: {:?}",
                out.writes
            );
            let wake = out.wake.expect("a re-send is scheduled");
            assert_eq!(
                wake,
                now + RESEND_PAUSE * sends,
                "a pause of RESEND_PAUSE times the tries so far"
            );
            assert_eq!(c.next_turn(now), None, "nothing is sent during the pause");
            now = wake;
            line = c.next_turn(now).expect("re-sent once the pause is over");
            assert_eq!(
                text_of(&line),
                "text of m1",
                "the refused message goes first, before m2"
            );
            sends += 1;
        }
    }

    /// A refused start is sent again after a pause (not at once), three times in all; then the
    /// message fails with the server's words and the next one starts without a pause.
    #[test]
    fn a_refused_start_is_resent_after_a_pause_three_times_in_all_then_fails() {
        assert_eq!(MAX_START_ATTEMPTS, 3);
        let now = Instant::now();
        let mut c = core();
        let line = c.send(msg("m1"), now).expect("starts at once");
        assert!(c.send(msg("m2"), now).is_none(), "waits behind m1");
        let (out, sends) = refuse_until_failed(&mut c, line, now);
        assert_eq!(sends, MAX_START_ATTEMPTS, "three sends in all");
        assert!(
            out.events.contains(&DriverEvent::MessageFailed {
                message_id: "m1".into(),
                reason: "Codex did not start a turn for this message (3 tries): a turn is already in progress".into(),
            }),
            "{:?}",
            out.events
        );
        assert_eq!(out.writes.len(), 1);
        assert_eq!(
            text_of(&out.writes[0]),
            "text of m2",
            "the next message is not held up"
        );
        let out = started(&mut c, &out.writes[0], "t1");
        assert!(
            out.events.contains(&DriverEvent::MessageConsumed {
                message_id: "m2".into(),
                turn_id: "t1".into()
            }),
            "{:?}",
            out.events
        );
        assert!(c.in_flight.is_empty());
    }

    /// A `turn/completed` during the pause ends it: the refused message is re-sent at once.
    #[test]
    fn a_turn_completed_ends_the_pause_before_a_resend() {
        let now = Instant::now();
        let mut c = core();
        let line = c.send(msg("m1"), now).unwrap();
        assert!(refuse(&mut c, &line, now).writes.is_empty());
        let out = c.on_line(r#"{"method":"turn/completed","params":{"threadId":"thr-1","turn":{"id":"t0","status":"completed"}}}"#, now);
        assert_eq!(out.writes.len(), 1, "{:?}", out.writes);
        assert_eq!(text_of(&out.writes[0]), "text of m1");
    }

    /// Nothing is sent while a turn runs; the held message starts on `turn/completed`.
    #[test]
    fn a_message_waits_for_the_running_turn_to_complete() {
        let now = Instant::now();
        let mut c = core();
        let line = c.send(msg("m1"), now).unwrap();
        started(&mut c, &line, "t1");
        assert!(c.send(msg("m2"), now).is_none(), "t1 runs");
        let out = c.on_line(r#"{"method":"item/completed","params":{"threadId":"thr-1","turnId":"t1","item":{"type":"agentMessage","id":"a","text":"hi"}}}"#, now);
        assert!(out.writes.is_empty(), "{:?}", out.writes);
        let out = c.on_line(r#"{"method":"turn/completed","params":{"threadId":"thr-1","turn":{"id":"t1","status":"completed"}}}"#, now);
        assert_eq!(out.writes.len(), 1);
        assert_eq!(text_of(&out.writes[0]), "text of m2");
    }

    /// Between sending `turn/start` and learning the turn's id there is nothing to name in a
    /// `turn/interrupt`: the interrupt is remembered and sent once, as soon as the id arrives.
    #[test]
    fn an_interrupt_while_the_turn_is_starting_is_sent_when_its_id_arrives() {
        let mut c = core();
        let line = c.send(msg("m1"), Instant::now()).unwrap();
        assert_eq!(c.interrupt(), None, "no turn id yet");
        let out = started(&mut c, &line, "t1");
        let interrupts: Vec<Value> = out
            .writes
            .iter()
            .filter(|w| method_of(w) == "turn/interrupt")
            .map(|w| serde_json::from_str(w).unwrap())
            .collect();
        assert_eq!(interrupts.len(), 1, "{:?}", out.writes);
        assert_eq!(
            interrupts[0]["params"],
            json!({ "threadId": "thr-1", "turnId": "t1" })
        );
        let later = c.on_line(
            r#"{"method":"turn/started","params":{"threadId":"thr-1","turn":{"id":"t1"}}}"#,
            Instant::now(),
        );
        assert!(later.writes.is_empty(), "sent once: {:?}", later.writes);
        assert!(
            c.interrupt().is_some(),
            "a running turn is interrupted at once"
        );
    }

    /// An interrupt for a start that is then refused interrupts nothing: no turn runs, and the
    /// message's later turn is not interrupted by it.
    #[test]
    fn an_interrupt_for_a_refused_start_is_dropped() {
        let now = Instant::now();
        let mut c = core();
        let line = c.send(msg("m1"), now).unwrap();
        assert_eq!(c.interrupt(), None);
        let out = refuse(&mut c, &line, now);
        assert!(!out.writes.iter().any(|w| method_of(w) == "turn/interrupt"));
        let line = c.next_turn(out.wake.unwrap()).unwrap();
        let out = started(&mut c, &line, "t1");
        assert!(
            !out.writes.iter().any(|w| method_of(w) == "turn/interrupt"),
            "{:?}",
            out.writes
        );
    }

    fn notes(out: &Out) -> Vec<&DriverEvent> {
        out.events
            .iter()
            .filter(|e| matches!(e, DriverEvent::Unrecognized { raw } if raw.get("note").is_some()))
            .collect()
    }

    /// Task 13 re-review #8: an interrupt during a re-send pause stops nothing, and says so.
    #[test]
    fn an_interrupt_during_a_resend_pause_is_noted_as_not_applied() {
        let now = Instant::now();
        let mut c = core();
        let line = c.send(msg("m1"), now).unwrap();
        assert_eq!(c.paused_note(), None, "a start in flight is no pause");
        let out = refuse(&mut c, &line, now);
        assert!(out.wake.is_some());
        assert_eq!(c.interrupt(), None, "nothing runs to interrupt");
        let note = c.paused_note().expect("a note during the pause");
        assert_eq!(
            note,
            DriverEvent::Unrecognized {
                raw: json!({ "note": format!("interrupt not applied: {PAUSED}") })
            }
        );
        let line = c.next_turn(out.wake.unwrap()).unwrap();
        started(&mut c, &line, "t1");
        assert_eq!(c.paused_note(), None, "the pause is over");
    }

    /// Task 13 re-review #8: an interrupt held for a start that is then refused is dropped, with a note.
    #[test]
    fn a_dropped_interrupt_is_noted_as_not_applied() {
        let now = Instant::now();
        let mut c = core();
        let line = c.send(msg("m1"), now).unwrap();
        assert_eq!(c.interrupt(), None);
        let out = refuse(&mut c, &line, now);
        assert_eq!(notes(&out), [&not_applied(REFUSED_RESENT)]);
        let text = format!("{:?}", notes(&out)[0]);
        assert!(text.contains("interrupt not applied: "), "{text}");
        let line = c.next_turn(out.wake.unwrap()).unwrap();
        let out = started(&mut c, &line, "t1");
        assert!(notes(&out).is_empty(), "noted once");
    }

    /// An interrupt that reaches its turn is sent, not noted.
    #[test]
    fn an_applied_interrupt_has_no_note() {
        let mut c = core();
        let line = c.send(msg("m1"), Instant::now()).unwrap();
        assert_eq!(c.interrupt(), None);
        let out = started(&mut c, &line, "t1");
        assert!(notes(&out).is_empty());
        assert_eq!(c.paused_note(), None);
    }
}
