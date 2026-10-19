//! The published schemas are generated from the types and describe what the server writes.
use controld::portable;

#[test]
fn published_schemas_match_the_types() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../web/public/schema");
    for (file, schema) in portable::schemas() {
        let text = serde_json::to_string_pretty(&schema).unwrap() + "\n";
        let path = dir.join(file);
        if std::env::var_os("UPDATE_SCHEMAS").is_some() {
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(&path, &text).unwrap();
        }
        assert_eq!(std::fs::read_to_string(&path).unwrap_or_default(), text, "{file} is stale: run with UPDATE_SCHEMAS=1");
    }
}

fn errors(v: &jsonschema::Validator, x: &serde_json::Value) -> Vec<String> {
    v.iter_errors(x).map(|e| e.to_string()).collect()
}

#[test]
fn session_lines_with_their_madl_team_validate_against_the_schema() {
    let schemas: std::collections::HashMap<_, _> = portable::schemas().into_iter().collect();
    assert!(!schemas.contains_key("team-v1.json"), "a team's file is MADL, with MADL's own schema");
    let team = serde_json::to_value(portable::madl::to_madl(&portable::team::PortableTeam {
        team: serde_json::from_value(serde_json::json!({"id":"t","name":"T","groups":[],"agents":[{"id":"a","group_id":null,"name":"A","kind":"practice","folder":null,"x":0,"y":0}],"connections":[],"revision":0})).unwrap(),
        settings: Default::default(),
    }))
    .unwrap();
    let s = jsonschema::validator_for(&serde_json::to_value(&schemas["session-v1.json"]).unwrap()).unwrap();
    let header = serde_json::json!({"format":"sculptured.session/v1","source":"sculptured","team":team,"team_id":"t","started_ms":1,"ended_ms":null,"agents":[{"id":"team.a@t","name":"A","parent":null,"runtime":"stub"}],"tokens":{"input":0,"output":0,"cache_read":0,"cache_write":0}});
    assert!(s.is_valid(&header), "{:?}", errors(&s, &header));
    let line = serde_json::json!({"origin":"observed","event":{"seq":1,"host":"local","host_seq":1,"at_ms":1,"actor":{"kind":"system","id":"check"},"subject":{"kind":"seat","id":"team.a@t"},"provenance":"verified","body":{"type":"check-started","command":"true"}}});
    assert!(s.is_valid(&line), "{:?}", errors(&s, &line));
    assert!(!s.is_valid(&serde_json::json!({"origin":"observed"})), "a line without its event is refused");
}
