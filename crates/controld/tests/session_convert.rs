use controld::portable::convert::{convert, Upload};
use controld::portable::session::{parse_session, Origin, Source};
use model::EventBody;

fn fixture(path: &str) -> Upload {
    let full = format!("{}/tests/fixtures/sessions/{path}", env!("CARGO_MANIFEST_DIR"));
    Upload { name: path.rsplit('/').next().unwrap().into(), text: std::fs::read_to_string(full).unwrap() }
}

fn bodies(session: &str) -> Vec<EventBody> {
    parse_session(session).unwrap().1.into_iter().map(|(_, l)| l.event.body).collect()
}

/// 2026-10-01T10:00:00Z in Unix milliseconds.
const T0: i64 = 1_790_848_800_000;

#[test]
fn a_claude_session_converts_to_a_run_with_one_agent() {
    let c = convert(&[fixture("claude/main.jsonl")]).unwrap();
    let (header, lines) = parse_session(&c.session).unwrap();
    assert_eq!(header.source, Source::ClaudeCode);
    assert_eq!(header.agents.len(), 1);
    assert_eq!(header.agents[0].name.as_deref(), Some("Login page"));
    assert_eq!(header.started_ms, T0);
    assert!(lines.iter().all(|(_, l)| l.origin == Origin::Converted));
    assert_eq!(c.preview.agents, 1);
    assert_eq!(c.preview.unknown_lines, 1);
}

#[test]
fn usage_counts_once_per_message_even_when_split_over_lines() {
    let c = convert(&[fixture("claude/main.jsonl")]).unwrap();
    let t = &parse_session(&c.session).unwrap().0.tokens;
    assert_eq!((t.input, t.output, t.cache_read), (110, 23, 5));
}

#[test]
fn only_real_prompts_become_messages() {
    let c = convert(&[fixture("claude/main.jsonl")]).unwrap();
    let prompts: Vec<String> = bodies(&c.session).into_iter().filter_map(|b| match b { EventBody::MessageQueued { text, .. } => Some(text), _ => None }).collect();
    assert_eq!(prompts, vec!["Add a login page".to_string()]);
}

#[test]
fn tool_calls_results_text_and_turns_are_kept_in_order() {
    let c = convert(&[fixture("claude/main.jsonl")]).unwrap();
    let kinds: Vec<&str> = bodies(&c.session)
        .iter()
        .map(|b| match b {
            EventBody::TenureStarted { .. } => "tenure",
            EventBody::MessageQueued { .. } => "prompt",
            EventBody::TurnStarted { .. } => "turn",
            EventBody::AgentText { .. } => "text",
            EventBody::ToolCalled { .. } => "call",
            EventBody::ToolResulted { .. } => "result",
            EventBody::TurnEnded { .. } => "end",
            EventBody::TenureEnded { .. } => "tenure-end",
            _ => "",
        })
        .filter(|k| !k.is_empty())
        .collect();
    assert_eq!(kinds, ["tenure", "prompt", "turn", "text", "call", "result", "text", "end", "tenure-end"]);
}

#[test]
fn a_line_without_a_timestamp_takes_the_previous_time() {
    let c = convert(&[fixture("claude/main.jsonl")]).unwrap();
    let (_, lines) = parse_session(&c.session).unwrap();
    let done = lines.iter().find(|(_, l)| matches!(&l.event.body, EventBody::AgentText { text, .. } if text == "Done.")).unwrap();
    assert_eq!(done.1.event.at_ms, T0 + 7_000);
}

#[test]
fn a_file_that_is_neither_format_is_refused() {
    let e = convert(&[Upload { name: "x.jsonl".into(), text: "{\"hello\":1}\n".into() }]).unwrap_err();
    assert!(e.message.contains("isn't a Claude Code or Codex session file"));
}

fn claude_with_agents(include_a2: bool) -> Vec<Upload> {
    let mut v = vec![fixture("claude/main-with-agents.jsonl"), fixture("claude/subagents/agent-a1.jsonl"), fixture("claude/subagents/agent-a1.meta.json")];
    if include_a2 {
        v.extend([fixture("claude/subagents/agent-a2.jsonl"), fixture("claude/subagents/agent-a2.meta.json")]);
    }
    v
}

