//! An agent's skills (the Agent Skills format: a `SKILL.md` with a name, a description and a body).
//! Each belongs to one agent. Only names and descriptions sit in the agent's context until a skill is
//! used: Claude Code loads them natively, other agents read one with `read_skill`.
use serde::{Deserialize, Serialize};
use ts_rs::TS;

pub const MAX_NAME: usize = 64;
pub const MAX_DESCRIPTION: usize = 1024;
/// A skill's body, at most; a longer one belongs in files the skill points to.
pub const MAX_BODY: usize = 100_000;

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema, TS)]
#[ts(export)]
pub struct Skill {
    /// Lowercase letters, digits and hyphens.
    pub name: String,
    /// When to use it: what the agent sees until it loads the skill.
    pub description: String,
    /// The instructions, in Markdown.
    pub body: String,
    /// Where it was copied from (an `https://` address, such as a catalog's SKILL.md at a commit).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub source: Option<String>,
}

impl Skill {
    /// What is wrong with it, in plain words; empty when it is valid.
    pub fn problems(&self) -> Vec<String> {
        let mut out = Vec::new();
        let n = &self.name;
        if n.is_empty() || n.len() > MAX_NAME || !n.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-') || n.starts_with('-') || n.ends_with('-') || n.contains("--") {
            out.push(format!("Skill name {n:?} must be 1–{MAX_NAME} lowercase letters, digits and single hyphens."));
        }
        let d = self.description.trim();
        if d.is_empty() || d.chars().count() > MAX_DESCRIPTION {
            out.push(format!("Skill {n:?} needs a description of 1–{MAX_DESCRIPTION} characters saying when to use it."));
        }
        if self.body.trim().is_empty() || self.body.len() > MAX_BODY {
            out.push(format!("Skill {n:?} needs instructions (at most {} KB).", MAX_BODY / 1000));
        }
        out
    }

    /// A `SKILL.md`: its frontmatter gives the name and description, the rest is the body.
    pub fn from_skill_md(text: &str) -> Result<Skill, String> {
        let text = text.trim_start_matches('\u{feff}');
        let rest = text.strip_prefix("---").ok_or("A SKILL.md starts with a --- frontmatter block naming the skill.")?;
        let (front, body) = rest.split_once("\n---").ok_or("The SKILL.md frontmatter has no closing ---.")?;
        let field = |key: &str| {
            front.lines().find_map(|l| l.strip_prefix(key).and_then(|v| v.strip_prefix(':'))).map(|v| v.trim().trim_matches(|c| c == '"' || c == '\'').to_string())
        };
        let body = body.split_once('\n').map_or("", |(_, b)| b);
        Ok(Skill {
            name: field("name").ok_or("The SKILL.md frontmatter has no name.")?,
            description: field("description").ok_or("The SKILL.md frontmatter has no description.")?,
            body: body.trim_start_matches('\n').to_string(),
            source: None,
        })
    }

    pub fn to_skill_md(&self) -> String {
        let quoted = serde_json::to_string(self.description.trim()).expect("a string serializes");
        format!("---\nname: {}\ndescription: {quoted}\n---\n\n{}", self.name, self.body.trim_start())
    }
}

/// The problems of an agent's skills, duplicates included.
pub fn skills_problems(skills: &[Skill]) -> Vec<String> {
    let mut out: Vec<String> = skills.iter().flat_map(Skill::problems).collect();
    let mut seen = std::collections::BTreeSet::new();
    for s in skills {
        if !seen.insert(s.name.as_str()) {
            out.push(format!("Two skills are named {:?}.", s.name));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_skill_md_round_trips_and_bad_names_are_named() {
        let md = "---\nname: money-rules\ndescription: \"Use when handling amounts: Decimal only.\"\n---\n\n# Money\n\nAlways use Decimal.\n";
        let s = Skill::from_skill_md(md).unwrap();
        assert_eq!((s.name.as_str(), s.description.as_str(), s.body.as_str()), ("money-rules", "Use when handling amounts: Decimal only.", "# Money\n\nAlways use Decimal.\n"));
        assert!(s.problems().is_empty());
        assert_eq!(Skill::from_skill_md(&s.to_skill_md()).unwrap(), s);
        let bad = Skill { name: "Money Rules".into(), description: String::new(), body: "x".into(), ..Default::default() };
        assert_eq!(bad.problems().len(), 2);
        assert_eq!(skills_problems(&[s.clone(), s]).last().unwrap(), "Two skills are named \"money-rules\".");
        assert!(Skill::from_skill_md("no frontmatter").is_err());
    }
}
