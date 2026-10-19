//! RigSpec YAML → draft. What the builder cannot represent is listed as dropped.
use super::layout::auto_layout;
use super::meta::{read_meta, AgentMeta, GroupMeta, Meta};
use super::{dropped, dropped_with, Imported};
use crate::compile::seat_id;
use crate::diag::present_paths;
use crate::draft::{spec_yaml, AGENT_REF, RESTRICTED_POLICY};
use crate::{compile, parse, CompiledRig, CompiledSeat, Diagnostic, MemberSpec, PodSpec, RestorePolicy, RigSpec, Runtime, Severity};
use model::{
    AgentAdvanced, AgentKind, ConnectionKind, DraftAgent, DraftConnection, DraftGroup, ModelDefaults, PermissionPosture,
    ResumeBehaviour, TeamDraft, DEFAULT_GROUP_ID,
};
use std::collections::HashMap;

/// Validates `yaml` with the normal compiler, then converts it. `id_for` receives the
/// spec's name and returns the new team's id. A blank `cwd` (how export writes a Claude or
/// Codex agent with no folder) imports as "no folder"; the draft's own checks then ask for one.
pub fn import_yaml(yaml: &str, id_for: impl FnOnce(&str) -> String) -> Result<Imported, Vec<Diagnostic>> {
    let spec = parse(yaml)?;
    let rig = compile_import(yaml, &spec)?;
    let mut lost = Vec::new();
    let meta = read_meta(yaml, &mut lost);
    spec_drops(&spec, &mut lost);
    let id = id_for(&spec.name);
    let mut im = Importer { spec: &spec, rig: &rig, meta, lost, agents: Vec::new(), keys: Vec::new(), agent_of: HashMap::new() };
    im.import_agents()?;
    let connections = im.connections();
    Ok(im.finish(id, connections))
}

/// The YAML is parsed twice on purpose: `compile` validates and compiles from text
/// (rigspec has no compile-from-parsed entry point), and the converter reads the
/// parsed spec's optional fields. A spec is small, and import is a rare expert action.
/// Parsing first keeps syntax errors' line numbers on the user's own text; only when a
/// `cwd` is blank is the parsed spec re-serialized (with `.`) for the compiler, whose
/// remaining errors then carry structural paths, not line numbers.
fn compile_import(yaml: &str, spec: &RigSpec) -> Result<CompiledRig, Vec<Diagnostic>> {
    let mut compilable = spec.clone();
    let mut blank_cwd = false;
    for m in compilable.pods.iter_mut().flat_map(|p| p.members.iter_mut()) {
        if m.cwd.trim().is_empty() {
            m.cwd = ".".into();
            blank_cwd = true;
        }
    }
    if blank_cwd { compile(&spec_yaml(&compilable)) } else { compile(yaml) }
}

/// The builder holds only "restricted" (exported as RESTRICTED_POLICY) or "unrestricted".
fn custom_policy(p: &Option<String>) -> bool {
    p.as_deref().is_some_and(|p| p != "none" && p != RESTRICTED_POLICY)
}

fn spec_drops(spec: &RigSpec, lost: &mut Vec<Diagnostic>) {
    if custom_policy(&spec.permission_policy) {
        lost.push(dropped("permission_policy".into()));
    }
    let fields = [
        ("summary", spec.summary.is_some()),
        ("culture_file", spec.culture_file.is_some()),
        ("managed_blocks", spec.managed_blocks.is_some()),
        ("workspace", spec.workspace.is_some()),
        ("docs", spec.docs.is_some()),
        ("startup", spec.startup.is_some()),
        ("services", spec.services.is_some()),
    ];
    lost.extend(present_paths("", &fields).map(dropped));
}

fn member_drops(path: &str, m: &MemberSpec, seat: &CompiledSeat, lost: &mut Vec<Diagnostic>) {
    let fields = [
        ("agent_ref", m.agent_ref != AGENT_REF),
        ("profile", m.profile != "default"),
        ("role", m.role.is_some()),
        ("effort", m.effort.is_some()),
        ("compaction_strategy", m.compaction_strategy.is_some()),
        ("mechanic", m.mechanic.is_some()),
        ("codex_config_profile", m.codex_config_profile.is_some()),
        ("session_source", m.session_source.is_some()),
        ("starter_ref", m.starter_ref.is_some()),
        ("startup", m.startup.is_some()),
    ];
    lost.extend(present_paths(path, &fields).map(dropped));
    // Only full paths are folders the builder can hold. `.` (and a blank `cwd`, already
    // `.` here) is the default working folder, so nothing is lost; any other relative
    // folder is.
    if !seat.cwd.starts_with('/') && !matches!(seat.cwd.trim(), "." | "./") {
        lost.push(dropped(format!("{path}.cwd")));
    }
    if custom_policy(&m.permission_policy) {
        lost.push(dropped(format!("{path}.permission_policy")));
    }
}

