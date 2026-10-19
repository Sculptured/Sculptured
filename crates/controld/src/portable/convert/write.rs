//! Threads to one run: each agent starts, does its steps and stops; all agents' events are merged by time.
use super::{Ev, Preview, Thread};
use crate::portable::session::{Origin, SessionAgent, SessionHeader, SessionLine, Source, TokenTotals, SESSION_FORMAT};
use model::{Actor, Caps, EventBody, EventRecord, MessageIntent, Provenance, SeatLifecycle, Subject, TenureCause};
use std::collections::HashMap;

/// The seat id an imported agent gets in the run.
pub(super) fn seat_of(key: &str) -> String {
    let slug: String = key.chars().map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '-' }).collect();
    format!("imported.{slug}@imported")
}

/// One event before it gets its place in the run.
struct Item {
    at_ms: i64,
    actor: Actor,
    subject: Subject,
    provenance: Provenance,
    body: EventBody,
}

/// The run's text and its preview. `missing` lists subagents whose files were not uploaded.
pub(super) fn write(threads: &[Thread], source: Source, missing: Vec<String>, team: Option<crate::portable::team::PortableTeam>) -> (String, Preview) {
    let seats: HashMap<&str, String> = threads.iter().map(|t| (t.key.as_str(), seat_of(&t.key))).collect();
    let mut items: Vec<Item> = Vec::new();
    let mut n = 0;
    for t in threads {
        items.extend(thread_items(t, &seats, &mut n));
    }
    items.sort_by_key(|i| i.at_ms); // stable: each agent's own order is kept
    let events: Vec<EventRecord> = items.into_iter().enumerate().map(|(i, it)| record(i as i64 + 1, it)).collect();
    let header = header(threads, &seats, source, &events, team);
    let preview = preview(&header, threads, missing, &events);
    let mut out = serde_json::to_string(&header).expect("header serializes");
    out.push('\n');
    for event in events {
        out.push_str(&serde_json::to_string(&SessionLine { origin: Origin::Converted, event }).expect("line serializes"));
        out.push('\n');
    }
    (out, preview)
}

fn record(seq: i64, it: Item) -> EventRecord {
    EventRecord { seq, host: "imported".into(), host_seq: seq as u64, at_ms: it.at_ms, actor: it.actor, subject: it.subject, provenance: it.provenance, body: it.body }
}

fn system(at_ms: i64, subject: Subject, body: EventBody) -> Item {
    Item { at_ms, actor: Actor::System("import".into()), subject, provenance: Provenance::ProtocolObserved, body }
}

/// An agent's run: it starts at its first step, does each step, and stops at its last.
fn thread_items(t: &Thread, seats: &HashMap<&str, String>, n: &mut usize) -> Vec<Item> {
    let (Some(first), Some(last)) = (t.steps.first(), t.steps.last()) else { return vec![] };
    let seat = seats[t.key.as_str()].clone();
    let me = || Subject::Seat(seat.clone());
    let started = EventBody::TenureStarted { tenure: 1, cause: TenureCause::Fresh, driver: t.runtime.into(), native_session_id: t.session_id.clone(), caps: Caps::default() };
    let mut out = vec![system(first.at_ms, me(), started), system(first.at_ms, me(), EventBody::SeatLifecycleChanged { lifecycle: SeatLifecycle::Running, reason: None })];
    if let Some(parent) = t.parent.as_deref().and_then(|p| seats.get(p)) {
        out.push(system(first.at_ms, Subject::Rig("imported".into()), EventBody::TeamTopologyChanged { parent: parent.clone(), child: seat.clone() }));
    }
    for s in &t.steps {
        out.push(step_item(s.at_ms, &s.ev, &seat, seats, n));
    }
    let ended = EventBody::TenureEnded { tenure: 1, exit_code: None, signal: None, stderr_tail: String::new() };
    out.push(system(last.at_ms, me(), ended));
    out.push(system(last.at_ms, me(), EventBody::SeatLifecycleChanged { lifecycle: SeatLifecycle::Stopped, reason: Some("the imported session ended".into()) }));
    out
}

fn step_item(at_ms: i64, ev: &Ev, seat: &str, seats: &HashMap<&str, String>, n: &mut usize) -> Item {
    let subject = Subject::Seat(seat.to_string());
    match ev {
        Ev::Body(body, provenance) => Item { at_ms, actor: Actor::Seat(seat.to_string()), subject, provenance: *provenance, body: body.clone() },
        Ev::Prompt { text, from } => {
            *n += 1;
            let actor = match from.as_deref().and_then(|f| seats.get(f)) {
                Some(parent) => Actor::Seat(parent.clone()),
                None => Actor::Human("local".into()),
            };
            let body = EventBody::MessageQueued { message_id: format!("imported-{n}"), intent: MessageIntent::Instruct, text: text.clone() };
            Item { at_ms, actor, subject, provenance: Provenance::ProtocolObserved, body }
        }
    }
}

fn header(threads: &[Thread], seats: &HashMap<&str, String>, source: Source, events: &[EventRecord], team: Option<crate::portable::team::PortableTeam>) -> SessionHeader {
    let agents = threads
        .iter()
        .map(|t| SessionAgent { id: seats[t.key.as_str()].clone(), name: t.name.clone(), parent: t.parent.as_deref().and_then(|p| seats.get(p)).cloned(), runtime: Some(t.runtime.into()) })
        .collect();
    let mut tokens = TokenTotals::default();
    for e in events {
        if let EventBody::UsageReported { input_tokens, output_tokens, cache_read_tokens, cache_write_tokens, .. } = &e.body {
            (tokens.input, tokens.output) = (tokens.input + input_tokens, tokens.output + output_tokens);
            (tokens.cache_read, tokens.cache_write) = (tokens.cache_read + cache_read_tokens, tokens.cache_write + cache_write_tokens);
        }
    }
    let started_ms = events.first().map_or(0, |e| e.at_ms);
    let ended_ms = events.last().map(|e| e.at_ms);
    let team = team.as_ref().map(crate::portable::madl::to_madl);
    SessionHeader { format: SESSION_FORMAT.into(), source, team, team_id: "imported".into(), started_ms, ended_ms, agents, tokens }
}

fn preview(h: &SessionHeader, threads: &[Thread], missing: Vec<String>, events: &[EventRecord]) -> Preview {
    let unknown_lines = events.iter().filter(|e| matches!(e.body, EventBody::Unrecognized { .. })).count();
    let subagents = threads.iter().filter(|t| t.parent.is_some()).count();
    Preview { source: h.source, agents: threads.len(), subagents, started_ms: h.started_ms, ended_ms: h.ended_ms, tokens: h.tokens.clone(), missing, unknown_lines }
}
