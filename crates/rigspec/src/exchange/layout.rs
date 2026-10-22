//! The one-time layout for an imported spec that has no usable layout comment.
use crate::CompiledRig;
use model::TeamDraft;
use std::collections::HashMap;

const CARD: i32 = 200;
const COL: i32 = 260;
const ROW: i32 = 240;
const PAD: i32 = 24;
const HEAD: i32 = 28;
const MARGIN: i32 = 40;
const GROUP_GAP: i32 = 60;

/// One-time layout: groups left to right; inside a group, a column per launch layer.
pub(super) fn auto_layout(
    d: &mut TeamDraft,
    rig: &CompiledRig,
    seat_of_agent: &HashMap<String, String>,
) {
    let layer_of: HashMap<&str, usize> = rig
        .launch_layers
        .iter()
        .enumerate()
        .flat_map(|(i, l)| l.iter().map(move |s| (s.as_str(), i)))
        .collect();
    let layer = |agent_id: &str| {
        seat_of_agent
            .get(agent_id)
            .and_then(|s| layer_of.get(s.as_str()))
            .copied()
            .unwrap_or(0)
    };
    let mut x0 = MARGIN;
    for gi in 0..d.groups.len() {
        x0 += lay_out_group(d, gi, x0, &layer) + GROUP_GAP;
    }
}

/// Places group `gi` at `x0` and its agents in columns by launch layer; returns its width.
fn lay_out_group(d: &mut TeamDraft, gi: usize, x0: i32, layer: &impl Fn(&str) -> usize) -> i32 {
    let gid = d.groups[gi].id.clone();
    let members: Vec<usize> = (0..d.agents.len())
        .filter(|&i| d.agents[i].group_id.as_deref() == Some(gid.as_str()))
        .collect();
    let mut cols: Vec<usize> = members.iter().map(|&i| layer(&d.agents[i].id)).collect();
    cols.sort_unstable();
    cols.dedup();
    let mut rows: HashMap<i32, i32> = HashMap::new();
    let (mut max_c, mut max_r) = (0, 0);
    for &i in &members {
        let c = cols
            .iter()
            .position(|&x| x == layer(&d.agents[i].id))
            .unwrap_or(0) as i32;
        let r = rows.entry(c).or_insert(0);
        d.agents[i].x = x0 + PAD + c * COL;
        d.agents[i].y = MARGIN + HEAD + PAD + *r * ROW;
        max_c = max_c.max(c);
        max_r = max_r.max(*r);
        *r += 1;
    }
    let g = &mut d.groups[gi];
    g.x = x0;
    g.y = MARGIN;
    g.w = PAD * 2 + CARD + max_c * COL;
    g.h = HEAD + PAD * 2 + CARD + max_r * ROW;
    g.w
}