fn kind_of(runtime: Runtime, path: &str) -> Result<AgentKind, Diagnostic> {
    match runtime {
        Runtime::ClaudeCode => Ok(AgentKind::Claude),
        Runtime::Codex => Ok(AgentKind::Codex),
        Runtime::Stub => Ok(AgentKind::Practice),
        other => Err(Diagnostic {
            path: format!("{path}.runtime"),
            severity: Severity::Error,
            message: format!("{} agents are not supported by the team builder", other.as_str()),
        }),
    }
}

fn resume_of(r: RestorePolicy) -> ResumeBehaviour {
    match r {
        RestorePolicy::ResumeIfPossible => ResumeBehaviour::ResumeIfPossible,
        RestorePolicy::RelaunchFresh => ResumeBehaviour::RelaunchFresh,
        RestorePolicy::CheckpointOnly => ResumeBehaviour::CheckpointOnly,
    }
}

fn connection_of(kind: &str) -> Option<ConnectionKind> {
    match kind {
        "delegates_to" => Some(ConnectionKind::HandsWorkTo),
        "escalates_to" => Some(ConnectionKind::ReportsTo),
        "can_observe" => Some(ConnectionKind::Watches),
        _ => None,
    }
}

fn draft_agent(id: String, pod: &PodSpec, m: &MemberSpec, seat: &CompiledSeat, kind: AgentKind, am: Option<&AgentMeta>) -> DraftAgent {
    let restricted = seat.permission_policy.as_deref().is_some_and(|p| p != "none");
    DraftAgent {
        id,
        group_id: Some(pod.id.clone()),
        name: seat.label.clone().unwrap_or_else(|| m.id.clone()),
        kind,
        job: am.map(|a| a.job.clone()).unwrap_or_default(),
        folder: (seat.cwd.starts_with('/') && !am.is_some_and(|a| a.inherits_folder)).then(|| seat.cwd.clone()),
        x: am.map_or(0, |a| a.x),
        y: am.map_or(0, |a| a.y),
        advanced: AgentAdvanced { data_access: None, authority: None, skills: vec![], integrations: vec![], turn_limit: None, minute_limit: None, briefing: None,
            model: seat.model.clone(),
            permission: if restricted { PermissionPosture::Restricted } else { PermissionPosture::Unrestricted },
            resume: resume_of(seat.restore_policy),
            token_limit: None,
            check: None,
        },
    }
}

fn draft_group(id: &str, label: &str, gm: Option<&GroupMeta>) -> DraftGroup {
    DraftGroup {
        id: id.to_string(),
        label: label.to_string(),
        x: gm.map_or(0, |g| g.x),
        y: gm.map_or(0, |g| g.y),
        w: gm.map_or(0, |g| g.w),
        h: gm.map_or(0, |g| g.h),
        models: ModelDefaults::default(),
    }
}

/// One import in progress: the parsed and compiled spec, its layout comment, and what is built.
struct Importer<'a> {
    spec: &'a RigSpec,
    rig: &'a CompiledRig,
    meta: Option<Meta>,
    lost: Vec<Diagnostic>,
    agents: Vec<DraftAgent>,
    /// Each agent's layout key (`pod.member`), in agent order.
    keys: Vec<String>,
    /// Seat id → agent id.
    agent_of: HashMap<String, String>,
}

