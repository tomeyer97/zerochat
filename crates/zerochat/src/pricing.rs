//! Estimated USD cost from token usage when the provider does not report a cost.
//!
//! First-party Anthropic API list prices per million tokens. Cache writes are priced at the
//! 1-hour TTL (2x input) for Claude Code, which writes 1-hour cache entries, and at the 5-minute
//! TTL (1.25x) for other harnesses; cache reads are 0.1x input unless the model has its own rate. Estimates, not invoices: subscription (OAuth)
//! runs are not billed per token at all.

use serde::Serialize;
use ts_rs::TS;

use crate::harness_name::Harness;

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct Usage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read_input_tokens: u64,
    pub cache_creation_input_tokens: u64,
}

impl Usage {
    pub fn from_json(value: &serde_json::Value) -> Self {
        let n = |key: &str| {
            value
                .get(key)
                .and_then(serde_json::Value::as_u64)
                .unwrap_or(0)
        };
        Self {
            input_tokens: n("inputTokens"),
            output_tokens: n("outputTokens"),
            cache_read_input_tokens: n("cacheReadInputTokens"),
            cache_creation_input_tokens: n("cacheCreationInputTokens"),
        }
    }

    /// Usage as one harness reports it. Copilot's `inputTokens` already contains the cache reads
    /// and writes; Claude Code's does not. `input_tokens` here is always the uncached part.
    pub fn from_harness_json(value: &serde_json::Value, harness: Option<&str>) -> Self {
        let mut usage = Self::from_json(value);
        if Harness::of(harness) == Some(Harness::Copilot) {
            usage.input_tokens = usage
                .input_tokens
                .saturating_sub(usage.cache_read_input_tokens + usage.cache_creation_input_tokens);
        }
        usage
    }

    pub fn add(&mut self, other: &Usage) {
        self.input_tokens += other.input_tokens;
        self.output_tokens += other.output_tokens;
        self.cache_read_input_tokens += other.cache_read_input_tokens;
        self.cache_creation_input_tokens += other.cache_creation_input_tokens;
    }

    pub fn total(&self) -> u64 {
        self.input_tokens
            + self.output_tokens
            + self.cache_read_input_tokens
            + self.cache_creation_input_tokens
    }
}

/// Cache prices as multiples of the input price: a 1-hour cache write (Claude Code writes those),
/// a 5-minute one (other harnesses), and a cache read when the model has no rate of its own.
const CACHE_WRITE_1H: f64 = 2.0;
const CACHE_WRITE_5M: f64 = 1.25;
const CACHE_READ: f64 = 0.1;

/// USD per million tokens. `cache_read` is the model's own cache-read rate, when it has one.
struct Price {
    input: f64,
    output: f64,
    cache_read: Option<f64>,
}

