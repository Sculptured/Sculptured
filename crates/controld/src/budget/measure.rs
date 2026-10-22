//! How much of each budget a run has used: plan shares from the CLIs' readings, estimated between
//! readings from the run's tokens; tokens; minutes. Pure apart from `spend` and `working`.
use super::record::{PlanTrack, Reading, Record};
use crate::store::Store;
use rusqlite::params;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// A reading at Start older than this is not used as the baseline; the first one after Start is.
pub const FRESH_MS: i64 = 120_000;

/// The run's tokens on one plan, and the largest single report (about one model call).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Spend {
    pub tokens: u64,
    pub max_call: u64,
}

/// One budget's standing, for the board and for the decisions.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ts_rs::TS)]
#[ts(export)]
pub struct Meter {
    /// e.g. `Claude 5-hour`, `Tokens`, `Time`.
    pub what: String,
    /// `percent`, `tokens` or `minutes`.
    pub unit: String,
    /// None while nothing has been measured yet (no plan reading and no learned rate).
    pub used: Option<f64>,
    pub soft: f64,
    pub hard: f64,
    /// What the agents working now may add before a stop reaches them (one model call each).
    pub reserve: f64,
    /// The number includes an estimate made since the last reading.
    pub estimated: bool,
    #[ts(type = "number | null")]
    pub observed_ms: Option<i64>,
    /// Set when reaching the hard value pauses the run (a plan budget with windows left) instead of
    /// stopping it.
    pub pause: Option<PauseAt>,
    /// Which limit this is, for raising it: `plan`, `tokens` or `minutes`.
    #[serde(default)]
    pub limit: String,
    /// For `plan`: the plan window it measures.
    #[serde(default)]
    pub provider: Option<String>,
    #[serde(default)]
    pub window: Option<String>,
}

/// Where a run would pause: until when, and which window of how many it is in.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, ts_rs::TS)]
#[ts(export)]
pub struct PauseAt {
    /// The window's reset plus 2 minutes, so the first reading after it belongs to the new window.
    #[ts(type = "number")]
    pub until_ms: i64,
    pub window: u32,
    pub max_windows: u32,
}

/// After the window resets, the readings of a moment ago may still show the old window.
pub const AFTER_RESET_MS: i64 = 120_000;

/// The pause a plan budget calls for at its hard value: it pauses, it has a window left, and its window
/// says when it resets.
pub(super) fn pause_at(r: &Record, t: &PlanTrack, b: &model::PlanBudget) -> Option<PauseAt> {
    let p = b.pause?;
    let window = r.windows + 1;
    (window < p.max_windows).then_some(())?;
    Some(PauseAt {
        until_ms: t.resets_at? * 1000 + AFTER_RESET_MS,
        window,
        max_windows: p.max_windows,
    })
}

fn provider_of_runtime(runtime: &str) -> &'static str {
    match runtime {
        "claude-code" => "claude",
        "codex" => "codex",
        _ => "other",
    }
}

/// Tokens of every kind reported by `rig`'s agents since `start_seq`, per plan.
pub fn spend(
    store: &Store,
    rig: &str,
    start_seq: i64,
) -> rusqlite::Result<HashMap<&'static str, Spend>> {
    let n = "COALESCE(json_extract(e.json,'$.body.input_tokens'),0) + COALESCE(json_extract(e.json,'$.body.output_tokens'),0)
           + COALESCE(json_extract(e.json,'$.body.cache_read_tokens'),0) + COALESCE(json_extract(e.json,'$.body.cache_write_tokens'),0)";
    // A Claude turn's last report can carry a whole turn's remainder: only its per-call reports count
    // toward the largest call. Codex reports are per call.
    let sql = format!(
        "SELECT s.runtime, COALESCE(SUM({n}), 0),
                COALESCE(MAX(CASE WHEN s.runtime <> 'claude-code' OR json_extract(e.json,'$.body.in_turn') = 1 THEN {n} END), 0) FROM events e JOIN seats s ON e.subject = 'seat:' || s.id
         WHERE s.rig = ?1 AND e.seq > ?2 AND json_extract(e.json, '$.body.type') = 'usage-reported' GROUP BY s.runtime"
    );
    let mut q = store.connection().prepare(&sql)?;
    let rows = q.query_map(params![rig, start_seq], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, i64>(1)?,
            r.get::<_, i64>(2)?,
        ))
    })?;
    let mut out: HashMap<&'static str, Spend> = HashMap::new();
    for row in rows {
        let (runtime, sum, max) = row?;
        let e = out.entry(provider_of_runtime(&runtime)).or_default();
        e.tokens += sum.max(0) as u64;
        e.max_call = e.max_call.max(max.max(0) as u64);
    }
    Ok(out)
}

