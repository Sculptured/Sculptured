use serde_json::{Value, json};

pub(super) fn tools() -> Vec<Value> {
    vec![
        json!({
            "name":"dispatch_work", "description":"Configured dispatcher only: create one immutable batch of N work items for the project's bounded dynamic workflow. Retry with identical content is safe. Read workflow_status first. No additional agents are created.",
            "inputSchema":{"type":"object","additionalProperties":false,"properties":{
                "batch_id":{"type":"string"},"summary":{"type":"string","description":"Shared learning goals, source map, file and interface contracts"},
                "tasks":{"type":"array","items":{"type":"object","additionalProperties":false,"properties":{"id":{"type":"string"},"title":{"type":"string"},"context":{"type":"string","description":"Self-contained source sections, prerequisites, goals, artifact directory and constraints"},"mode":{"type":"string","enum":["independent","dependent"],"description":"Independent topics may run concurrently. Dependent topics wait for every depends_on topic to be accepted."},"depends_on":{"type":"array","items":{"type":"string"},"description":"Other topic IDs required before this topic starts. Empty for independent; nonempty for dependent. Cycles are rejected."}},"required":["id","title","context"]}}
            },"required":["batch_id","summary","tasks"]}
        }),
        json!({
            "name":"complete_work", "description":"Finish the current workflow assignment with one configured outcome and evidence. The outcome selects the next stage, a review loop, or completion. Use blocked when you cannot safely continue. Results remain agent-reported.",
            "inputSchema":{"type":"object","additionalProperties":false,"properties":{"assignment":{"type":"string"},"outcome":{"type":"string"},"result":{"type":"string","description":"Artifact paths, exact revision, source references, actual checks and findings; maximum 12000 bytes"}},"required":["assignment","outcome","result"]}
        }),
        json!({"name":"workflow_status","description":"Read the current project's dynamic batch, task context, assignments and review history.","inputSchema":{"type":"object","properties":{},"additionalProperties":false}}),
        json!({"name":"read_decisions","description":"Read the team's decision log: precedents people recorded (newest first, up to 50). Check it before deciding something it may cover. Read-only.",
        "inputSchema":{"type":"object","properties":{"query":{"type":"string","description":"Words to look for; leave out to read the latest entries"}},"additionalProperties":false}}),
    ]
}
