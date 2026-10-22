//! Where the design assistant's new cards go: a new agent joins the right end of its group's box
//! (which widens to fit), anything else new goes to the right of the board. Cards that still
//! exist keep their positions; removed ones lose theirs.
use madl::model as m;
use std::collections::BTreeMap;

/// Room one card takes across, with its gap.
const STEP_X: f64 = 260.0;
const STEP_Y: f64 = 180.0;

pub fn place(mut layout: m::Layout, doc: &m::Document) -> m::Layout {
    let node_ids: Vec<&String> = doc
        .agents
        .iter()
        .map(|a| &a.id)
        .chain(doc.steps.iter().map(|s| &s.id))
        .chain(doc.inputs.iter().map(|i| &i.id))
        .collect();
    layout.nodes.retain(|id, _| node_ids.contains(&id));
    layout
        .groups
        .retain(|id, _| doc.groups.iter().any(|g| &g.id == id));
    layout.units.get_or_insert_with(|| "px".into());
    let mut free = Free::of(&layout);
    new_groups(&mut layout, doc, &mut free);
    let new_agents: Vec<&m::Agent> = doc
        .agents
        .iter()
        .filter(|a| !layout.nodes.contains_key(&a.id))
        .collect();
    for a in new_agents {
        let spot = match a
            .groups
            .first()
            .and_then(|g| layout.groups.get_mut(g).map(|r| (g.clone(), r)))
        {
            Some((g, rect)) => free.in_group(&g, rect),
            None => free.next(),
        };
        layout.nodes.insert(a.id.clone(), spot);
    }
    let new_steps: Vec<String> = doc
        .steps
        .iter()
        .map(|s| s.id.clone())
        .filter(|id| !layout.nodes.contains_key(id))
        .collect();
    for id in new_steps {
        layout.nodes.insert(id, free.next());
    }
    let new_inputs: Vec<String> = doc
        .inputs
        .iter()
        .map(|i| i.id.clone())
        .filter(|id| !layout.nodes.contains_key(id))
        .collect();
    let indexed_new_inputs = new_inputs.into_iter().enumerate();
    for (n, id) in indexed_new_inputs {
        layout.nodes.insert(
            id,
            m::Point {
                x: free.left - STEP_X,
                y: free.top + STEP_Y * n as f64,
            },
        );
    }
    layout
}

/// A new group's box goes right of everything, wide enough for its members.
fn new_groups(layout: &mut m::Layout, doc: &m::Document, free: &mut Free) {
    let new: Vec<&m::Group> = doc
        .groups
        .iter()
        .filter(|g| !layout.groups.contains_key(&g.id))
        .collect();
    for g in new {
        let members = doc
            .agents
            .iter()
            .filter(|a| a.groups.contains(&g.id))
            .count()
            .max(1);
        let rect = m::Rect {
            x: free.right + 80.0,
            y: free.top,
            w: STEP_X * members as f64 + 40.0,
            h: STEP_Y + 80.0,
        };
        free.right = rect.x + rect.w;
        layout.groups.insert(g.id.clone(), rect);
        free.inside.insert(g.id.clone(), 0);
    }
}

/// The board's edges, and how many new cards each new group has taken.
struct Free {
    left: f64,
    right: f64,
    top: f64,
    inside: BTreeMap<String, usize>,
}

impl Free {
    fn of(layout: &m::Layout) -> Self {
        let xs = layout
            .nodes
            .values()
            .map(|p| (p.x, p.x + 240.0))
            .chain(layout.groups.values().map(|r| (r.x, r.x + r.w)));
        let (left, right) = xs.fold((f64::MAX, f64::MIN), |(l, r), (a, b)| (l.min(a), r.max(b)));
        let top = layout
            .nodes
            .values()
            .map(|p| p.y)
            .chain(layout.groups.values().map(|r| r.y))
            .fold(f64::MAX, f64::min);
        let none = left == f64::MAX;
        Free {
            left: if none { 40.0 } else { left },
            right: if none { -40.0 } else { right },
            top: if none { 40.0 } else { top },
            inside: BTreeMap::new(),
        }
    }

    /// The next spot right of everything.
    fn next(&mut self) -> m::Point {
        let p = m::Point {
            x: self.right + 80.0,
            y: self.top,
        };
        self.right = p.x + 240.0;
        p
    }

    /// Inside a group: a new group fills left to right; an existing one widens to fit one more.
    fn in_group(&mut self, group: &str, rect: &mut m::Rect) -> m::Point {
        if let Some(n) = self.inside.get_mut(group) {
            let p = m::Point {
                x: rect.x + 20.0 + STEP_X * *n as f64,
                y: rect.y + 50.0,
            };
            *n += 1;
            return p;
        }
        let p = m::Point {
            x: rect.x + rect.w,
            y: rect.y + 50.0,
        };
        rect.w += STEP_X;
        self.right = self.right.max(rect.x + rect.w);
        p
    }
}
