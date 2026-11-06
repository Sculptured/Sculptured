//! What a Run in Arena sends and starts, worked out without I/O: each board agent as Arena registers
//! it, the run's own team (the board's design, each agent given its Arena MCP endpoint, the agents
//! nobody hands work to given Arena's task), and each usage report and end as Arena records them.
use model::{Component, ComponentKind, EventBody, TeamDraft};
use serde_json::{Value, json};

/// The id of the MCP server Run in Arena gives `agent` (one per agent: each has its own endpoint).
#[must_use]
pub fn server_id(agent: &str) -> String {
    let id: String = format!("arena-{agent}")
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .take(40)
        .collect();
    id
}

/// One board agent as Arena registers it on an external run: Arena does not run it (`external`);
/// its model and job are recorded with it.
#[must_use]
pub fn registration(d: &TeamDraft, agent: &model::DraftAgent) -> Value {
    json!({
        "id": agent.id,
        "name": agent.name,
        "kind": "external",
        "model": d.effective_model(agent),
        "prompt": agent.job,
    })
}

/// Arena's task, in the words the agents get: its ticket and its instructions.
#[derive(Clone, Debug, Default)]
pub struct Task {
    pub id: String,
    pub ticket: String,
    pub instructions: String,
}

impl Task {
    /// The task from `GET /api/tasks`: `ticket` (else `reason`) and `instructions`.
    #[must_use]
    pub fn from_json(v: &Value) -> Task {
        let text = |k: &str| {
            v.get(k)
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string()
        };
        let ticket = Some(text("ticket"))
            .filter(|t| !t.is_empty())
            .unwrap_or_else(|| text("reason"));
        Task {
            id: text("id"),
            ticket,
            instructions: text("instructions"),
        }
    }

    fn brief(&self) -> String {
        let mut out = format!("## Arena task {}\n\n{}", self.id, self.ticket.trim());
        if !self.instructions.trim().is_empty() {
            out.push_str(&format!(
                "\n\n## Instructions\n\n{}",
                self.instructions.trim()
            ));
        }
        out.push_str("\n\nThe systems of this task are the tools of your Arena world MCP server.");
        out
    }
}

fn arena_component(agent: &str, url: &str) -> Component {
    Component {
        id: server_id(agent),
        name: "Arena world".into(),
        description: "The world of this Arena run: its systems' tools, recorded by Arena.".into(),
        kind: ComponentKind::McpRemote,
        program: String::new(),
        args: Vec::new(),
        briefing: None,
        install: "Nothing to install: arenad serves this endpoint while the run is live.".into(),
        claude_only: false,
        requires: Vec::new(),
        homepage: None,
        url: Some(url.to_string()),
        headers: Default::default(),
        skills: Vec::new(),
        plugin: None,
    }
}

/// The run's team: the board's design under `id` and `name`, each agent with its own Arena endpoint
/// (`urls`: agent id → MCP URL), and each agent nobody hands work to given the task after its job.
#[must_use]
pub fn run_draft(
    board: &TeamDraft,
    id: &str,
    name: &str,
    urls: &[(String, String)],
    task: &Task,
) -> TeamDraft {
    let mut d = board.clone();
    d.id = id.to_string();
    d.name = name.to_string();
    d.revision = 0;
    for (agent, url) in urls {
        d.components.push(arena_component(agent, url));
    }
    let entry: Vec<String> = d
        .agents
        .iter()
        .filter(|a| !crate::teams::has_upstream(board, &a.id))
        .map(|a| a.id.clone())
        .collect();
    for a in &mut d.agents {
        a.advanced.integrations.push(server_id(&a.id));
        if entry.contains(&a.id) {
            a.job = format!("{}\n\n{}", a.job.trim(), task.brief())
                .trim()
                .to_string();
        }
    }
    d
}

