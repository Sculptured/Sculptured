//! Community publishes Sculptured's example teams from plain team JSON (it does not read MADL), made
//! here from the MADL documents. Regenerate with `UPDATE_EXAMPLES=1 cargo test --test community_examples`.
use std::path::Path;

#[test]
fn communitys_example_teams_match_their_madl_documents() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    for (madl, out) in [
        (
            "examples/teams/gstack-roles.madl.json",
            "examples/community/gstack-product-team.json",
        ),
        (
            "docs/showcase/coding-hospital.madl.json",
            "examples/community/coding-hospital.json",
        ),
    ] {
        let team =
            controld::portable::madl::read(&std::fs::read_to_string(root.join(madl)).unwrap())
                .unwrap()
                .file;
        let text = serde_json::to_string_pretty(&team).unwrap() + "\n";
        let path = root.join(out);
        if std::env::var_os("UPDATE_EXAMPLES").is_some() {
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, &text).unwrap();
        }
        assert_eq!(
            std::fs::read_to_string(&path).unwrap_or_default(),
            text,
            "{out} is stale: run with UPDATE_EXAMPLES=1"
        );
    }
}