#[test]
fn subagents_become_child_agents_with_their_parent_and_task() {
    let c = convert(&claude_with_agents(true)).unwrap();
    let h = parse_session(&c.session).unwrap().0;
    let main = h.agents.iter().find(|a| a.parent.is_none()).unwrap();
    let kids: Vec<_> = h.agents.iter().filter(|a| a.parent.as_deref() == Some(main.id.as_str())).collect();
    assert_eq!(kids.len(), 3, "two with files, one missing");
    assert_eq!(c.preview.subagents, 3);
    assert!(c.preview.missing.len() == 1 && c.preview.missing[0].starts_with("Explore db"));
}

#[test]
fn a_subagent_without_its_file_is_listed_as_missing_and_still_shown() {
    let c = convert(&claude_with_agents(false)).unwrap();
    assert_eq!(c.preview.missing.len(), 2);
    assert!(c.preview.missing[0].contains("subagents"));
    assert_eq!(parse_session(&c.session).unwrap().0.agents.len(), 4);
}

#[test]
fn the_parent_hands_the_subagent_its_task_as_a_message() {
    let c = convert(&claude_with_agents(true)).unwrap();
    let (h, lines) = parse_session(&c.session).unwrap();
    let main = h.agents.iter().find(|a| a.parent.is_none()).unwrap();
    let a1 = h.agents.iter().find(|a| a.name.as_deref() == Some("Explore auth")).unwrap();
    let task = lines.iter().find(|(_, l)| l.event.subject.key() == format!("seat:{}", a1.id) && matches!(l.event.body, EventBody::MessageQueued { .. })).unwrap();
    assert_eq!(task.1.event.actor, model::Actor::Seat(main.id.clone()));
    assert!(matches!(&task.1.event.body, EventBody::MessageQueued { text, .. } if text == "Find how auth works"));
}

#[test]
fn subagent_tokens_count_toward_the_run() {
    let c = convert(&claude_with_agents(true)).unwrap();
    let t = parse_session(&c.session).unwrap().0.tokens;
    assert_eq!((t.input, t.output), (50 + 20 + 30 + 40, 10 + 5 + 6 + 8));
}

#[test]
fn inline_sidechains_from_older_sessions_become_subagents() {
    let c = convert(&[fixture("claude/inline.jsonl")]).unwrap();
    let h = parse_session(&c.session).unwrap().0;
    assert_eq!(h.agents.len(), 2);
    let kid = h.agents.iter().find(|a| a.parent.is_some()).unwrap();
    assert_eq!(kid.name.as_deref(), Some("Search logs"));
}

#[test]
fn two_different_claude_sessions_are_refused() {
    let other = fixture("claude/main.jsonl");
    let e = convert(&[fixture("claude/main-with-agents.jsonl"), other]).unwrap_err();
    assert!(e.message.contains("2 different sessions"));
}

#[test]
fn a_codex_rollout_converts_with_prompts_tools_and_turns() {
    let c = convert(&[fixture("codex/rollout.jsonl")]).unwrap();
    let (h, _) = parse_session(&c.session).unwrap();
    assert_eq!(h.source, Source::Codex);
    let b = bodies(&c.session);
    let prompts: Vec<_> = b.iter().filter_map(|x| match x { EventBody::MessageQueued { text, .. } => Some(text.as_str()), _ => None }).collect();
    assert_eq!(prompts, ["Fix the tests"], "environment context is not a prompt");
    assert_eq!(b.iter().filter(|x| matches!(x, EventBody::ToolCalled { .. })).count(), 2);
    assert_eq!(b.iter().filter(|x| matches!(x, EventBody::ToolResulted { .. })).count(), 2);
    assert_eq!(b.iter().filter(|x| matches!(x, EventBody::TurnStarted { .. })).count(), 1);
    assert!(b.iter().any(|x| matches!(x, EventBody::TurnEnded { .. })));
    assert!(b.iter().any(|x| matches!(x, EventBody::ThinkingRecorded { text, .. } if text == "Run them first")));
    assert_eq!(c.preview.unknown_lines, 1);
}

#[test]
fn codex_token_counts_are_cumulative_so_usage_is_the_difference() {
    let c = convert(&[fixture("codex/rollout.jsonl")]).unwrap();
    let t = parse_session(&c.session).unwrap().0.tokens;
    assert_eq!((t.input, t.cache_read, t.output), (350, 150, 50));
}

