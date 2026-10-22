//! The stub's starting members and blocks.
use super::{Block, User};
use serde_json::{Value, json};

/// A group block by ana, version 1.
pub const SEED_BLOCK: &str = "b-review-pair";
/// A block of another kind (`"component"`), which this app opens in a later version.
pub const SEED_OTHER_KIND: &str = "b-kindex-setup";

const REVIEW_PAIR: &str = include_str!("../../community/review-pair.group.json");

pub(super) fn you() -> User {
    User {
        handle: None,
        name: "You Example".into(),
        bio: String::new(),
        links: vec![],
        avatar: false,
        compact: false,
        admin: false,
        disabled: false,
    }
}

pub(super) fn ana() -> User {
    User {
        handle: Some("ana".into()),
        name: "Ana Lee".into(),
        bio: "I build review teams.".into(),
        links: vec!["https://ana.example".into()],
        avatar: true,
        compact: false,
        admin: false,
        disabled: false,
    }
}

pub(super) fn blocks() -> Vec<Block> {
    let content: Value = serde_json::from_str(REVIEW_PAIR).expect("the seed block is JSON");
    vec![
        Block {
            id: SEED_BLOCK.into(),
            kind: "group",
            author: "ana".into(),
            version: 1,
            title: "Review pair".into(),
            description: "A writer and a reviewer who checks the draft.".into(),
            content,
            published: true,
            hidden: false,
        },
        Block {
            id: SEED_OTHER_KIND.into(),
            kind: "component",
            author: "ana".into(),
            version: 1,
            title: "Kindex setup".into(),
            description: "A component this app opens in a later version.".into(),
            content: json!({"kind": "component", "component": {"id": "kindex"}}),
            published: true,
            hidden: false,
        },
    ]
}
