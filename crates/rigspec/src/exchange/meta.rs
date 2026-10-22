//! The layout comment: what a RigSpec cannot hold, as one line of ASCII-only JSON.
use super::META_PREFIX;
use crate::Diagnostic;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fmt::Write as _;

/// Path and message of the dropped entry for a layout comment that cannot be read.
const LAYOUT_PATH: &str = "layout";
const LAYOUT_UNREADABLE: &str = "not imported: the layout comment could not be read, so names, jobs, positions and empty groups were not restored";
const BAD_LAYOUT_GROUP: &str =
    "not imported: this group in the layout comment has an invalid id or a negative size";

#[derive(Default, Serialize, Deserialize)]
pub(super) struct Meta {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) workflow: Option<model::Workflow>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) folder: Option<String>,
    #[serde(default)]
    pub(super) name: String,
    #[serde(default)]
    pub(super) agents: BTreeMap<String, AgentMeta>,
    /// Every drawn group, in draft order. The spec holds only groups that have agents,
    /// and has no pod for agents outside any drawn group to tell apart from a drawn one.
    #[serde(default)]
    pub(super) groups: Vec<GroupMeta>,
}

#[derive(Serialize, Deserialize)]
pub(super) struct AgentMeta {
    #[serde(default)]
    pub(super) inherits_folder: bool,
    pub(super) x: i32,
    pub(super) y: i32,
    #[serde(default)]
    pub(super) job: String,
}

#[derive(Serialize, Deserialize)]
pub(super) struct GroupMeta {
    pub(super) id: String,
    #[serde(default)]
    pub(super) label: String,
    pub(super) x: i32,
    pub(super) y: i32,
    pub(super) w: i32,
    pub(super) h: i32,
}

/// JSON with every non-ASCII character written as a `\uXXXX` escape (UTF-16, so
/// surrogate pairs above U+FFFF). YAML reads U+0085, U+2028 and U+2029 as line breaks,
/// even inside a comment, so user text left raw could end the comment and add spec keys.
/// Non-ASCII only occurs inside JSON strings, where the escapes decode back unchanged.
pub(super) fn ascii_json(json: &str) -> String {
    let mut out = String::with_capacity(json.len());
    let matching_json = json.chars();
    for c in matching_json {
        if c.is_ascii() {
            out.push(c);
        } else {
            let mut utf16_units = [0u16; 2];
            let matching_c = c.encode_utf16(&mut utf16_units);
            for unit in matching_c {
                write!(out, "\\u{unit:04x}").expect("writing to a String cannot fail");
            }
        }
    }
    out
}

/// The file's layout comment, if it has one that reads. An unreadable comment, and each group
/// it cannot describe, is reported in `lost`; such a group is treated as absent (a group with
/// agents then gets the computed layout, an empty one is not drawn).
pub(super) fn read_meta(yaml: &str, lost: &mut Vec<Diagnostic>) -> Option<Meta> {
    let line = yaml
        .lines()
        .rev()
        .find_map(|l| l.strip_prefix(META_PREFIX))?;
    let Ok(mut meta) = serde_json::from_str::<Meta>(line) else {
        lost.push(crate::exchange::dropped_with(
            LAYOUT_PATH.into(),
            LAYOUT_UNREADABLE,
        ));
        return None;
    };
    let mut i = 0;
    meta.groups.retain(|g| {
        let ok = model::valid_segment(&g.id) && g.w >= 0 && g.h >= 0;
        if !ok {
            lost.push(crate::exchange::dropped_with(
                format!("{LAYOUT_PATH}.groups[{i}]"),
                BAD_LAYOUT_GROUP,
            ));
        }
        i += 1;
        ok
    });
    Some(meta)
}
