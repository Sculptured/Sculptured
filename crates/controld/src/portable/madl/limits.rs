//! Budgets and limits both ways: a team's run budget is MADL's `system.budget` (with plan pauses and
//! the most agents at once); an agent's token, turn and working-time limits are its
//! `restrictions.budget`, with which tokens count.
use madl::model as m;
use model::{ContinueMode, PlanBudget, PlanPause, RunBudget, SoftHard, TokenCount, TokenLimit};

fn out<T: Clone>(soft: Option<T>, hard: T) -> m::Limit<T> {
    match soft {
        Some(s) => m::Limit::SoftHard(m::SoftHard {
            soft: Some(s),
            hard,
        }),
        None => m::Limit::Hard(hard),
    }
}

fn minutes_out(v: SoftHard<u32>) -> m::Limit<String> {
    out(v.soft.map(|s| format!("{s}m")), format!("{}m", v.hard))
}

/// Whole minutes, rounding a part minute down (at least 1) so a limit never grows.
fn minutes(text: &str) -> Option<u32> {
    madl::duration::seconds(text).map(|s| (s / 60).clamp(1, u64::from(u32::MAX)) as u32)
}

fn minutes_in(l: &m::Limit<String>) -> Option<SoftHard<u32>> {
    Some(SoftHard {
        soft: l.soft().and_then(|s| minutes(s)),
        hard: minutes(l.hard())?,
    })
}

fn whole_in(l: &m::Limit<u64>) -> SoftHard<u32> {
    let n = |v: u64| v.min(u64::from(u32::MAX)) as u32;
    SoftHard {
        soft: l.soft().map(|s| n(*s)),
        hard: n(*l.hard()),
    }
}

fn pause_out(p: PlanPause) -> m::PlanPause {
    let with = match p.continue_with {
        ContinueMode::Notes => m::ContinueWith::Notes,
        ContinueMode::Resume => m::ContinueWith::Resume,
    };
    m::PlanPause {
        continue_with: Some(with),
        max_windows: Some(p.max_windows),
    }
}

fn pause_in(p: m::PlanPause) -> PlanPause {
    let continue_with = match p.continue_with {
        Some(m::ContinueWith::Resume) => ContinueMode::Resume,
        Some(m::ContinueWith::Notes) | None => ContinueMode::Notes,
    };
    PlanPause {
        continue_with,
        max_windows: p.max_windows.unwrap_or(3),
    }
}

pub fn run_out(b: &RunBudget, max_agents: Option<u32>) -> Option<m::RunBudget> {
    if b.is_empty() && max_agents.is_none() {
        return None;
    }
    Some(m::RunBudget {
        plan: b
            .plan
            .iter()
            .map(|p| m::PlanShare {
                provider: p.provider.clone(),
                window: p.window.clone(),
                soft: p.soft,
                hard: p.hard,
                pause: p.pause.map(pause_out),
            })
            .collect(),
        tokens: b.tokens.map(|t| out(t.soft, t.hard)),
        cost_usd: None,
        wall_clock: b.minutes.map(minutes_out),
        max_working: b.max_working,
        max_agents,
    })
}

/// The run budget, and the parts Sculptured cannot keep (path, why).
pub fn run_in(b: Option<&m::RunBudget>) -> (RunBudget, Vec<(String, String)>) {
    let Some(b) = b else {
        return (RunBudget::default(), vec![]);
    };
    let mut dropped = vec![];
    if b.cost_usd.is_some() {
        dropped.push(("/system/budget/cost_usd".into(), "Sculptured budgets plan shares, tokens and time, not money; the cost limit was not kept".into()));
    }
    let budget = RunBudget {
        plan: b
            .plan
            .iter()
            .map(|p| PlanBudget {
                provider: p.provider.clone(),
                window: p.window.clone(),
                soft: p.soft,
                hard: p.hard,
                pause: p.pause.map(pause_in),
            })
            .collect(),
        tokens: b.tokens.as_ref().map(|t| SoftHard {
            soft: t.soft().copied(),
            hard: *t.hard(),
        }),
        minutes: b.wall_clock.as_ref().and_then(minutes_in),
        max_working: b.max_working,
    };
    (budget, dropped)
}

pub fn agent_out(adv: &model::AgentAdvanced) -> Option<m::Budget> {
    let t = adv.token_limit;
    if t.is_none() && adv.turn_limit.is_none() && adv.minute_limit.is_none() {
        return None;
    }
    let scope = t.map(|t| {
        if t.include_subagents {
            m::BudgetScope::Shared
        } else {
            m::BudgetScope::Own
        }
    });
    let counts = t
        .map(|t| t.counts)
        .filter(|c| *c != TokenCount::All)
        .map(|c| match c {
            TokenCount::All => m::TokenCount::All,
            TokenCount::New => m::TokenCount::New,
            TokenCount::Output => m::TokenCount::Output,
        });
    Some(m::Budget {
        tokens: t.map(|t| out(t.soft, t.tokens)),
        turns: adv
            .turn_limit
            .map(|v| out(v.soft.map(u64::from), u64::from(v.hard))),
        working_time: adv.minute_limit.map(minutes_out),
        scope,
        counts,
        ..Default::default()
    })
}

/// An agent's (token, turn, minute) limits from its effective budget.
pub fn agent_in(
    b: Option<&m::Budget>,
) -> (
    Option<TokenLimit>,
    Option<SoftHard<u32>>,
    Option<SoftHard<u32>>,
) {
    let Some(b) = b else {
        return (None, None, None);
    };
    let include_subagents = b.scope != Some(m::BudgetScope::Own);
    let counts = match b.counts {
        Some(m::TokenCount::New) => TokenCount::New,
        Some(m::TokenCount::Output) => TokenCount::Output,
        Some(m::TokenCount::All) | None => TokenCount::All,
    };
    let tokens = b.tokens.as_ref().map(|t| TokenLimit {
        soft: t.soft().copied(),
        tokens: *t.hard(),
        counts,
        include_subagents,
    });
    (
        tokens,
        b.turns.as_ref().map(whole_in),
        b.working_time.as_ref().and_then(minutes_in),
    )
}