/// How a row of `PRICES` recognises a model id (lower case, `.` spelled `-`).
enum Model {
    /// Any id containing this text, e.g. `"sonnet"` for `claude-sonnet-4-5-20250929`.
    Contains(&'static str),
    /// Exactly this id (the undated `claude-opus-4`, whose name is a prefix of later Opus ids).
    Is(&'static str),
}
use Model::{Contains, Is};

const fn usd(input: f64, output: f64, cache_read: Option<f64>) -> Price {
    Price {
        input,
        output,
        cache_read,
    }
}

/// Anthropic list prices for Claude models (the Fable, Opus, Sonnet and Haiku families). To
/// update, take the per-model input, output and cache-read prices from Anthropic's pricing page
/// (https://www.anthropic.com/pricing). These prices were entered on 2026-11-04.
///
/// The first row with a matching name wins, so a specific version comes before its family:
/// `opus-4-1` before `opus`, `fable-5-1` before `fable`. `mythos` ids are priced like Fable.
const PRICES: &[(&[Model], Price)] = &[
    (
        &[Contains("fable-5-1"), Contains("mythos-5-1")],
        usd(10.0, 50.0, Some(0.25)),
    ),
    (
        &[Contains("fable"), Contains("mythos")],
        usd(10.0, 50.0, None),
    ),
    (&[Contains("opus-5-5")], usd(4.0, 20.0, Some(0.20))),
    (
        &[
            Contains("opus-4-1"),
            Contains("opus-4-0"),
            Is("claude-opus-4"),
        ],
        usd(15.0, 75.0, None),
    ),
    (&[Contains("opus")], usd(5.0, 25.0, None)),
    (&[Contains("sonnet-5")], usd(2.0, 10.0, None)),
    (&[Contains("sonnet")], usd(3.0, 15.0, None)),
    (&[Contains("haiku-4")], usd(1.0, 5.0, None)),
    (&[Contains("haiku-3-5")], usd(0.8, 4.0, None)),
];

/// The price of `model` as any harness spells it: `claude-opus-4-1`, `claude-opus-4.1`
/// (Copilot) or `anthropic.claude-opus-4-1` (Bedrock).
fn price(model: &str) -> Option<&'static Price> {
    let model = model.to_ascii_lowercase();
    let model = model.trim_start_matches("anthropic.").replace('.', "-");
    let matches = |name: &Model| match name {
        Contains(part) => model.contains(part),
        Is(id) => model == *id,
    };
    PRICES
        .iter()
        .find(|(names, _)| names.iter().any(matches))
        .map(|(_, price)| price)
}

/// `None` when the model is unknown (for example a Codex or Copilot model).
pub fn estimate_usd(model: &str, usage: &Usage, harness: Option<&str>) -> Option<f64> {
    let price = price(model)?;
    let cache_write = if Harness::of(harness) == Some(Harness::Claude) {
        CACHE_WRITE_1H
    } else {
        CACHE_WRITE_5M
    };
    let per = |tokens: u64, rate: f64| tokens as f64 * rate / 1_000_000.0;
    Some(
        per(usage.input_tokens, price.input)
            + per(usage.output_tokens, price.output)
            + per(
                usage.cache_read_input_tokens,
                price.cache_read.unwrap_or(price.input * CACHE_READ),
            )
            + per(usage.cache_creation_input_tokens, price.input * cache_write),
    )
}

/// The types of this module the web reads (`web/src/generated/pricing.ts`, api_types.rs), in file order.
#[cfg(test)]
pub(crate) fn ts_module(cfg: &ts_rs::Config) -> crate::api_types::Module {
    crate::api_types::ts_types!(cfg, pricing: Usage)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn haiku_estimate() {
        let usage = Usage {
            input_tokens: 1_000_000,
            output_tokens: 1_000_000,
            cache_read_input_tokens: 1_000_000,
            cache_creation_input_tokens: 1_000_000,
        };
        let usd = estimate_usd("claude-haiku-4-5", &usage, Some("claude")).unwrap();
        assert!((usd - (1.0 + 5.0 + 0.1 + 2.0)).abs() < 1e-9);
    }

    #[test]
    fn copilot_spelling_and_unknown_models() {
        assert!(estimate_usd("claude-haiku-4.5", &Usage::default(), Some("copilot")).is_some());
        assert!(estimate_usd("gpt-5-codex", &Usage::default(), Some("codex")).is_none());
    }

    #[test]
    fn copilot_input_contains_the_cache() {
        // Real Copilot report: input = cache read + cache write + 4 uncached tokens.
        let raw = serde_json::json!({"inputTokens": 45350, "outputTokens": 159, "cacheReadInputTokens": 41428, "cacheCreationInputTokens": 3920});
        let copilot = Usage::from_harness_json(&raw, Some("copilot"));
        assert_eq!(copilot.input_tokens, 2);
        assert_eq!(copilot.total(), 45350 + 159);
        assert_eq!(
            Usage::from_harness_json(&raw, Some("claude")).input_tokens,
            45350
        );
        let usd = estimate_usd("claude-opus-5.5", &copilot, Some("copilot")).unwrap();
        let expected = (2.0 * 4.0 + 159.0 * 20.0 + 41428.0 * 0.20 + 3920.0 * 4.0 * 1.25) / 1e6;
        assert!((usd - expected).abs() < 1e-9);
    }
}
