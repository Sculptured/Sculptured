//! The design assistant: the person's own CLI (a fixture here; no model is called) changes the design
//! as MADL; the answer comes back as a proposal and the saved design is untouched until approved.
mod common;
use axum::http::StatusCode;

use common::m2::{Served, rid};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

/// A fake `claude` that logs each prompt and answers with `answer1.json`, then `answer2.json`…
fn fake_claude(dir: &Path) -> PathBuf {
    let p = dir.join("claude");
    let body =
        r#"N=$(cat $D/n 2>/dev/null || echo 0); N=$((N+1)); echo $N > $D/n; cat $D/answer$N.json"#;
    std::fs::write(&p, format!("#!/bin/sh\n[ \"$1\" = --version ] && exit 0\nD={}\ncat >> $D/prompts\necho '---' >> $D/prompts\n{body}\n", dir.display())).unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
    for _ in 0..100 {
        match std::process::Command::new(&p).arg("--version").output() {
            Err(e) if e.raw_os_error() == Some(26) => {
                std::thread::sleep(std::time::Duration::from_millis(20))
            }
            _ => break,
        }
    }
    p
}

fn answer(dir: &Path, n: usize, summary: &str, madl: &str) {
    let v = json!({ "type": "result", "structured_output": { "summary": summary, "madl": madl } });
    std::fs::write(dir.join(format!("answer{n}.json")), v.to_string()).unwrap();
}

async fn served(dir: &Path) -> Served {
    let mut s = crate::common::m2::serve().await;
    s.st = s.st.clone().with_cli_programs(Some(fake_claude(dir)), None);
    s.router = controld::app(s.st.clone(), None);
    s
}

fn opts() -> Value {
    json!({ "cli": "claude", "model": null })
}

const WITH_TESTER: &str = "madl: draft
system: { id: whatever, name: Pair }
agents:
  - { id: a, name: Alpha, runtime: x-sculptured-practice, instructions: Plan the work in numbered steps. }
  - { id: b, name: Beta, runtime: x-sculptured-practice, instructions: build }
  - { id: tester, name: Tester, runtime: x-sculptured-practice, instructions: Run the tests and report failures. }
connections:
  - { id: c-a-b, from: a, to: b, kind: handoff }
  - { from: b, to: tester, kind: handoff }
";

