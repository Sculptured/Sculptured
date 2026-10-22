//! What a turn cost, and where the figure came from. Claude Code reports its own estimate; for Codex,
//! Sculptured prices the reported tokens with OpenAI's published list prices (below). Both are estimates
//! at API prices: someone on a Claude or ChatGPT plan is not billed these amounts.
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Where a cost figure came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema, TS)]
#[serde(rename_all = "kebab-case")]
#[ts(export)]
pub enum CostSource {
    /// Claude Code's own `costUSD` for the turn.
    ClaudeCode,
    /// Sculptured's price table of OpenAI list prices (`OPENAI_PRICES_CHECKED`).
    OpenaiListPrices,
}

/// An estimated cost in US dollars, with its source.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, schemars::JsonSchema, TS)]
#[ts(export)]
pub struct Cost {
    pub usd: f64,
    pub source: CostSource,
}

/// When the OpenAI prices below were copied from <https://developers.openai.com/api/docs/pricing>.
pub const OPENAI_PRICES_CHECKED: &str = "2026-10-12";

/// Standard tier, short context (under 272K input tokens), US dollars per million tokens:
/// (model, input, cached input, output). Longer contexts cost more, so those turns are underestimated.
const OPENAI_PRICES: &[(&str, f64, f64, f64)] = &[
    ("gpt-6-astra", 10.00, 1.00, 50.00),
    ("gpt-6.1-sol", 2.00, 0.10, 10.00),
    ("gpt-6-sol", 2.00, 0.20, 10.00),
    ("gpt-6-luna", 0.10, 0.01, 0.50),
    ("gpt-5.6-sol", 4.00, 0.40, 20.00),
    ("gpt-5.6-terra", 2.00, 0.20, 12.00),
    ("gpt-5.6-luna", 0.20, 0.02, 1.20),
    ("gpt-5.5", 5.00, 0.50, 30.00),
    ("gpt-5.4", 2.50, 0.25, 15.00),
    ("gpt-5.4-mini", 0.75, 0.075, 4.50),
    ("gpt-5.4-nano", 0.20, 0.02, 1.25),
    ("gpt-5.3-codex", 1.75, 0.175, 14.00),
    ("gpt-5.2", 1.75, 0.175, 14.00),
    ("gpt-5.1", 1.25, 0.125, 10.00),
    ("gpt-5", 1.25, 0.125, 10.00),
    ("gpt-5-mini", 0.25, 0.025, 2.00),
    ("gpt-5-nano", 0.05, 0.005, 0.40),
];

/// The list-price cost of a Codex turn (input not counting cached input), or None for a model not in the table.
#[must_use]
pub fn openai_cost(model: &str, input: u64, cached: u64, output: u64) -> Option<Cost> {
    let (_, i, c, o) = OPENAI_PRICES.iter().find(|(m, ..)| *m == model)?;
    let usd = (input as f64 * i + cached as f64 * c + output as f64 * o) / 1_000_000.0;
    Some(Cost {
        usd,
        source: CostSource::OpenaiListPrices,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_codex_turn_is_priced_by_its_model_or_not_at_all() {
        let c = openai_cost("gpt-6-astra", 1_000_000, 1_000_000, 1_000_000).unwrap();
        assert!((c.usd - 61.0).abs() < 1e-9 && c.source == CostSource::OpenaiListPrices);
        assert!(
            (openai_cost("gpt-5-mini", 2_000_000, 0, 500_000)
                .unwrap()
                .usd
                - 1.5)
                .abs()
                < 1e-9
        );
        assert_eq!(openai_cost("some-future-model", 1, 1, 1), None);
    }
}