#[test]
fn a_codex_child_rollout_is_a_subagent_of_its_parent_thread() {
    let c = convert(&[fixture("codex/rollout.jsonl"), fixture("codex/child.jsonl")]).unwrap();
    let h = parse_session(&c.session).unwrap().0;
    assert_eq!(h.agents.len(), 2);
    assert!(h.agents.iter().any(|a| a.parent.is_some()));
    assert!(c.preview.missing.is_empty());
}

#[test]
fn two_unrelated_codex_sessions_are_refused() {
    let mut other = fixture("codex/rollout.jsonl");
    other.text = other.text.replace("\"id\":\"t1\"", "\"id\":\"t9\"");
    let e = convert(&[fixture("codex/rollout.jsonl"), other]).unwrap_err();
    assert!(e.message.contains("different sessions"));
}

#[test]
fn claude_and_codex_files_together_are_refused() {
    let e = convert(&[fixture("codex/rollout.jsonl"), fixture("claude/main.jsonl")]).unwrap_err();
    assert!(e.message.contains("different tools"));
}

fn team_of(files: &[Upload]) -> model::TeamDraft {
    let doc = parse_session(&convert(files).unwrap().session).unwrap().0.team.unwrap();
    controld::portable::madl::read_document(&doc).unwrap().file.team
}

#[test]
fn the_team_has_one_agent_per_distinct_subagent_with_arrows_from_the_parent() {
    let t = team_of(&claude_with_agents(true));
    // Explore ×2 (auth, db) share one Explore agent; general-purpose is the other.
    assert_eq!(t.agents.len(), 3);
    let main = t.agents.iter().find(|a| a.id == "main").unwrap();
    assert!(main.job.starts_with("Add a login page"));
    assert!(main.job.contains("send_agent_message"));
    assert_eq!(t.connections.len(), 2);
    assert!(t.connections.iter().all(|c| c.from == "main" && c.kind == model::ConnectionKind::HandsWorkTo));
    let explore = t.agents.iter().find(|a| a.name == "Explore auth").unwrap();
    assert_eq!(explore.kind, model::AgentKind::Claude);
    assert_eq!(explore.advanced.model.as_deref(), Some("claude-haiku-5-5"));
    assert!(explore.job.starts_with("Find how auth works"));
    assert!(explore.job.contains("started 2 times"));
}

#[test]
fn the_team_keeps_the_session_folder_and_main_model() {
    let t = team_of(&[fixture("claude/main.jsonl")]);
    assert_eq!(t.agents[0].folder.as_deref(), Some("/work/app"));
    assert_eq!(t.agents[0].advanced.model.as_deref(), Some("claude-opus-5-5"));
    assert_eq!(t.name, "Login page");
    assert!(!t.agents[0].job.contains("send_agent_message"), "no teammates, no handoff line");
}

#[test]
fn a_codex_session_rebuilds_a_codex_team() {
    let t = team_of(&[fixture("codex/rollout.jsonl"), fixture("codex/child.jsonl")]);
    assert!(t.agents.iter().all(|a| a.kind == model::AgentKind::Codex));
    assert_eq!(t.agents.len(), 2);
    assert_eq!(t.name, "Imported Codex session");
    assert!(t.agents.iter().any(|a| a.job == "Check the linter"));
}

#[test]
fn the_rebuilt_team_holds_together() {
    for files in [claude_with_agents(true), vec![fixture("codex/rollout.jsonl"), fixture("codex/child.jsonl")]] {
        let t = team_of(&files);
        let errors: Vec<_> = rigspec::validate_draft(&t).into_iter().filter(|d| d.severity == model::IssueSeverity::Error).collect();
        assert!(errors.is_empty(), "{errors:?}");
    }
}

#[test]
fn an_agent_whose_file_is_missing_works_in_the_session_folder() {
    let t = team_of(&claude_with_agents(false));
    let tests = t.agents.iter().find(|a| a.name == "Write tests").unwrap();
    assert_eq!(tests.folder.as_deref(), Some("/work/app"));
}

// ---- Final review fixes

#[test]
fn a_codex_child_whose_file_repeats_its_parents_meta_stays_a_child() {
    let c = convert(&[fixture("codex/rollout.jsonl"), fixture("codex/child-two-metas.jsonl")]).unwrap();
    let h = parse_session(&c.session).unwrap().0;
    assert_eq!(h.agents.len(), 2);
    assert_eq!(h.agents.iter().filter(|a| a.parent.is_some()).count(), 1);
}

