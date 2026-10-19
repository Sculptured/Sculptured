//! Settings both ways: a team's models per runtime, its folder and own copies, question timeout,
//! permissions, protections, Claude Code setup and safety rules are MADL settings blocks.
use crate::portable::team::PortableTeam;
use madl::model as m;
use model::{BriefingLevel, ClaudeSetup, DraftAgent, ModelDefaults, PermissionPosture, Protection, SafetyRule};

/// The approval Sculptured asks a person for: whatever the agent's runtime asks permission for.
pub const RUNTIME_PERMISSIONS: &str = "runtime-permissions";

fn model_ref(provider: &str, id: &Option<String>) -> Option<m::ModelRef> {
    let id = id.as_deref().map(str::trim).filter(|m| !m.is_empty())?;
    Some(m::ModelRef::Id(if id.contains('/') { id.to_string() } else { format!("{provider}/{id}") }))
}

fn models_out(models: &ModelDefaults) -> std::collections::BTreeMap<String, m::ModelRef> {
    let pairs = [("claude-code", model_ref("anthropic", &models.claude)), ("codex", model_ref("openai", &models.codex))];
    pairs.into_iter().filter_map(|(runtime, model)| Some((runtime.to_string(), model?))).collect()
}

/// A group's models as its settings block.
pub fn models_settings(models: &ModelDefaults) -> Option<m::Settings> {
    let models = models_out(models);
    (!models.is_empty()).then(|| m::Settings { preferences: Some(m::Preferences { models, ..Default::default() }), ..Default::default() })
}

pub fn defaults_out(file: &PortableTeam) -> Option<m::Settings> {
    let (team, s) = (&file.team, &file.settings);
    let workspace = (team.folder.is_some() || s.own_copies).then(|| m::Workspace { folder: team.folder.clone(), copy: s.own_copies.then_some(m::CopyMode::Own) });
    let preferences = m::Preferences {
        models: models_out(&team.models),
        workspace,
        questions: s.question_timeout_min.map(|min| m::Questions { timeout: Some(format!("{min}m")), on_timeout: None }),
        context: (!team.protection.is_default()).then(|| context_out(&team.protection)),
        config: (!team.claude_setup.is_default()).then(|| m::RuntimeConfig { user: Some(team.claude_setup.global), project: Some(team.claude_setup.project) }),
        ..Default::default()
    };
    let restrictions = m::Restrictions {
        spawn: team.limits.max_child_levels.map(|d| m::Spawn { max_depth: Some(d), ..Default::default() }),
        approvals: s.posture.map(approvals),
        tools: (!team.safety.is_empty()).then(|| m::ToolRule { refuse: team.safety.iter().map(rule_out).collect(), ..Default::default() }),
        ..Default::default()
    };
    let preferences = (preferences != m::Preferences::default()).then_some(preferences);
    let restrictions = (restrictions != m::Restrictions::default()).then_some(restrictions);
    (preferences.is_some() || restrictions.is_some()).then_some(m::Settings { preferences, restrictions, applies_to: None })
}

fn approvals(p: PermissionPosture) -> m::Approvals {
    let list = match p {
        PermissionPosture::Restricted => vec![RUNTIME_PERMISSIONS.to_string()],
        PermissionPosture::Unrestricted => Vec::new(),
    };
    m::Approvals { required_for: Some(list) }
}

fn context_out(p: &Protection) -> m::ContextCare {
    m::ContextCare { managed: (!p.on).then_some(false), read_max_kb: Some(p.read_kb), tool_output_chars: Some(p.shell_chars), refresh_at_tokens: Some(p.context_tokens.into()) }
}

fn rule_out(r: &SafetyRule) -> m::CallRule {
    m::CallRule { name: r.name.clone(), tool: Some(r.tool_pattern.clone()).filter(|t| t != ".*"), conditions: r.conditions.clone(), reason: r.reason.clone() }
}

