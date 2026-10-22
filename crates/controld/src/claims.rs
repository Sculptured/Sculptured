//! Claim checks (an idea from Arbiter): when an agent's turn ends, what it says it changed or ran is
//! compared with the tool calls captured in that turn. A file it says it changed with no edit to it,
//! or passing tests with no command run, goes into the team's log as a heuristic finding. It never
//! interrupts the agent, and a match proves nothing: it only means the claim has a captured action.
use crate::state::AppState;
use model::{EventBody, Subject, TurnStatus};
use regex::Regex;
use std::sync::LazyLock;
use tokio::sync::broadcast::error::RecvError;

pub const SOURCE: &str = "claim-check";

/// "added/created/updated … `path`" within one sentence.
static CHANGED: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\b(?:added|created|updated|wrote|written|edited|changed|modified|fixed|implemented|rewrote|refactored)\b[^.\n]{0,120}?`?((?:[\w.-]+/)*[\w-]+\.[A-Za-z0-9]{1,6})`?").expect("built-in regular expression must be valid")
});
static TESTS_PASS: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\b(?:all\s+)?(?:\d+\s+)?tests?\s+(?:now\s+)?pass(?:es|ed)?\b")
        .expect("built-in regular expression must be valid")
});

/// What the agent claims in `text` that the turn's captured `edits` (file paths written) and
/// `commands` (shell commands run) do not show.
pub fn unverified(text: &str, edits: &[String], commands: &[String]) -> Vec<String> {
    let mut out = Vec::new();
    let matching_changed = CHANGED.captures_iter(text);
    for c in matching_changed {
        let file = c[1].to_string();
        let edited = edits
            .iter()
            .any(|e| e.ends_with(&file) || e.ends_with(&format!("/{file}")));
        let by_shell = commands
            .iter()
            .any(|cmd| cmd.contains(file.rsplit('/').next().unwrap_or(&file)));
        let line = format!("says it changed {file}, but no edit to it was captured in that turn");
        if !edited && !by_shell && !out.contains(&line) {
            out.push(line);
        }
    }
    if TESTS_PASS.is_match(text) && commands.is_empty() {
        out.push("says tests pass, but it ran no command in that turn".into());
    }
    out
}

/// The turn's final text, the files it wrote and the commands it ran.
fn turn(st: &AppState, seat: &str, turn_id: &str) -> Option<(String, Vec<String>, Vec<String>)> {
    let store = st.store.lock().expect("store mutex must not be poisoned");
    let started = store
        .latest_of_types(&Subject::Seat(seat.into()), &["turn-started"], 20)
        .ok()?
        .into_iter()
        .find(|e| matches!(&e.body, EventBody::TurnStarted { turn_id: t } if t == turn_id))?
        .seq;
    let (mut text, mut edits, mut commands) = (String::new(), Vec::new(), Vec::new());
    for e in store.events_for_seat(seat, started, 5000).ok()? {
        match e.body {
            EventBody::AgentText { text: t, .. } => text = t,
            EventBody::ToolCalled { name, input, .. } => match name.as_str() {
                "Write" | "Edit" | "MultiEdit" | "NotebookEdit" | "apply_patch" => {
                    edits.extend(
                        input
                            .get("file_path")
                            .or_else(|| input.get("notebook_path"))
                            .and_then(|v| v.as_str())
                            .map(str::to_string),
                    );
                    // Codex patches name their files inside the patch text.
                    commands.extend(
                        input
                            .get("patch")
                            .or_else(|| input.get("input"))
                            .and_then(|v| v.as_str())
                            .map(str::to_string),
                    );
                }
                "Bash" | "shell" | "exec_command" => commands.push(
                    input
                        .get("command")
                        .map(|c| c.as_str().map_or_else(|| c.to_string(), str::to_string))
                        .unwrap_or_default(),
                ),
                _ => {}
            },
            EventBody::TurnEnded { turn_id: t, .. } if t == turn_id => break,
            _ => {}
        }
    }
    Some((text, edits, commands))
}

fn check_turn(st: &AppState, seat: &str, turn_id: &str) {
    let Some(team) = crate::seat_ids::rig_of(seat) else {
        return;
    };
    let Some((text, edits, commands)) = turn(st, seat, turn_id) else {
        return;
    };
    let found = unverified(&text, &edits, &commands);
    if found.is_empty() {
        return;
    }
    let store = st.store.lock().expect("store mutex must not be poisoned");
    let who = store
        .draft(team)
        .ok()
        .flatten()
        .and_then(|d| d.agent_for_seat(seat).map(|a| a.name.clone()))
        .unwrap_or_else(|| seat.to_string());
    for f in found {
        if let Err(e) = store.add_decision(
            team,
            "Sculptured (heuristic claim check)",
            &format!("{who} {f}."),
            SOURCE,
        ) {
            tracing::warn!(%seat, error = %e, "claim check not kept");
        }
    }
}

pub async fn run(st: AppState) {
    let mut rx = st.hub.subscribe();
    loop {
        match rx.recv().await {
            Ok(item) => {
                if let (
                    Subject::Seat(seat),
                    EventBody::TurnEnded {
                        turn_id,
                        status: TurnStatus::Completed,
                    },
                ) = (&item.event.subject, &item.event.body)
                {
                    check_turn(&st, seat, turn_id);
                }
            }
            Err(RecvError::Lagged(_)) => continue,
            Err(RecvError::Closed) => return,
        }
    }
}

#[cfg(test)]
mod tests {

    #[test]
    fn a_claimed_change_without_a_captured_edit_is_named_and_a_backed_one_is_not() {
        let text = "Added `round_cents` to src/ledgerly/money.py and updated tests/test_money.py. All 4 tests pass.";
        let edits = vec!["/w/src/ledgerly/money.py".to_string()];
        assert_eq!(
            crate::claims::unverified(text, &edits, &[]),
            [
                "says it changed tests/test_money.py, but no edit to it was captured in that turn",
                "says tests pass, but it ran no command in that turn",
            ]
        );
        let commands = vec![
            "python3 -m unittest discover -s tests".to_string(),
            "cat > tests/test_money.py <<EOF".to_string(),
        ];
        assert!(
            crate::claims::unverified(text, &edits, &commands).is_empty(),
            "a shell write and a test run back the claims"
        );
        assert!(
            crate::claims::unverified("I read money.py; it already rounds.", &[], &[]).is_empty(),
            "reading is not a claimed change"
        );
    }
}
