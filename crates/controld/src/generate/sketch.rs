//! What the AI fills in (a strict, small "sketch"), and how a sketch becomes a laid-out team draft.
//! The AI never writes ids or coordinates.
use model::{
    AgentAdvanced, AgentKind, ConnectionKind, DraftAgent, DraftConnection, DraftGroup,
    ModelDefaults, TeamDraft,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::{BTreeMap, HashSet};

#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct Sketch {
    pub name: String,
    pub groups: Vec<SketchGroup>,
    pub agents: Vec<SketchAgent>,
    pub arrows: Vec<SketchArrow>,
}
#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct SketchGroup {
    pub name: String,
}
#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct SketchAgent {
    pub name: String,
    pub kind: String,
    pub group: Option<String>,
    pub job: String,
    pub check: Option<String>,
}
#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct SketchArrow {
    pub from: String,
    pub to: String,
    pub kind: String,
}

fn strict(properties: Value) -> Value {
    let required: Vec<String> = properties
        .as_object()
        .expect("built-in schema properties are JSON objects")
        .keys()
        .cloned()
        .collect();
    json!({ "type": "object", "additionalProperties": false, "properties": properties, "required": required })
}

fn sketch_schema() -> Value {
    strict(json!({
        "name": { "type": "string" },
        "groups": { "type": "array", "items": strict(json!({ "name": { "type": "string" } })) },
        "agents": { "type": "array", "items": strict(json!({
            "name": { "type": "string" },
            "kind": { "type": "string", "enum": ["claude", "codex"] },
            "group": { "type": ["string", "null"] },
            "job": { "type": "string" },
            "check": { "type": ["string", "null"] },
        })) },
        "arrows": { "type": "array", "items": strict(json!({
            "from": { "type": "string" },
            "to": { "type": "string" },
            "kind": { "type": "string", "enum": ["hands-work-to", "reports-to", "watches"] },
        })) },
    }))
}

/// `{questions: [{question, suggestions}]}`, 2–4 questions of 2–4 suggestions.
#[must_use]
pub fn questions_schema() -> Value {
    // Counts are asked for in the prompt and enforced by the server: structured-output modes may
    // refuse item limits.
    strict(
        json!({ "questions": { "type": "array", "items": strict(json!({
        "question": { "type": "string" },
        "suggestions": { "type": "array", "items": { "type": "string" } },
    })) } }),
    )
}

/// `{alternatives: [{label, rationale, team}]}`, exactly `count` of them.
#[must_use]
pub fn teams_schema(count: usize) -> Value {
    let _ = count;
    strict(
        json!({ "alternatives": { "type": "array", "items": strict(json!({
        "label": { "type": "string", "enum": ["lean", "balanced", "thorough", "team"] },
        "rationale": { "type": "string" },
        "team": sketch_schema(),
    })) } }),
    )
}

/// Which kinds the created agents have.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kinds {
    /// Keep what the AI chose (also "mixed").
    AsSketched,
    All(AgentKind),
}

impl Kinds {
    #[must_use]
    pub fn from_option(o: Option<&str>) -> Kinds {
        match o {
            Some("claude") => Kinds::All(AgentKind::Claude),
            Some("codex") => Kinds::All(AgentKind::Codex),
            _ => Kinds::AsSketched,
        }
    }
}

pub struct Converted {
    pub team: TeamDraft,
    /// What could not be kept, in plain words.
    pub dropped: Vec<String>,
}

/// Board spacing, matching the built-in templates.
const COL: i32 = 260;
const ROW: i32 = 240;
const CARD: i32 = 200;
const PAD: i32 = 24;
const HEAD: i32 = 32;

fn kind_of(s: &str) -> AgentKind {
    match s {
        "codex" => AgentKind::Codex,
        _ => AgentKind::Claude,
    }
}