impl Importer<'_> {
    /// Every member that the builder supports becomes an agent; unsupported runtimes are errors.
    fn import_agents(&mut self) -> Result<(), Vec<Diagnostic>> {
        let spec = self.spec;
        let mut errors = Vec::new();
        for (pi, pod) in spec.pods.iter().enumerate() {
            let base = format!("pods[{pi}]");
            let fields = [
                ("summary", pod.summary.is_some()),
                ("continuity_policy", pod.continuity_policy.is_some()),
                ("startup", pod.startup.is_some()),
            ];
            self.lost.extend(present_paths(&base, &fields).map(dropped));
            for (mi, m) in pod.members.iter().enumerate() {
                if let Err(e) = self.import_member(&format!("{base}.members[{mi}]"), pod, m) {
                    errors.push(e);
                }
            }
        }
        if errors.is_empty() { Ok(()) } else { Err(errors) }
    }

    fn import_member(&mut self, path: &str, pod: &PodSpec, m: &MemberSpec) -> Result<(), Diagnostic> {
        let rig = self.rig;
        let seat = rig.seats.iter().find(|s| s.pod == pod.id && s.member == m.id).expect("every member compiles to a seat");
        let kind = kind_of(seat.runtime, path)?;
        member_drops(path, m, seat, &mut self.lost);
        let id = self.free_id(path, pod, m);
        let key = format!("{}.{}", pod.id, m.id);
        let agent = draft_agent(id, pod, m, seat, kind, self.meta.as_ref().and_then(|x| x.agents.get(&key)));
        self.agent_of.insert(seat.id.clone(), agent.id.clone());
        self.agents.push(agent);
        self.keys.push(key);
        Ok(())
    }

    /// Agent ids are unique across the team, member ids only within a group. A repeated id is
    /// renamed (and reported); later members' own ids are reserved too, so a rename never takes one.
    fn free_id(&mut self, path: &str, pod: &PodSpec, m: &MemberSpec) -> String {
        if !self.agents.iter().any(|a| a.id == m.id) {
            return m.id.clone();
        }
        let taken = |id: &str| {
            self.agents.iter().any(|a| a.id == id) || self.spec.pods.iter().flat_map(|p| &p.members).any(|x| x.id == id && x.id != m.id)
        };
        let base = format!("{}-{}", m.id, pod.id);
        let id = std::iter::once(base.clone())
            .chain((2u32..).map(|n| format!("{base}-{n}")))
            .find(|c| !taken(c))
            .expect("an unbounded range always finds a free id");
        let message = format!("renamed to {id}: another agent already uses the id {}", m.id);
        self.lost.push(dropped_with(format!("{path}.id"), &message));
        id
    }

    /// Pod-local edges first, then cross-pod edges; one whose ends or kind the builder cannot hold is dropped.
    fn connections(&mut self) -> Vec<DraftConnection> {
        let spec = self.spec;
        let mut out = Vec::new();
        for (pi, pod) in spec.pods.iter().enumerate() {
            for (ei, e) in pod.edges.iter().enumerate() {
                let (from, to) = (seat_id(spec, &pod.id, &e.from), seat_id(spec, &pod.id, &e.to));
                self.connect(&mut out, format!("pods[{pi}].edges[{ei}]"), &e.kind, &from, &to);
            }
        }
        for (ei, e) in spec.edges.iter().enumerate() {
            let (fp, fm) = e.from.split_once('.').expect("validated cross-pod edge");
            let (tp, tm) = e.to.split_once('.').expect("validated cross-pod edge");
            self.connect(&mut out, format!("edges[{ei}]"), &e.kind, &seat_id(spec, fp, fm), &seat_id(spec, tp, tm));
        }
        out
    }

    fn connect(&mut self, out: &mut Vec<DraftConnection>, path: String, kind: &str, from: &str, to: &str) {
        let (Some(kind), Some(f), Some(t)) = (connection_of(kind), self.agent_of.get(from), self.agent_of.get(to)) else {
            self.lost.push(dropped(path));
            return;
        };
        out.push(DraftConnection { id: format!("c{}", out.len() + 1), from: f.clone(), to: t.clone(), kind });
    }

    fn meta_group(&self, id: &str) -> Option<&GroupMeta> {
        self.meta.as_ref().and_then(|m| m.groups.iter().find(|g| g.id == id))
    }

    /// The comment's layout is used only when it covers every agent and every drawn group.
    fn positioned(&self) -> bool {
        self.meta.as_ref().is_some_and(|m| {
            self.keys.iter().all(|k| m.agents.contains_key(k))
                && self.spec.pods.iter().all(|p| p.id == DEFAULT_GROUP_ID || self.meta_group(&p.id).is_some())
        })
    }

    /// The spec's pods as groups, plus drawn groups with no agents (only the layout comment keeps
    /// them), in the comment's order.
    fn groups(&self, ungrouped_team: bool) -> Vec<DraftGroup> {
        let mut groups: Vec<DraftGroup> = self
            .spec
            .pods
            .iter()
            .filter(|p| !(ungrouped_team && p.id == DEFAULT_GROUP_ID))
            .map(|p| draft_group(&p.id, &p.label, self.meta_group(&p.id)))
            .collect();
        if let Some(m) = &self.meta {
            for g in &m.groups {
                if !groups.iter().any(|x| x.id == g.id) {
                    groups.push(draft_group(&g.id, &g.label, Some(g)));
                }
            }
            let order = |id: &str| m.groups.iter().position(|g| g.id == id).unwrap_or(usize::MAX);
            groups.sort_by_key(|g| order(&g.id));
        }
        groups
    }

    fn finish(mut self, id: String, connections: Vec<DraftConnection>) -> Imported {
        let name = self.meta.as_ref().map(|m| m.name.trim().to_string()).filter(|n| !n.is_empty()).unwrap_or_else(|| self.spec.name.clone());
        let positioned = self.positioned();
        // A `team` pod that the layout does not list as a drawn group holds the agents that were
        // outside every group: they stay ungrouped rather than gaining a box.
        let ungrouped_team = positioned && self.meta_group(DEFAULT_GROUP_ID).is_none();
        if ungrouped_team {
            for a in self.agents.iter_mut().filter(|a| a.group_id.as_deref() == Some(DEFAULT_GROUP_ID)) {
                a.group_id = None;
            }
        }
        let groups = self.groups(ungrouped_team);
        let agents = std::mem::take(&mut self.agents);
        let folder = self.meta.as_ref().and_then(|m| m.folder.clone());
        let workflow = self.meta.as_ref().and_then(|m| m.workflow.clone());
        let mut draft = TeamDraft { claude_setup: Default::default(), protection: Default::default(), safety: vec![], components: vec![], budget: Default::default(), input: None, schedules: vec![], decision_log: false, limits: Default::default(), workflow, folder, id, name, groups, agents, connections, models: ModelDefaults::default(), revision: 0 };
        if !positioned {
            let seat_ids: HashMap<String, String> = self.agent_of.iter().map(|(seat, agent)| (agent.clone(), seat.clone())).collect();
            auto_layout(&mut draft, self.rig, &seat_ids);
        }
        Imported { draft, dropped: self.lost }
    }
}