/// A usage report as Arena takes it from an external design (one model call or turn the CLI reported).
#[must_use]
pub fn usage_report(seq: i64, body: &EventBody) -> Option<Value> {
    let EventBody::UsageReported {
        input_tokens,
        output_tokens,
        cache_read_tokens,
        cache_write_tokens,
        ..
    } = body
    else {
        return None;
    };
    Some(json!({
        "message_id": format!("sculptured-{seq}"),
        "input_tokens": input_tokens,
        "cache_creation_input_tokens": cache_write_tokens,
        "cache_read_input_tokens": cache_read_tokens,
        "output_tokens": output_tokens,
    }))
}

/// An agent's totals over the run, from its usage reports.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Totals {
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub cost_usd: Option<f64>,
    pub turns_ended: u32,
}

impl Totals {
    pub fn add(&mut self, body: &EventBody) {
        match body {
            EventBody::UsageReported {
                input_tokens,
                output_tokens,
                cache_read_tokens,
                cache_write_tokens,
                cost,
                ..
            } => {
                self.prompt_tokens += input_tokens + cache_read_tokens + cache_write_tokens;
                self.completion_tokens += output_tokens;
                if let Some(c) = cost {
                    self.cost_usd = Some(self.cost_usd.unwrap_or(0.0) + c.usd);
                }
            }
            EventBody::TurnEnded { .. } => self.turns_ended += 1,
            _ => {}
        }
    }

    /// The agent's end as Arena records it: ok unless it failed (with the runtime's reason), and its totals.
    #[must_use]
    pub fn end(&self, failed: Option<&str>, seconds: f64) -> Value {
        json!({
            "ok": failed.is_none(),
            "error": failed,
            "prompt_tokens": self.prompt_tokens,
            "completion_tokens": self.completion_tokens,
            "cost_usd": self.cost_usd,
            "cost_source": self.cost_usd.map(|_| "reported by the agent's CLI, through Sculptured"),
            "seconds": seconds,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{Task, Totals, server_id, usage_report};
    use model::{Cost, CostSource, EventBody, TurnStatus};
    use serde_json::json;

    fn usage(input: u64, output: u64, cache_read: u64, cost: Option<f64>) -> EventBody {
        EventBody::UsageReported {
            turn_id: None,
            input_tokens: input,
            output_tokens: output,
            cache_read_tokens: cache_read,
            cache_write_tokens: 1,
            cost: cost.map(|usd| Cost {
                usd,
                source: CostSource::ClaudeCode,
            }),
            in_turn: false,
        }
    }

    #[test]
    fn a_server_id_is_a_valid_component_id_and_a_task_falls_back_to_its_reason() {
        assert_eq!(server_id("Quality_Lead 2"), "arena-quality-lead-2");
        assert!(server_id(&"x".repeat(60)).len() <= 40);
        let t = Task::from_json(&json!({ "id": "7", "reason": "Ship it", "instructions": "" }));
        assert_eq!(
            (t.ticket.as_str(), t.brief().contains("## Instructions")),
            ("Ship it", false)
        );
    }

    #[test]
    fn usage_goes_to_arena_with_cache_tokens_and_totals_sum_for_the_end() {
        assert_eq!(
            usage_report(9, &usage(10, 5, 100, None)),
            Some(
                json!({ "message_id": "sculptured-9", "input_tokens": 10, "cache_creation_input_tokens": 1, "cache_read_input_tokens": 100, "output_tokens": 5 })
            )
        );
        let mut t = Totals::default();
        t.add(&usage(10, 5, 100, Some(0.5)));
        t.add(&usage(1, 1, 0, None));
        t.add(&EventBody::TurnEnded {
            turn_id: "t".into(),
            status: TurnStatus::Completed,
        });
        assert_eq!(
            (
                t.prompt_tokens,
                t.completion_tokens,
                t.cost_usd,
                t.turns_ended
            ),
            (113, 6, Some(0.5), 1)
        );
        let end = t.end(Some("crashed"), 2.0);
        assert_eq!(
            (end["ok"].as_bool(), end["error"].as_str()),
            (Some(false), Some("crashed"))
        );
    }
}
