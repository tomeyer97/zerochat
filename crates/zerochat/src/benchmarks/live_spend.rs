//! A running pb run's spend so far, read from the shim's per-call `live.json` and priced with
//! pb.toml (`pb_pricing`) the way pb's runner prices it.

use std::collections::HashMap;
use std::path::Path;

use serde_json::Value;

use super::lists::push_unique;
use super::pb_pricing::{CallTokens, Rates, rates_for};
use crate::paths;

pub(super) struct LiveSpend {
    pub(super) cost_usd: f64,
    /// Models pb.toml has no price for; their tokens are left out of `cost_usd`.
    pub(super) unpriced: Vec<String>,
}

/// The run's spend so far: every call's `live.json` (`{"messages": {id: [model, usage,
/// parent]}}`, deduplicated by message id by the shim), priced like pb's `cost_of`. `None` when
/// the run has no calls yet.
pub(super) fn live_spend(
    pb_root: &Path,
    run_id: &str,
    pricing: &HashMap<String, Rates>,
) -> Option<LiveSpend> {
    let calls = std::fs::read_dir(paths::pb_run_calls_dir(pb_root, run_id)).ok()?;
    let mut spend = LiveSpend {
        cost_usd: 0.0,
        unpriced: Vec::new(),
    };
    let mut any_call = false;
    for call in calls.flatten() {
        let Ok(text) = std::fs::read_to_string(call.path().join("live.json")) else {
            continue;
        };
        let Ok(live) = serde_json::from_str::<Value>(&text) else {
            continue;
        };
        any_call = true;
        let messages = live
            .get("messages")
            .and_then(Value::as_object)
            .into_iter()
            .flat_map(|m| m.values());
        for message in messages {
            let (Some(model), Some(usage)) =
                (message.get(0).and_then(Value::as_str), message.get(1))
            else {
                continue;
            };
            if model.is_empty() || model == "<synthetic>" {
                continue;
            }
            match rates_for(model, pricing) {
                Some(rates) => spend.cost_usd += rates.cost(&call_tokens(usage)),
                None => push_unique(&mut spend.unpriced, model),
            }
        }
    }
    any_call.then_some(spend)
}

/// A message's Anthropic `usage` object as pb prices it: its 1-hour cache writes
/// (`cache_creation.ephemeral_1h_input_tokens`) split out of `cache_creation_input_tokens`.
fn call_tokens(usage: &Value) -> CallTokens {
    let tokens = |key: &str| usage.get(key).and_then(Value::as_f64).unwrap_or(0.0);
    let cache_write = tokens("cache_creation_input_tokens");
    let cache_write_1h = usage
        .pointer("/cache_creation/ephemeral_1h_input_tokens")
        .and_then(Value::as_f64)
        .unwrap_or(0.0)
        .min(cache_write);
    CallTokens {
        input: tokens("input_tokens"),
        output: tokens("output_tokens"),
        cache_read: tokens("cache_read_input_tokens"),
        cache_write_5m: cache_write - cache_write_1h,
        cache_write_1h,
    }
}