/// Agents of `rig` working or starting now, per plan.
pub fn working(store: &Store, rig: &str) -> rusqlite::Result<HashMap<&'static str, u32>> {
    let mut q = store.connection().prepare("SELECT runtime, COUNT(*) FROM seats WHERE rig = ?1 AND (activity = 'working' OR lifecycle = 'launching') GROUP BY runtime")?;
    let rows = q.query_map([rig], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))?;
    let mut out: HashMap<&'static str, u32> = HashMap::new();
    for row in rows {
        let (runtime, n) = row?;
        *out.entry(provider_of_runtime(&runtime)).or_default() += n.max(0) as u32;
    }
    Ok(out)
}

/// A reading of the window `t` follows: (percent, resets_at, observed_ms).
pub type Observed = (f64, Option<i64>, i64);

/// Folds a reading into `t`, given the run's tokens on that plan now. A later `resets_at` starts a
/// new window, counted from zero.
pub fn observe(t: &mut PlanTrack, (percent, resets_at, at_ms): Observed, tokens_now: u64) {
    if t.last.is_some_and(|l| at_ms <= l.at_ms) {
        return;
    }
    let reading = Reading {
        percent,
        tokens: tokens_now,
        at_ms,
    };
    if t.resets_at
        .zip(resets_at)
        .is_some_and(|(was, now)| now > was)
    {
        // A new window: everything in it so far is counted (the run's share since it began).
        *t = PlanTrack {
            provider: t.provider.clone(),
            window: t.window.clone(),
            baseline: Some(0.0),
            base_tokens: 0,
            peak: t.peak,
            ..Default::default()
        };
    }
    t.resets_at = resets_at.or(t.resets_at);
    if t.baseline.is_none() {
        t.baseline = Some(percent);
        t.base_tokens = tokens_now;
    }
    t.first.get_or_insert(reading);
    t.last = Some(reading);
}

/// Percent per token on this plan: from the run's readings, else `fallback` (an earlier run's).
#[must_use]
pub fn rate(t: &PlanTrack, fallback: Option<f64>) -> Option<f64> {
    let learned = t.first.zip(t.last).and_then(|(f, l)| {
        let (dp, dt) = (l.percent - f.percent, l.tokens.saturating_sub(f.tokens));
        (dt > 0 && dp > 0.0).then(|| dp / dt as f64)
    });
    learned.or(fallback)
}

/// The run's share of the window, in percent, and whether it includes an estimate.
#[must_use]
pub fn share(t: &PlanTrack, tokens_now: u64, rate: Option<f64>) -> Option<(f64, bool)> {
    let (Some(base), Some(last)) = (t.baseline, t.last) else {
        return rate.map(|r| (tokens_now as f64 * r, true));
    };
    let read = last.percent - base + rate.map_or(0.0, |r| t.base_tokens as f64 * r);
    let est = rate.map(|r| read + tokens_now.saturating_sub(last.tokens) as f64 * r);
    match est {
        Some(e) if e > read + 1e-9 => Some((e, true)),
        _ => Some((read.max(0.0), false)),
    }
}

#[must_use]
pub fn window_words(provider: &str, window: &str) -> String {
    let p = if provider == "claude" {
        "Claude"
    } else {
        "Codex"
    };
    let w = match window {
        "five_hour" => "5-hour",
        "seven_day" => "weekly",
        other => other,
    };
    format!("{p} {w}")
}

