//! Golden/snapshot tests: pin every pure transform and every JSON API response of zerochat byte
//! for byte (see tests/GOLDENS.md for the inventory and how to update them).

pub mod golden;
pub mod world;
mod zsf_fixture;

mod api;
mod benchmarks_goldens;
mod features_goldens;
mod projection_goldens;
mod pure;
mod transcript_goldens;

use serde_json::Value;

/// Compares `value` (normalised: the fixture root becomes `<root>`) with
/// `testdata/golden/<name>.json`.
pub fn check(name: &str, value: &Value) {
    let value = world::normalize(value.clone());
    golden::check_json_in(&golden::golden_dir(world::MANIFEST), name, &value);
}

pub fn check_serialize<T: serde::Serialize>(name: &str, value: &T) {
    check(name, &serde_json::to_value(value).unwrap());
}

/// Goldens whose scenario is fork-only (or official 16.1) are named so a migration finds them:
/// `fork-only/…` and `zeroshot-16/…` path segments.
pub fn family_prefix(tags: &[String]) -> &'static str {
    if tags.iter().any(|t| t == "zeroshot16") {
        "zeroshot-16/"
    } else if tags.iter().any(|t| t == "fork-only" || t == "fork") {
        "fork-only/"
    } else {
        ""
    }
}
