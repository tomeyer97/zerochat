//! pb.toml's price table (`[pricing."<model>"]`), applied the way pb's own `cost_of` applies it.
//!
//! zerochat has a second pricer, `crate::pricing`, for its own run pages: built-in list prices,
//! an estimate shown with `~`. This one is deliberately separate: a running pb cell's spend must
//! match what pb's runner counts against the run's cost cap, so it uses pb's table and pb's rules.

use std::collections::HashMap;
use std::path::Path;

/// pb's default price of a 1-hour-TTL cache write, as a multiple of the 5-minute one, when
/// pb.toml gives no `cache_write_1h`.
const CACHE_WRITE_1H_MULTIPLIER: f64 = 1.6;

const TOKENS_PER_PRICE_UNIT: f64 = 1_000_000.0;

/// One model's prices from pb.toml, USD per million tokens.
#[derive(Clone, Copy, Debug)]
pub(super) struct Rates {
    input: f64,
    output: f64,
    cache_read: f64,
    cache_write: f64,
    cache_write_1h: f64,
}

/// One model call's tokens, split the way pb prices them.
#[derive(Clone, Copy, Debug, Default)]
pub(super) struct CallTokens {
    pub(super) input: f64,
    pub(super) output: f64,
    pub(super) cache_read: f64,
    /// Cache writes with the default 5-minute TTL.
    pub(super) cache_write_5m: f64,
    pub(super) cache_write_1h: f64,
}

impl Rates {
    /// The call's cost in USD (pb's `cost_of`).
    pub(super) fn cost(&self, tokens: &CallTokens) -> f64 {
        let per_unit = tokens.input * self.input
            + tokens.output * self.output
            + tokens.cache_read * self.cache_read
            + tokens.cache_write_5m * self.cache_write
            + tokens.cache_write_1h * self.cache_write_1h;
        per_unit / TOKENS_PER_PRICE_UNIT
    }
}

/// pb.toml's price table by model. Empty when pb.toml is missing or not TOML; a model missing
/// one of `input`, `output`, `cache_read`, `cache_write` is left out.
pub(super) fn pb_pricing(pb_root: &Path) -> HashMap<String, Rates> {
    let Ok(text) = std::fs::read_to_string(pb_root.join("pb.toml")) else {
        return HashMap::new();
    };
    let Ok(doc) = toml::from_str::<toml::Value>(&text) else {
        return HashMap::new();
    };
    let mut out = HashMap::new();
    for (model, table) in doc
        .get("pricing")
        .and_then(toml::Value::as_table)
        .into_iter()
        .flatten()
    {
        if let Some(rates) = rates_from_toml(table) {
            out.insert(model.clone(), rates);
        }
    }
    out
}

fn rates_from_toml(table: &toml::Value) -> Option<Rates> {
    let number = |key: &str| {
        let value = table.get(key)?;
        value
            .as_float()
            .or_else(|| value.as_integer().map(|i| i as f64))
    };
    let cache_write = number("cache_write")?;
    Some(Rates {
        input: number("input")?,
        output: number("output")?,
        cache_read: number("cache_read")?,
        cache_write,
        cache_write_1h: number("cache_write_1h").unwrap_or(cache_write * CACHE_WRITE_1H_MULTIPLIER),
    })
}

/// pb's `rates_for`: the exact model, else the longest pricing key it starts with
/// (`claude-haiku-4-5-20251001` → `claude-haiku-4-5`).
pub(super) fn rates_for<'a>(model: &str, pricing: &'a HashMap<String, Rates>) -> Option<&'a Rates> {
    if let Some(exact) = pricing.get(model) {
        return Some(exact);
    }
    pricing
        .iter()
        .filter(|(key, _)| model.starts_with(key.as_str()))
        .max_by_key(|(key, _)| key.len())
        .map(|(_, rates)| rates)
}