/// Every budget's meter for a run, given its spend, working agents and rates per plan.
#[must_use]
pub fn meters(
    r: &Record,
    spend: &HashMap<&str, Spend>,
    working: &HashMap<&str, u32>,
    rates: &HashMap<String, f64>,
    now_ms: i64,
) -> Vec<Meter> {
    let mut out = Vec::new();
    let paired_plans = r.plans.iter().zip(&r.budget.plan);
    for (t, b) in paired_plans {
        let s = spend.get(t.provider.as_str()).copied().unwrap_or_default();
        let rate = rate(
            t,
            rates.get(&format!("{}:{}", t.provider, t.window)).copied(),
        );
        let measured = share(t, s.tokens, rate);
        let reserve = rate.map_or(0.0, |r| {
            working.get(t.provider.as_str()).copied().unwrap_or(0) as f64 * s.max_call as f64 * r
        });
        out.push(Meter {
            what: window_words(&t.provider, &t.window),
            unit: "percent".into(),
            used: measured.map(|m| m.0),
            soft: b.soft_value(),
            hard: b.hard,
            reserve,
            estimated: measured.is_some_and(|m| m.1),
            observed_ms: t.last.map(|l| l.at_ms),
            pause: pause_at(r, t, b),
            limit: "plan".into(),
            provider: Some(t.provider.clone()),
            window: Some(t.window.clone()),
        });
    }
    if let Some(tb) = r.budget.tokens {
        let total: u64 = spend.values().map(|s| s.tokens).sum();
        let max_call = spend.values().map(|s| s.max_call).max().unwrap_or(0);
        let n: u32 = working.values().sum();
        out.push(Meter {
            what: "Tokens".into(),
            unit: "tokens".into(),
            used: Some(total as f64),
            soft: tb.soft_value() as f64,
            hard: tb.hard as f64,
            reserve: (n as u64 * max_call) as f64,
            estimated: false,
            observed_ms: None,
            pause: None,
            limit: "tokens".into(),
            provider: None,
            window: None,
        });
    }
    if let Some(m) = r.budget.minutes {
        // Minutes the run worked: time paused at a plan budget does not count.
        let used = (now_ms - r.start_ms - r.paused_ms).max(0) as f64 / 60_000.0;
        out.push(Meter {
            what: "Time".into(),
            unit: "minutes".into(),
            used: Some(used),
            soft: m.soft_value() as f64,
            hard: m.hard as f64,
            reserve: 0.0,
            estimated: false,
            observed_ms: None,
            pause: None,
            limit: "minutes".into(),
            provider: None,
            window: None,
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn track() -> PlanTrack {
        PlanTrack {
            provider: "claude".into(),
            window: "five_hour".into(),
            ..Default::default()
        }
    }

    #[test]
    fn no_reading_at_start_uses_the_first_one_and_counts_the_tokens_before_it() {
        let mut t = track();
        assert_eq!(share(&t, 1000, None), None, "nothing to go on yet");
        observe(&mut t, (40.0, Some(9_000), 10), 1000); // the first reading: 40%, after 1000 tokens
        observe(&mut t, (42.0, Some(9_000), 20), 3000); // 2% for 2000 tokens: 0.001%/token
        let r = rate(&t, None).unwrap();
        assert!((r - 0.001).abs() < 1e-12);
        let (used, est) = share(&t, 3000, Some(r)).unwrap();
        assert!(
            (used - 3.0).abs() < 1e-9 && !est,
            "2% read plus the 1000 tokens before the baseline: {used}"
        );
    }

    #[test]
    fn the_estimate_runs_ahead_of_the_last_reading() {
        let mut t = track();
        observe(&mut t, (10.0, None, 1), 0);
        observe(&mut t, (11.0, None, 2), 1000);
        let (used, est) = share(&t, 5000, rate(&t, None)).unwrap();
        assert!(
            (used - 5.0).abs() < 1e-9 && est,
            "4000 more tokens at 0.001%/token: {used}"
        );
    }

    #[test]
    fn a_window_reset_restarts_the_share() {
        let mut t = track();
        observe(&mut t, (80.0, Some(100), 1), 0);
        observe(&mut t, (90.0, Some(100), 2), 1000);
        observe(&mut t, (3.0, Some(200), 3), 1500);
        let (used, _) = share(&t, 1500, None).unwrap();
        assert!(
            (used - 3.0).abs() < 1e-9,
            "the new window began at 0 and holds 3%: {used}"
        );
        assert_eq!(t.peak, 0.0, "the peak is the caller's to keep");
    }

    #[test]
    fn an_old_reading_is_ignored() {
        let mut t = track();
        observe(&mut t, (10.0, None, 5), 0);
        observe(&mut t, (50.0, None, 4), 10);
        assert_eq!(t.last.unwrap().percent, 10.0);
    }
}