fn claude_lines(lines: &[serde_json::Value]) -> Upload {
    Upload { name: "s.jsonl".into(), text: lines.iter().map(|l| l.to_string() + "\n").collect() }
}

fn prompts_of(u: Upload) -> Vec<String> {
    bodies(&convert(&[u]).unwrap().session).into_iter().filter_map(|b| match b { EventBody::MessageQueued { text, .. } => Some(text), _ => None }).collect()
}

#[test]
fn a_prompt_with_an_image_keeps_its_words() {
    use serde_json::json;
    let u = claude_lines(&[json!({"type":"user","sessionId":"s","timestamp":"2026-10-01T10:00:00Z","message":{"role":"user","content":[{"type":"image","source":{"type":"base64","data":"AAAA"}},{"type":"text","text":"Make it look like this"}]}})]);
    assert_eq!(prompts_of(u), vec!["[image]\nMake it look like this".to_string()]);
}

#[test]
fn claude_codes_own_shell_and_notification_lines_are_not_prompts() {
    use serde_json::json;
    let line = |text: &str| json!({"type":"user","sessionId":"s","timestamp":"2026-10-01T10:00:00Z","message":{"role":"user","content":text}});
    let u = claude_lines(&[
        line("Fix the bug"),
        line("<bash-input>ls</bash-input>"),
        line("<bash-stdout>secret.txt</bash-stdout><bash-stderr></bash-stderr>"),
        line("<task-notification>done</task-notification>"),
        json!({"type":"user","sessionId":"s","isCompactSummary":true,"timestamp":"2026-10-01T10:00:01Z","message":{"role":"user","content":"This session is being continued..."}}),
    ]);
    let c = convert(&[u]).unwrap();
    let b = bodies(&c.session);
    let prompts: Vec<_> = b.iter().filter_map(|x| match x { EventBody::MessageQueued { text, .. } => Some(text.as_str()), _ => None }).collect();
    assert_eq!(prompts, ["Fix the bug"]);
    assert!(b.iter().any(|x| matches!(x, EventBody::AgentText { text, .. } if text == "[context compacted]")));
}

#[test]
fn a_subagent_without_meta_links_to_its_real_parent_whatever_the_upload_order() {
    use serde_json::json;
    let call = |id: &str, desc: &str, prompt: &str| json!({"type":"assistant","sessionId":"s","timestamp":"2026-10-01T10:00:01Z","message":{"id":format!("m-{id}"),"role":"assistant","model":"m","content":[{"type":"tool_use","id":id,"name":"Agent","input":{"description":desc,"subagent_type":"general-purpose","prompt":prompt}}]}});
    let first = |agent: &str, prompt: &str| json!({"type":"user","sessionId":"s","isSidechain":true,"agentId":agent,"timestamp":"2026-10-01T10:00:02Z","message":{"role":"user","content":prompt}});
    let main = claude_lines(&[json!({"type":"user","sessionId":"s","timestamp":"2026-10-01T10:00:00Z","message":{"role":"user","content":"Go"}}), call("t-a", "Agent A", "Do A\n")]);
    let a = Upload { name: "agent-a.jsonl".into(), text: [first("a", "Do A\n"), call("t-b", "Agent B", "Do B\n")].iter().map(|l| l.to_string() + "\n").collect() };
    let b = Upload { name: "agent-b.jsonl".into(), text: format!("{}\n", first("b", "Do B\n")) };
    let h = parse_session(&convert(&[main, b, a]).unwrap().session).unwrap().0;
    let id = |n: &str| h.agents.iter().find(|x| x.name.as_deref() == Some(n)).unwrap().id.clone();
    let parent = |n: &str| h.agents.iter().find(|x| x.name.as_deref() == Some(n)).unwrap().parent.clone();
    assert_eq!(parent("Agent B"), Some(id("Agent A")));
    assert_eq!(h.agents.len(), 3, "no invented missing agents");
}

#[test]
fn the_same_file_uploaded_twice_counts_once() {
    let mut files = claude_with_agents(true);
    files.push(fixture("claude/subagents/agent-a1.jsonl"));
    let h = parse_session(&convert(&files).unwrap().session).unwrap().0;
    assert_eq!(h.agents.len(), 4);
}