pub fn to_draft(s: &Sketch, kinds: Kinds, folder: Option<&str>) -> Converted {
    let mut dropped = Vec::new();
    // Groups: unique ids from their names.
    let mut taken = HashSet::new();
    let mut group_ids = BTreeMap::new();
    for g in &s.groups {
        if group_ids.contains_key(&g.name) {
            continue;
        }
        let id = rigspec::draft::unique_slug(&g.name, "group", |c| {
            taken.contains(c) || c == model::DEFAULT_GROUP_ID
        });
        taken.insert(id.clone());
        group_ids.insert(g.name.clone(), id);
    }
    // Agents: unique ids, and unique names (a repeated name gets " 2", " 3", so arrows and people can
    // tell them apart). Arrows match names ignoring case and surrounding spaces.
    let key = |n: &str| n.trim().to_lowercase();
    let mut agent_ids = HashSet::new();
    let mut by_name: BTreeMap<String, String> = BTreeMap::new();
    let mut agents = Vec::new();
    for a in &s.agents {
        let base = a.name.trim();
        let mut name = base.to_string();
        let mut n = 2;
        while by_name.contains_key(&key(&name)) {
            name = format!("{base} {n}");
            n += 1;
        }
        if name != base {
            dropped.push(format!("Two agents were called \"{base}\"; the second is now \"{name}\", and arrows naming \"{base}\" go to the first"));
        }
        let id = rigspec::draft::unique_slug(&name, "agent", |c| agent_ids.contains(c));
        agent_ids.insert(id.clone());
        by_name.insert(key(&name), id.clone());
        let group_id = match &a.group {
            Some(g) => match group_ids.get(g) {
                Some(id) => Some(id.clone()),
                None => {
                    dropped.push(format!("{} was put in group \"{g}\", which isn't one of the team's groups, so it is in the main group", a.name));
                    None
                }
            },
            None => None,
        };
        let kind = match kinds {
            Kinds::All(k) => k,
            Kinds::AsSketched => kind_of(&a.kind),
        };
        let advanced = AgentAdvanced {
            check: a
                .check
                .clone()
                .map(|c| c.trim().to_string())
                .filter(|c| !c.is_empty()),
            ..Default::default()
        };
        agents.push(DraftAgent {
            id,
            group_id,
            name,
            kind,
            job: a.job.clone(),
            folder: None,
            x: 0,
            y: 0,
            advanced,
            connection: None,
        });
    }
    // Arrows: only between known, different agents; no duplicates.
    let mut connections = Vec::new();
    let mut seen = HashSet::new();
    let mut connection_ids = HashSet::new();
    for r in &s.arrows {
        let (Some(from), Some(to)) = (by_name.get(&key(&r.from)), by_name.get(&key(&r.to))) else {
            dropped.push(format!(
                "The arrow from \"{}\" to \"{}\" names an agent that isn't in the team",
                r.from, r.to
            ));
            continue;
        };
        let kind = match r.kind.as_str() {
            "reports-to" => ConnectionKind::ReportsTo,
            "watches" => ConnectionKind::Watches,
            _ => ConnectionKind::HandsWorkTo,
        };
        if from == to || !seen.insert((from.clone(), to.clone(), r.kind.clone())) {
            dropped.push(format!("The arrow from \"{}\" to \"{}\" was left out (it points at itself or repeats another)", r.from, r.to));
            continue;
        }
        // Unique even for two arrows between the same agents, or ids that join into the same text.
        let base = if kind == ConnectionKind::HandsWorkTo {
            format!("c-{from}-{to}")
        } else {
            format!("c-{from}-{}-{to}", r.kind)
        };
        let id = rigspec::draft::unique_slug(&base, "c", |c| connection_ids.contains(c));
        connection_ids.insert(id.clone());
        connections.push(DraftConnection {
            id,
            from: from.clone(),
            to: to.clone(),
            kind,
        });
    }
    lay_out(&mut agents, &connections);
    let groups = group_boxes(&s.groups, &group_ids, &agents);
    let team = TeamDraft {
        claude_setup: Default::default(),
        protection: Default::default(),
        safety: vec![],
        components: vec![],
        budget: Default::default(),
        input: None,
        schedules: vec![],
        mandate: Default::default(),
        triggers: Vec::new(),
        decision_log: false,
        limits: Default::default(),
        workflow: None,
        folder: folder.map(str::to_string),
        id: Some(rigspec::draft::slug(&s.name))
            .filter(|x| !x.is_empty())
            .unwrap_or_else(|| "generated-team".into()),
        name: s.name.clone(),
        groups,
        agents,
        connections,
        models: ModelDefaults::default(),
        revision: 0,
    };
    Converted { team, dropped }
}

/// Columns follow "hands work to": an agent sits one column right of the furthest agent that hands
/// it work (cycles are cut). Within a column, agents are stacked by group, in sketch order.
fn lay_out(agents: &mut [DraftAgent], connections: &[DraftConnection]) {
    let index: BTreeMap<&str, usize> = agents
        .iter()
        .enumerate()
        .map(|(i, a)| (a.id.as_str(), i))
        .collect();
    let mut column = vec![0i32; agents.len()];
    // Longest path by relaxation, at most n rounds, so a cycle can't loop forever.
    for _ in 0..agents.len() {
        let mut changed = false;
        let matching_connections = connections
            .iter()
            .filter(|c| c.kind == ConnectionKind::HandsWorkTo);
        for c in matching_connections {
            let (f, t) = (index[c.from.as_str()], index[c.to.as_str()]);
            if column[t] < column[f] + 1 && column[f] + 1 < agents.len() as i32 {
                column[t] = column[f] + 1;
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
    let mut order: Vec<usize> = (0..agents.len()).collect();
    order.sort_by_key(|&i| (column[i], agents[i].group_id.clone(), i));
    let mut rows: BTreeMap<i32, i32> = BTreeMap::new();
    for i in order {
        let row = rows.entry(column[i]).or_insert(0);
        agents[i].x = 40 + column[i] * COL;
        agents[i].y = 40 + HEAD + *row * ROW;
        *row += 1;
    }
}

/// A box around each named group's agents; groups without agents are left out.
fn group_boxes(
    groups: &[SketchGroup],
    ids: &BTreeMap<String, String>,
    agents: &[DraftAgent],
) -> Vec<DraftGroup> {
    let mut out = Vec::new();
    for g in groups {
        let Some(id) = ids.get(&g.name) else { continue };
        if out.iter().any(|x: &DraftGroup| &x.id == id) {
            continue;
        }
        let members: Vec<&DraftAgent> = agents
            .iter()
            .filter(|a| a.group_id.as_deref() == Some(id))
            .collect();
        let (Some(x0), Some(y0), Some(x1), Some(y1)) = (
            members.iter().map(|a| a.x).min(),
            members.iter().map(|a| a.y).min(),
            members.iter().map(|a| a.x + CARD).max(),
            members.iter().map(|a| a.y + CARD).max(),
        ) else {
            continue;
        };
        out.push(DraftGroup {
            id: id.clone(),
            label: g.name.clone(),
            x: x0 - PAD,
            y: y0 - PAD - HEAD,
            w: x1 - x0 + 2 * PAD,
            h: y1 - y0 + 2 * PAD + HEAD,
            models: ModelDefaults::default(),
        });
    }
    out
}
