//! Who started whom in a Claude Code session: each subagent thread is matched to its parent's `Agent`
//! call (by the `.meta.json`'s `toolUseId`, else by its first prompt); with neither, it is shown under the
//! main agent rather than guessed.
//! A call whose subagent file was not uploaded still becomes an agent, listed as missing.
use super::claude::{Call, Reader};
use super::{Ev, Read, Thread};
use serde_json::Value;
use std::collections::{HashMap, HashSet};

pub(super) fn link(readers: Vec<Reader>, metas: &HashMap<String, Value>) -> Read {
    let calls: Vec<(String, Call)> = readers
        .iter()
        .flat_map(|r| r.calls.iter().map(|c| (r.t.key.clone(), c.clone())))
        .collect();
    let mut used = HashSet::new();
    let mut threads = Vec::new();
    for r in readers {
        let meta = r
            .stem
            .as_ref()
            .and_then(|s| metas.get(s))
            .cloned()
            .unwrap_or(Value::Null);
        let mut t = r.finish();
        if t.key != "main" {
            attach(&mut t, &meta, &calls, &mut used);
        }
        threads.push(t);
    }
    let missing = add_missing(&mut threads, &calls, &used);
    Read {
        threads,
        warnings: Vec::new(),
        missing,
    }
}

fn first_prompt(t: &Thread) -> Option<&str> {
    t.steps.iter().find_map(|s| match &s.ev {
        Ev::Prompt { text, .. } => Some(text.as_str()),
        Ev::Body(..) => None,
    })
}

/// The parent's call for this subagent, and (when found) its task, name and type.
fn attach(t: &mut Thread, meta: &Value, calls: &[(String, Call)], used: &mut HashSet<String>) {
    let by_id = meta["toolUseId"]
        .as_str()
        .and_then(|id| calls.iter().find(|(_, c)| c.id == id));
    let prompt = first_prompt(t).map(|p| p.trim().to_string());
    let same =
        |c: &Call| c.prompt.as_deref().map(str::trim) == prompt.as_deref() && prompt.is_some();
    let by_prompt = || {
        calls
            .iter()
            .find(|(owner, c)| *owner != t.key && !used.contains(&c.id) && same(c))
    };
    let found = by_id.or_else(by_prompt).cloned();
    let text = |k: &str| meta[k].as_str().map(str::to_string);
    let Some((owner, call)) = found else {
        (t.parent, t.name, t.agent_type) =
            (Some("main".into()), text("description"), text("agentType"));
        return;
    };
    used.insert(call.id.clone());
    t.parent = Some(owner.clone());
    t.task = call.prompt.clone().or(prompt);
    t.name = text("description").or(call.description);
    t.agent_type = text("agentType").or(call.agent_type);
    if let Some(Ev::Prompt { from, .. }) = t
        .steps
        .iter_mut()
        .map(|s| &mut s.ev)
        .find(|e| matches!(e, Ev::Prompt { .. }))
    {
        *from = Some(owner);
    }
}

/// Calls whose subagent file was not uploaded: an agent with no transcript, and a line in `missing`.
fn add_missing(
    threads: &mut Vec<Thread>,
    calls: &[(String, Call)],
    used: &HashSet<String>,
) -> Vec<String> {
    let mut missing = Vec::new();
    let matching_calls = calls.iter().filter(|(_, c)| !used.contains(&c.id));
    for (owner, c) in matching_calls {
        let name = c.description.clone().unwrap_or_else(|| "A subagent".into());
        missing.push(format!(
            "{name}: its file (subagents/agent-*.jsonl next to the session) was not included"
        ));
        threads.push(Thread {
            key: c.id.clone(),
            name: Some(name),
            parent: Some(owner.clone()),
            runtime: "claude-stream",
            task: c.prompt.clone(),
            agent_type: c.agent_type.clone(),
            ..Thread::default()
        });
    }
    missing
}