/// An agent's own limits, its approvals when it is restricted, and its briefing.
pub fn agent_settings_out(a: &DraftAgent) -> Option<m::Settings> {
    let budget = super::limits::agent_out(&a.advanced);
    let approvals = (a.advanced.permission == PermissionPosture::Restricted).then(|| approvals(PermissionPosture::Restricted));
    let restrictions = Some(m::Restrictions { budget, approvals, ..Default::default() }).filter(|r| *r != m::Restrictions::default());
    let briefing = a.advanced.briefing.map(|b| match b {
        BriefingLevel::Full => m::Briefing::Full,
        BriefingLevel::Minimal => m::Briefing::Minimal,
    });
    let preferences = briefing.map(|b| m::Preferences { briefing: Some(b), ..Default::default() });
    (restrictions.is_some() || preferences.is_some()).then_some(m::Settings { preferences, restrictions, applies_to: None })
}

/// An agent's briefing from its resolved preferences.
pub fn briefing_in(p: &m::Preferences) -> Option<BriefingLevel> {
    p.briefing.map(|b| match b {
        m::Briefing::Full => BriefingLevel::Full,
        m::Briefing::Minimal => BriefingLevel::Minimal,
    })
}

/// The models a settings block gives Claude Code and Codex agents; other runtimes' entries are
/// returned by name, since Sculptured has no place for them.
pub fn models_in(s: Option<&m::Settings>) -> (ModelDefaults, Vec<String>) {
    let Some(models) = s.and_then(|s| s.preferences.as_ref()).map(|p| &p.models) else { return Default::default() };
    let id = |r: &str| models.get(r).map(|m| bare(model_id(m)));
    let others = models.keys().filter(|k| !matches!(k.as_str(), "claude-code" | "codex")).cloned().collect();
    (ModelDefaults { claude: id("claude-code"), codex: id("codex") }, others)
}

pub fn model_id(m: &m::ModelRef) -> &str {
    match m {
        m::ModelRef::Id(id) | m::ModelRef::WithFallback { id, .. } => id,
    }
}

/// The model without its provider.
pub fn bare(id: &str) -> String {
    id.split_once('/').map_or(id, |(_, model)| model).to_string()
}

/// `Some(Restricted)` when a person approves what the runtime asks; `Some(Unrestricted)` for an
/// explicit empty list; otherwise None. The other actions, which Sculptured cannot ask about.
pub fn posture_in(a: Option<&m::Approvals>) -> (Option<PermissionPosture>, Vec<String>) {
    let Some(list) = a.and_then(|a| a.required_for.as_ref()) else { return (None, Vec::new()) };
    let others = list.iter().filter(|x| *x != RUNTIME_PERMISSIONS).cloned().collect();
    let posture = if list.iter().any(|x| x == RUNTIME_PERMISSIONS) {
        Some(PermissionPosture::Restricted)
    } else {
        list.is_empty().then_some(PermissionPosture::Unrestricted)
    };
    (posture, others)
}

pub fn protection_in(c: Option<&m::ContextCare>) -> Protection {
    let d = Protection::default();
    let Some(c) = c else { return d };
    let tokens = c.refresh_at_tokens.map_or(d.context_tokens, |t| u32::try_from(t).unwrap_or(u32::MAX));
    Protection { on: c.managed != Some(false), read_kb: c.read_max_kb.unwrap_or(d.read_kb), shell_chars: c.tool_output_chars.unwrap_or(d.shell_chars), context_tokens: tokens }
}

pub fn setup_in(c: Option<&m::RuntimeConfig>) -> ClaudeSetup {
    let d = ClaudeSetup::default();
    c.map_or(d, |c| ClaudeSetup { global: c.user.unwrap_or(d.global), project: c.project.unwrap_or(d.project) })
}

pub fn rule_in(r: &m::CallRule) -> SafetyRule {
    SafetyRule { name: r.name.clone(), tool_pattern: r.tool.clone().unwrap_or_else(|| ".*".into()), conditions: r.conditions.clone(), reason: r.reason.clone() }
}
