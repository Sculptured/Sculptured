//! Helpers for the live driver tests: the drivers run the real `claude` and `codex` CLIs on their
//! cheapest models, so what is tested is a real model deployment, variance included. Each live test
//! is named `live_…` and marked `#[ignore = "live: …"]` (it spends credits and needs the CLI
//! installed and signed in); run them with
//!
//!     cargo test -p drivers --test claude_live -- --ignored live_
//!
//! A live test asserts on the driver's events (sessions, turns, tool calls, usage, questions, exits)
//! and on the processes it started, never on the model's exact words. When the CLI is missing,
//! `require` prints why and the test returns.
#![allow(dead_code)]
use drivers::{DriverEvent, OutboundMessage};
use futures::StreamExt;
use futures::stream::BoxStream;
use model::{MessageIntent, TurnStatus};
use std::path::PathBuf;
use std::time::Duration;

/// The model every live Claude test runs: the cheapest one.
pub const CLAUDE_MODEL: &str = "haiku";
/// The model every live Codex test runs: the cheapest one (`model::pricing`).
pub const CODEX_MODEL: &str = "gpt-6-luna";
/// How long one event may take: a real model turn takes seconds to a minute or two.
pub const WAIT: Duration = Duration::from_secs(240);

pub type Events = BoxStream<'static, DriverEvent>;

/// `name` on PATH, or None (with the reason printed) when this machine cannot run the test.
pub fn require(name: &str) -> Option<PathBuf> {
    let found = std::env::var_os("PATH").and_then(|paths| {
        std::env::split_paths(&paths)
            .map(|d| d.join(name))
            .find(|p| p.is_file())
    });
    if found.is_none() {
        eprintln!(
            "skipped: the {name} CLI is not on PATH; live tests need it installed and signed in"
        );
    }
    found
}

pub fn msg(id: &str, text: &str) -> OutboundMessage {
    OutboundMessage {
        message_id: id.into(),
        intent: MessageIntent::Inform,
        text: text.into(),
    }
}

pub async fn next(ev: &mut Events) -> DriverEvent {
    tokio::time::timeout(WAIT, ev.next())
        .await
        .expect("an event in time")
        .expect("the stream goes on")
}

/// Reads events until `pred` matches one; all the events read are returned (and printed).
pub async fn until(ev: &mut Events, pred: impl Fn(&DriverEvent) -> bool) -> Vec<DriverEvent> {
    let mut seen = Vec::new();
    loop {
        let e = next(ev).await;
        eprintln!("{e:?}");
        let done = pred(&e);
        seen.push(e);
        if done {
            return seen;
        }
    }
}

pub fn turn_ended(e: &DriverEvent) -> bool {
    matches!(e, DriverEvent::TurnEnded { .. })
}

pub fn exited(e: &DriverEvent) -> bool {
    matches!(e, DriverEvent::Exited { .. })
}

pub fn question(e: &DriverEvent) -> bool {
    matches!(e, DriverEvent::QuestionRaised { .. })
}

pub fn tool_call(e: &DriverEvent) -> bool {
    matches!(e, DriverEvent::ToolCall { .. })
}

/// The status of the last turn these events end.
pub fn ended(events: &[DriverEvent]) -> Option<TurnStatus> {
    events.iter().rev().find_map(|e| match e {
        DriverEvent::TurnEnded { status, .. } => Some(*status),
        _ => None,
    })
}

pub fn texts(events: &[DriverEvent]) -> Vec<&str> {
    events
        .iter()
        .filter_map(|e| match e {
            DriverEvent::Text { text, .. } => Some(text.as_str()),
            _ => None,
        })
        .collect()
}

/// (message id, turn id) of every message these events say was consumed.
pub fn consumed(events: &[DriverEvent]) -> Vec<(String, String)> {
    events
        .iter()
        .filter_map(|e| match e {
            DriverEvent::MessageConsumed {
                message_id,
                turn_id,
            } => Some((message_id.clone(), turn_id.clone())),
            _ => None,
        })
        .collect()
}

/// Input, output and cache-read tokens over every usage report.
pub fn tokens(events: &[DriverEvent]) -> u64 {
    events
        .iter()
        .map(|e| match e {
            DriverEvent::Usage {
                input_tokens,
                output_tokens,
                cache_read_tokens,
                ..
            } => input_tokens + output_tokens + cache_read_tokens,
            _ => 0,
        })
        .sum()
}

/// A word no other test or process uses, for commands the agent is asked to run.
pub fn marker(tag: &str) -> String {
    let n = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .subsec_nanos();
    format!("{tag}-{}-{n}", std::process::id())
}

/// A `sleep` long enough to outlast any test, its length unique so its process can be found.
pub fn long_sleep() -> String {
    let n = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .subsec_nanos();
    format!("sleep {}", 3000 + n % 997)
}

/// The command line of `pid`, one argument per item.
pub fn cmdline(pid: u32) -> Vec<String> {
    std::fs::read(format!("/proc/{pid}/cmdline"))
        .unwrap_or_default()
        .split(|b| *b == 0)
        .filter(|a| !a.is_empty())
        .map(|a| String::from_utf8_lossy(a).into_owned())
        .collect()
}

/// The environment `pid` was started with.
pub fn environ(pid: u32) -> Vec<(String, String)> {
    std::fs::read(format!("/proc/{pid}/environ"))
        .unwrap_or_default()
        .split(|b| *b == 0)
        .filter_map(|kv| {
            let kv = String::from_utf8_lossy(kv);
            kv.split_once('=')
                .map(|(k, v)| (k.to_string(), v.to_string()))
        })
        .collect()
}

/// Alive and not a zombie (Linux `/proc`).
pub fn alive(pid: u32) -> bool {
    std::fs::read_to_string(format!("/proc/{pid}/stat")).is_ok_and(|s| {
        s.rsplit_once(')')
            .and_then(|(_, rest)| rest.split_whitespace().next())
            .is_some_and(|st| st != "Z" && st != "X")
    })
}

fn parent(pid: u32) -> Option<u32> {
    let s = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    s.rsplit_once(')')?
        .1
        .split_whitespace()
        .nth(1)?
        .parse()
        .ok()
}

/// Live processes started (directly or not) by this test process whose command line holds `text`.
pub fn descendants_with(text: &str) -> Vec<u32> {
    let me = std::process::id();
    let descends = |mut pid: u32| {
        while let Some(p) = parent(pid) {
            if p == me {
                return true;
            }
            if p <= 1 {
                return false;
            }
            pid = p;
        }
        false
    };
    std::fs::read_dir("/proc")
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| e.file_name().to_str()?.parse::<u32>().ok())
        .filter(|&pid| alive(pid) && cmdline(pid).join(" ").contains(text) && descends(pid))
        .collect()
}

/// Waits up to `d` for a process of this test whose command line holds `text`.
pub async fn wait_descendants(text: &str, d: Duration) -> Vec<u32> {
    let t = std::time::Instant::now();
    loop {
        let found = descendants_with(text);
        if !found.is_empty() || t.elapsed() > d {
            return found;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

/// Whether every one of `pids` is gone within `d`.
pub async fn gone_within(pids: &[u32], d: Duration) -> bool {
    let t = std::time::Instant::now();
    while t.elapsed() < d {
        if !pids.iter().any(|&p| alive(p)) {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    !pids.iter().any(|&p| alive(p))
}

/// SIGKILL to `pid` alone, a process this test spawned (a crash, as the session sees it).
pub fn kill9(pid: u32) {
    // SAFETY: signalling a child this test started and has not reaped.
    unsafe {
        libc::kill(pid as i32, libc::SIGKILL);
    }
}
