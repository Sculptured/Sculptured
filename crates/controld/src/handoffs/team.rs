//! The team as handoffs see it: agent names, Jobs, and who hands work to whom.
use crate::store::Store;
use crate::views::SeatView;
use model::{SeatLifecycle, Subject, TeamDraft};
use rigspec::{CompiledRig, EdgeKind};
use std::collections::{HashMap, HashSet};

/// Agent names by seat id, for messages and reasons.
#[derive(Clone, Debug, Default)]
pub(crate) struct TeamNames {
    names: Vec<(String, String)>,
}

impl TeamNames {
    pub(crate) fn new(rig: &CompiledRig, draft: Option<&TeamDraft>) -> Self {
        let names = rig
            .seats
            .iter()
            .map(|s| {
                let from_draft =
                    draft.and_then(|d| d.agent_for_seat(&s.id).map(|a| a.name.trim().to_string()));
                let name = from_draft
                    .filter(|n| !n.is_empty())
                    .or_else(|| s.label.clone().filter(|l| !l.trim().is_empty()))
                    .unwrap_or_else(|| s.member.clone());
                (s.id.clone(), name)
            })
            .collect();
        TeamNames { names }
    }

    /// The agent's name in the builder, else the seat's label, else its member id.
    pub(crate) fn name(&self, seat: &str) -> String {
        self.names
            .iter()
            .find(|(id, _)| id == seat)
            .map(|(_, n)| n.clone())
            .unwrap_or_else(|| {
                let logical = seat.split('@').next().unwrap_or(seat);
                logical.rsplit('.').next().unwrap_or(logical).to_string()
            })
    }

    /// `text` with every seat id of the team replaced by the agent's name.
    pub(crate) fn plain(&self, text: &str) -> String {
        let mut ids: Vec<&(String, String)> = self.names.iter().collect();
        ids.sort_by_key(|(id, _)| std::cmp::Reverse(id.len()));
        ids.iter().fold(text.to_string(), |t, (id, name)| {
            t.replace(id.as_str(), name)
        })
    }
}

pub(super) struct Team<'a> {
    pub(super) rig: &'a CompiledRig,
    draft: Option<TeamDraft>,
    pub(super) names: TeamNames,
    seats: &'a [SeatView],
    /// Seq of each seat's latest lifecycle change.
    changed: HashMap<String, i64>,
}

impl Team<'_> {
    pub(super) fn load<'a>(
        store: &Store,
        rig: &'a CompiledRig,
        seats: &'a [SeatView],
        draft: Option<TeamDraft>,
    ) -> rusqlite::Result<Team<'a>> {
        let names = TeamNames::new(rig, draft.as_ref());
        let mut changed = HashMap::new();
        for s in &rig.seats {
            if let Some(e) = store
                .latest_of_types(&Subject::Seat(s.id.clone()), &["seat-lifecycle-changed"], 1)?
                .first()
            {
                changed.insert(s.id.clone(), e.seq);
            }
        }
        Ok(Team {
            rig,
            draft,
            names,
            seats,
            changed,
        })
    }

    pub(super) fn name(&self, seat: &str) -> String {
        self.names.name(seat)
    }

    pub(super) fn job(&self, seat: &str) -> Option<String> {
        job_of(self.draft.as_ref()?, seat)
    }

    pub(super) fn view(&self, seat: &str) -> Option<&SeatView> {
        self.seats.iter().find(|s| s.id == seat)
    }

    /// The seat's lifecycle as this start knows it. A lifecycle recorded before the start
    /// (an agent stopped in the previous run, not relaunched yet) is "still starting".
    pub(super) fn lifecycle_in(&self, seat: &str, start_seq: i64) -> Option<SeatLifecycle> {
        let view = self.view(seat)?;
        match self.changed.get(seat) {
            Some(&at) if at > start_seq => Some(view.lifecycle),
            _ => Some(SeatLifecycle::Planned),
        }
    }
}

pub(crate) fn job_of(d: &TeamDraft, seat: &str) -> Option<String> {
    let a = d.agent_for_seat(seat)?;
    Some(d.job_with_input(a)).filter(|j| !j.is_empty())
}

pub(crate) fn upstreams_of(rig: &CompiledRig) -> Vec<(String, Vec<String>)> {
    let order: Vec<&str> = rig.seats.iter().map(|s| s.id.as_str()).collect();
    let mut out = Vec::new();
    for b in &order {
        let mut ups: Vec<String> = rig
            .edges
            .iter()
            .filter(|e| {
                e.kind == EdgeKind::DelegatesTo
                    && e.to == *b
                    && e.from != *b
                    && order.contains(&e.from.as_str())
            })
            .map(|e| e.from.clone())
            .collect();
        ups.sort_by_key(|u| order.iter().position(|s| s == u));
        ups.dedup();
        if !ups.is_empty() {
            out.push((b.to_string(), ups));
        }
    }
    out
}

pub(super) fn downstreams_of(rig: &CompiledRig, seat: &str) -> Vec<String> {
    upstreams_of(rig)
        .into_iter()
        .filter(|(_, ups)| ups.iter().any(|u| u == seat))
        .map(|(b, _)| b)
        .collect()
}

/// "Hands work to" arrows that close a loop: back to a seat that launches no later. The launch
/// order already leaves them out (rigspec), so a seat never waits on one before it first runs.
pub(super) fn loop_backs(rig: &CompiledRig) -> HashSet<(String, String)> {
    let layer: HashMap<&str, usize> = rig
        .launch_layers
        .iter()
        .enumerate()
        .flat_map(|(i, l)| l.iter().map(move |s| (s.as_str(), i)))
        .collect();
    rig.edges
        .iter()
        .filter(|e| e.kind == EdgeKind::DelegatesTo && e.from != e.to)
        .filter(|e| matches!((layer.get(e.from.as_str()), layer.get(e.to.as_str())), (Some(f), Some(t)) if f >= t))
        .map(|e| (e.from.clone(), e.to.clone()))
        .collect()
}

/// Every seat reachable from `origin` along "hands work to" (not `origin` itself).
pub(super) fn reachable(rig: &CompiledRig, origin: &str) -> HashSet<String> {
    let mut seen = HashSet::new();
    let mut todo = vec![origin.to_string()];
    while let Some(s) = todo.pop() {
        for d in downstreams_of(rig, &s) {
            if d != origin && seen.insert(d.clone()) {
                todo.push(d);
            }
        }
    }
    seen
}

/// Seats in launch order (upstreams before downstreams), then any others.
pub(crate) fn topo_order(rig: &CompiledRig) -> Vec<String> {
    let mut out: Vec<String> = rig.launch_layers.iter().flatten().cloned().collect();
    for s in &rig.seats {
        if !out.contains(&s.id) {
            out.push(s.id.clone());
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reasons_name_agents_not_seat_ids() {
        let names = TeamNames {
            names: vec![
                ("team.b@t".into(), "Builder".into()),
                ("team.b2@t".into(), "Builder 2".into()),
            ],
        };
        assert_eq!(
            names.plain("seat team.b2@t is not running; team.b@t too"),
            "seat Builder 2 is not running; Builder too"
        );
        assert_eq!(names.name("team.x@t"), "x");
    }
}
