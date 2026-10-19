//! Components both ways: a team's components, and the built-in ones its agents use, are MADL
//! `tools`; an agent's components are its `tools` by id (MEP 0006).
use madl::model as m;
use model::component::{builtins, SAFETY_RULES};
use model::{Component, ComponentKind, TeamDraft};

/// What MADL calls the file of an agent's call rules, in a hook's arguments.
const CALL_RULES: &str = "{call_rules}";

/// The team's components, then each built-in component its agents use (with those it requires).
pub fn tools_out(team: &TeamDraft) -> Vec<m::Tool> {
    let mut out: Vec<m::Tool> = team.components.iter().map(tool_out).collect();
    let used = team.agents.iter().flat_map(|a| team.agent_components(a));
    for c in used.filter(Component::is_builtin) {
        if !out.iter().any(|t| t.id == c.id) {
            out.push(tool_out(&c));
        }
    }
    out
}

fn tool_out(c: &Component) -> m::Tool {
    let args = |a: &[String]| a.iter().map(|x| x.replace(SAFETY_RULES, CALL_RULES)).collect();
    let mut t = m::Tool {
        id: c.id.clone(),
        name: Some(c.name.clone()),
        description: Some(c.description.clone()).filter(|d| !d.is_empty()),
        instructions: c.briefing.clone(),
        runtimes: if c.claude_only { vec!["claude-code".into()] } else { Vec::new() },
        requires: c.requires.clone(),
        install: Some(c.install.clone()).filter(|i| !i.is_empty()),
        homepage: c.homepage.clone(),
        ..Default::default()
    };
    match c.kind {
        ComponentKind::Mcp => t.mcp = Some(m::McpServer { command: Some(c.program.clone()), args: args(&c.args), ..Default::default() }),
        ComponentKind::McpRemote => t.mcp = Some(m::McpServer { url: c.url.clone(), headers: c.headers.clone(), args: args(&c.args), ..Default::default() }),
        ComponentKind::ToolHook => t.hook = Some(m::Hook { command: c.program.clone(), args: args(&c.args) }),
        ComponentKind::ClaudePlugin => t.plugin = c.plugin.clone(),
        ComponentKind::Skills => t.skills = c.skills.iter().map(|s| m::EmbeddedSkill { name: s.name.clone(), description: s.description.clone(), body: s.body.clone() }).collect(),
    }
    t
}

/// The component a tool describes, or why Sculptured cannot use it.
pub fn tool_in(t: &m::Tool) -> Result<Component, String> {
    let args = |a: &[String]| a.iter().map(|x| x.replace(CALL_RULES, SAFETY_RULES)).collect::<Vec<_>>();
    let mut c = Component {
        id: t.id.clone(),
        name: t.name.clone().unwrap_or_else(|| t.id.clone()),
        description: t.description.clone().unwrap_or_default(),
        kind: ComponentKind::Mcp,
        program: String::new(),
        args: Vec::new(),
        briefing: t.instructions.clone(),
        install: t.install.clone().unwrap_or_default(),
        claude_only: t.runtimes == ["claude-code"],
        requires: t.requires.clone(),
        homepage: t.homepage.clone(),
        url: None,
        headers: Default::default(),
        skills: Vec::new(),
        plugin: None,
    };
    if !t.runtimes.is_empty() && !c.claude_only {
        return Err(format!("works with {}; Sculptured gives a component to every agent, or to Claude Code agents only", t.runtimes.join(", ")));
    }
    match (&t.mcp, &t.hook, &t.plugin, t.skills.is_empty(), &t.builtin) {
        (Some(mcp), ..) => mcp_in(mcp, &mut c, args(&mcp.args))?,
        (_, Some(h), ..) => (c.kind, c.program, c.args) = (ComponentKind::ToolHook, h.command.clone(), args(&h.args)),
        (_, _, Some(p), ..) => (c.kind, c.plugin) = (ComponentKind::ClaudePlugin, Some(p.clone())),
        (_, _, _, false, _) => {
            c.kind = ComponentKind::Skills;
            c.skills = t.skills.iter().map(|s| model::Skill { name: s.name.clone(), description: s.description.clone(), body: s.body.clone() }).collect();
        }
        (.., Some(b)) => return Err(format!("`{b}` is one of a runtime's own tools; Sculptured's agents already have their runtime's tools")),
        _ => return Err("names a tool the runner provides, and Sculptured provides none by that name".into()),
    }
    Ok(c)
}

fn mcp_in(mcp: &m::McpServer, c: &mut Component, args: Vec<String>) -> Result<(), String> {
    if !mcp.env.is_empty() {
        return Err("sets environment values for its server, which Sculptured's components do not carry".into());
    }
    c.args = args;
    match (&mcp.command, &mcp.url) {
        (Some(command), _) => (c.kind, c.program) = (ComponentKind::Mcp, command.clone()),
        (None, Some(url)) => (c.kind, c.url, c.headers) = (ComponentKind::McpRemote, Some(url.clone()), mcp.headers.clone()),
        (None, None) => return Err("an MCP server with neither a command nor a URL".into()),
    }
    Ok(())
}

/// Whether `c` is a built-in component exactly as Sculptured ships it.
pub fn is_builtin_as_shipped(c: &Component) -> bool {
    builtins().iter().any(|b| b == c)
}
