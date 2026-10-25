//! Who writes a team's status page (`TeamDraft::status_owner`), on the built-in templates.
use rigspec::templates::template;

fn owner(id: &str) -> Option<String> {
    template(id, "t", "T", false)
        .unwrap()
        .status_owner()
        .map(|a| a.id.clone())
}

#[test]
fn the_end_of_the_line_writes_it_else_the_lead_else_the_first_agent() {
    assert_eq!(owner("builder-reviewer").as_deref(), Some("reviewer"));
    // Two builders end the line, so the planner that leads them writes it.
    assert_eq!(owner("planner-builders").as_deref(), Some("planner"));
    assert_eq!(owner("solo").as_deref(), Some("claude"));
    assert_eq!(owner("blank"), None);
}

#[test]
fn a_workflow_s_collector_writes_it() {
    assert_eq!(owner("textbook").as_deref(), Some("frontend"));
}

#[test]
fn the_agent_chosen_for_it_writes_it() {
    let mut d = template("builder-reviewer", "t", "T", false).unwrap();
    d.agents[0].advanced.writes_status = true;
    assert_eq!(d.status_owner().map(|a| a.id.as_str()), Some("builder"));
    let json = serde_json::to_value(&d).unwrap();
    assert_eq!(json["agents"][0]["advanced"]["writes_status"], true);
    assert!(
        json["agents"][1]["advanced"].get("writes_status").is_none(),
        "left out when off"
    );
}