async fn ask(s: &Served, team: &str, body: Value) -> Value {
    let (status, got) = crate::common::m2::call(
        &s.router,
        "POST",
        &format!("/api/teams/{team}/assistant"),
        body,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{got}");
    got
}

#[tokio::test]
async fn a_proposal_changes_nothing_saved_and_keeps_cards_where_they_were() {
    let d = tempfile::tempdir().unwrap();
    let s = served(d.path()).await;
    let team = crate::common::m2::practice_chain(
        &s,
        "Pair",
        &[("a", "Alpha", "plan"), ("b", "Beta", "build")],
        &[("a", "b")],
    )
    .await;
    let (_, before) = crate::common::m2::get(&s.router, &format!("/api/teams/{team}")).await;
    answer(
        d.path(),
        1,
        "Added a tester after Beta and made Alpha's job specific.",
        WITH_TESTER,
    );
    let p = ask(&s, &team, json!({ "options": opts(), "messages": [{ "role": "person", "text": "Add a tester after Beta" }] })).await;

    let agents = p["draft"]["agents"].as_array().unwrap();
    assert_eq!(
        agents
            .iter()
            .map(|a| a["id"].as_str().unwrap())
            .collect::<Vec<_>>(),
        ["a", "b", "tester"]
    );
    assert_eq!(
        (agents[0]["x"].clone(), agents[1]["x"].clone()),
        (json!(40), json!(300)),
        "existing cards stay put"
    );
    assert!(
        agents[2]["x"].as_i64().unwrap() > 300,
        "the new card goes to the right: {}",
        agents[2]
    );
    assert_eq!(
        p["draft"]["id"],
        json!(team),
        "the saved team's id, whatever the CLI named it"
    );
    assert_eq!(
        p["summary"],
        "Added a tester after Beta and made Alpha's job specific."
    );
    assert_eq!(p["dropped"], json!([]));

    let (_, after) = crate::common::m2::get(&s.router, &format!("/api/teams/{team}")).await;
    assert_eq!(
        after["draft"], before["draft"],
        "nothing is saved until the person approves"
    );
    let prompt = std::fs::read_to_string(d.path().join("prompts")).unwrap();
    assert!(
        prompt.contains("Person: Add a tester after Beta"),
        "{prompt}"
    );
    assert!(
        prompt.contains("id: a")
            && !prompt.contains("layout:")
            && !prompt.contains("com.usesculptured"),
        "the CLI sees the design without board data: {prompt}"
    );
}

#[tokio::test]
async fn iterating_starts_from_the_proposal_and_carries_the_conversation() {
    let d = tempfile::tempdir().unwrap();
    let s = served(d.path()).await;
    let team = crate::common::m2::practice_chain(
        &s,
        "Pair",
        &[("a", "Alpha", "plan"), ("b", "Beta", "build")],
        &[("a", "b")],
    )
    .await;
    answer(d.path(), 1, "Added a tester.", WITH_TESTER);
    let first = ask(&s, &team, json!({ "options": opts(), "messages": [{ "role": "person", "text": "Add a tester after Beta" }] })).await;
    let renamed = WITH_TESTER.replace("name: Tester", "name: QA");
    answer(d.path(), 2, "Renamed the tester to QA.", &renamed);
    let messages = json!([
        { "role": "person", "text": "Add a tester after Beta" },
        { "role": "assistant", "text": "Added a tester." },
        { "role": "person", "text": "Call it QA" },
    ]);
    let second = ask(
        &s,
        &team,
        json!({ "options": opts(), "messages": messages, "proposal": first["madl"] }),
    )
    .await;
    assert_eq!(second["draft"]["agents"][2]["name"], "QA");
    assert_eq!(
        second["draft"]["agents"][2]["x"], first["draft"]["agents"][2]["x"],
        "the new card keeps its place between rounds"
    );
    let prompts = std::fs::read_to_string(d.path().join("prompts")).unwrap();
    let last = prompts
        .split("---")
        .filter(|p| !p.trim().is_empty())
        .last()
        .unwrap();
    assert!(
        last.contains("id: tester")
            && last.contains("You: Added a tester.")
            && last.contains("Person: Call it QA"),
        "{last}"
    );
}

#[tokio::test]
async fn an_invalid_answer_goes_back_once_with_what_was_wrong() {
    let d = tempfile::tempdir().unwrap();
    let s = served(d.path()).await;
    let team = crate::common::m2::practice_chain(
        &s,
        "Pair",
        &[("a", "Alpha", "plan"), ("b", "Beta", "build")],
        &[("a", "b")],
    )
    .await;
    answer(
        d.path(),
        1,
        "Oops.",
        &WITH_TESTER.replace("to: tester", "to: nobody"),
    );
    answer(d.path(), 2, "Added a tester.", WITH_TESTER);
    let p = ask(
        &s,
        &team,
        json!({ "options": opts(), "messages": [{ "role": "person", "text": "Add a tester" }] }),
    )
    .await;
    assert_eq!(p["draft"]["agents"].as_array().unwrap().len(), 3);
    let prompts = std::fs::read_to_string(d.path().join("prompts")).unwrap();
    assert!(
        prompts.contains("could not be used") && prompts.contains("unknown-reference"),
        "{prompts}"
    );
}

#[tokio::test]
async fn asking_needs_a_request() {
    let d = tempfile::tempdir().unwrap();
    let s = served(d.path()).await;
    let team = crate::common::m2::practice_chain(&s, "Pair", &[("a", "Alpha", "plan")], &[]).await;
    let (status, body) = crate::common::m2::call(
        &s.router,
        "POST",
        &format!("/api/teams/{team}/assistant"),
        json!({ "options": opts(), "messages": [] }),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
}

#[tokio::test]
async fn a_proposal_keeps_the_saved_budget_and_agent_limits() {
    let d = tempfile::tempdir().unwrap();
    let s = served(d.path()).await;
    let team = crate::common::m2::practice_chain(
        &s,
        "Pair",
        &[("a", "Alpha", "plan"), ("b", "Beta", "build")],
        &[("a", "b")],
    )
    .await;
    let (_, rec) = crate::common::m2::get(&s.router, &format!("/api/teams/{team}")).await;
    let mut draft = rec["draft"].clone();
    draft["budget"] = json!({ "plan": [{ "provider": "claude", "window": "five_hour", "hard": 30.0 }], "max_working": 3 });
    draft["agents"][0]["advanced"]["turn_limit"] = json!({ "hard": 12 });
    let (status, body) = crate::common::m2::call(
        &s.router,
        "PUT",
        &format!("/api/teams/{team}"),
        json!({ "request_id": rid(), "revision": draft["revision"], "draft": draft }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    answer(d.path(), 1, "Added a tester.", WITH_TESTER);
    let p = ask(
        &s,
        &team,
        json!({ "options": opts(), "messages": [{ "role": "person", "text": "Add a tester" }] }),
    )
    .await;
    assert_eq!(
        p["draft"]["budget"],
        json!({ "plan": [{ "provider": "claude", "window": "five_hour", "hard": 30.0 }], "max_working": 3 }),
        "the proposal said nothing about budgets"
    );
    assert_eq!(
        p["draft"]["agents"][0]["advanced"]["turn_limit"],
        json!({ "hard": 12 })
    );
    let prompt = std::fs::read_to_string(d.path().join("prompts")).unwrap();
    assert!(
        prompt.contains("max_working: 3"),
        "the CLI sees the budget it designs within: {prompt}"
    );
    assert!(
        prompt.contains("Split along the work's real dependencies")
            && prompt.contains("`data_access`")
            && prompt.contains("`scope`"),
        "the assistant knows the design rules and contracts"
    );
}
