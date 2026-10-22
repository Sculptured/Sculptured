use model::{DispatchTask, TaskStatus, TopicMode, WorkflowRun, WorkflowTask};
use std::collections::HashSet;

pub fn validate(tasks: &[DispatchTask]) -> Result<(), String> {
    for t in tasks {
        let deps: HashSet<_> = t.depends_on.iter().collect();
        if (t.mode == TopicMode::Independent && !deps.is_empty())
            || (t.mode == TopicMode::Dependent && deps.is_empty())
        {
            return Err("Independent topics have no prerequisites; dependent topics must select at least one.".into());
        }
        if deps.len() != t.depends_on.len()
            || deps
                .iter()
                .any(|id| **id == t.id || !tasks.iter().any(|other| &other.id == *id))
        {
            return Err(
                "Topic prerequisites must be unique existing topic IDs, excluding itself.".into(),
            );
        }
    }
    let mut done = HashSet::new();
    loop {
        let before = done.len();
        for t in tasks {
            if t.depends_on.iter().all(|id| done.contains(id)) {
                done.insert(t.id.clone());
            }
        }
        if done.len() == tasks.len() {
            return Ok(());
        }
        if before == done.len() {
            return Err("Topic prerequisites contain a cycle.".into());
        }
    }
}

pub fn waiting<'a>(run: &'a WorkflowRun, t: &'a WorkflowTask) -> Vec<&'a str> {
    t.input
        .depends_on
        .iter()
        .filter(|id| {
            !run.tasks
                .iter()
                .any(|p| &p.input.id == *id && p.status == TaskStatus::Complete)
        })
        .map(String::as_str)
        .collect()
}

pub fn reports(run: &WorkflowRun, t: &WorkflowTask) -> String {
    serde_json::to_string(&run.tasks.iter().filter(|p| t.input.depends_on.contains(&p.input.id)).map(|p|
        serde_json::json!({"id":p.input.id,"title":p.input.title,"accepted_report":p.history.last()})
    ).collect::<Vec<_>>()).expect("accepted workflow reports contain serializable values")
}
