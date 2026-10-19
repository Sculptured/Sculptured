//! Connections, the Input, and the MADL parts Sculptured has no place for, for
//! [`super::import::from_madl`].
use super::import::Cx;
use madl::model as m;
use model::*;

impl Cx<'_> {
    pub(super) fn connection(&mut self, i: usize, c: &m::Connection, team: &TeamDraft) -> Vec<DraftConnection> {
        let at = format!("/connections/{i}");
        let known = |id: &str| team.agents.iter().any(|a| a.id == id) || team.groups.iter().any(|g| g.id == id);
        if !known(&c.from) || !known(&c.to) {
            self.drop(&at, format!("`{}` → `{}` joins something that is not an agent or group on the board", c.from, c.to));
            return Vec::new();
        }
        let Some(kind) = self.kind(&at, c.kind) else { return Vec::new() };
        if c.via.is_some() || c.protocol.is_some() || c.when.is_some() || c.mode.is_some() || !c.carries.is_empty() {
            self.drop(&at, "Sculptured keeps who works with whom; this connection's channel, protocol, condition and contents were not kept");
        }
        let id = c.id.clone().unwrap_or_else(|| format!("{}-{}-{}", c.from, madl::expand::kind_name(c.kind), c.to));
        let mut out = vec![DraftConnection { id: id.clone(), from: c.from.clone(), to: c.to.clone(), kind }];
        if c.direction == Some(m::Direction::Both) {
            out.push(DraftConnection { id: format!("{id}-back"), from: c.to.clone(), to: c.from.clone(), kind });
        }
        out
    }

    fn kind(&mut self, at: &str, kind: m::ConnectionKind) -> Option<ConnectionKind> {
        match kind {
            m::ConnectionKind::Handoff => Some(ConnectionKind::HandsWorkTo),
            m::ConnectionKind::Report => Some(ConnectionKind::ReportsTo),
            m::ConnectionKind::Watch => Some(ConnectionKind::Watches),
            m::ConnectionKind::Delegate | m::ConnectionKind::Review => {
                self.drop(at, "Sculptured draws this as hands work to");
                Some(ConnectionKind::HandsWorkTo)
            }
            m::ConnectionKind::Ask | m::ConnectionKind::Approve => {
                self.drop(at, "Sculptured has no ask or approve connection; agents ask people from the app");
                None
            }
        }
    }

    /// The first input that is not the workflow's becomes the board's Input; it goes to the
    /// starting agents unless it names others.
    pub(super) fn input(&mut self, team: &TeamDraft) -> Option<TeamInput> {
        let workflow_input = super::workflow::input_of(self.doc).filter(|_| team.workflow.is_some());
        let inputs: Vec<(usize, &m::Input)> = self.doc.inputs.iter().enumerate().filter(|(_, i)| Some(i.id.as_str()) != workflow_input).collect();
        for (i, extra) in inputs.iter().skip(1) {
            self.drop(format!("/inputs/{i}"), format!("Sculptured has one Input; `{}` was not kept", extra.id));
        }
        let (_, first) = inputs.first()?;
        let on_board = |id: &String| team.agents.iter().any(|a| &a.id == id);
        let named: Vec<String> = first.to.iter().flat_map(|t| madl::graph::nodes_of(self.doc, t)).filter(on_board).collect();
        let to = if first.to.is_empty() { madl::graph::starting_agents(self.doc).into_iter().filter(on_board).collect() } else { named };
        let p = self.doc.layout.as_ref().and_then(|l| l.nodes.get(&first.id)).copied().unwrap_or(m::Point { x: -240.0, y: 40.0 });
        Some(TeamInput { text: first.text.clone(), x: p.x as i32, y: p.y as i32, to })
    }

    /// Parts of the system Sculptured has no place for yet.
    pub(super) fn unkept(&mut self) {
        let doc = self.doc;
        for (i, c) in doc.channels.iter().enumerate().filter(|(_, c)| c.medium != "decision-log") {
            self.drop(format!("/channels/{i}"), format!("Sculptured does not use channels yet; `{}` was not kept", c.id));
        }
        if doc.communication.as_ref().is_some_and(|c| c.mode.is_some() || !c.deny.is_empty()) {
            self.drop("/communication", "Sculptured does not enforce communication rules yet; they were not kept");
        }
        for (i, s) in doc.steps.iter().enumerate() {
            self.drop(format!("/steps/{i}"), format!("Step `{}` stands outside a workflow; Sculptured has no place for it", s.id));
        }
        let Some(p) = &doc.policies else { return };
        let guards = !p.guards.is_empty() && doc.workflows.is_empty();
        let parts = [("on_failure", p.on_failure.is_some()), ("escalation", !p.escalation.is_empty()), ("guards", guards)];
        for (field, set) in parts {
            if set {
                self.drop(format!("/policies/{field}"), format!("Sculptured does not use `policies.{field}` outside a workflow; it was not kept"));
            }
        }
    }
}
