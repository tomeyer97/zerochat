//! zsf ("Zeroshot features") drives a feature cut into slices through Zeroshot: one run per
//! slice, at most N at once, each finished slice squashed onto the feature branch, a person
//! asked at checkpoints (which wait unmerged for review) and failures.
//!
//! # Library surface
//!
//! Other tools (zerochat) use only these modules; everything else is the `zsf` program's
//! internals and may change without notice.
//!
//! - Read a feature: [`store::Feature::open`] (state root [`store::state_root`], names from
//!   [`store::list_features`]), then [`store::Feature::load_plan`],
//!   [`store::Feature::load_state`], [`store::Feature::events`] or [`status::report`]. The
//!   files' types are in [`model`].
//! - Read the scheduling rules instead of restating them: [`model::State::dep_done`],
//!   [`model::State::waits_for`], [`model::State::awaiting_approval`],
//!   [`model::SliceStatus::is_active`], [`model::SliceState::display`].
//! - Offer actions by [`actions::slice_actions`] and [`actions::feature_actions`] (the same rules
//!   the writer checks), named by [`SliceAction`] and [`FeatureAction`].
//! - Change a feature only through [`daemon::submit`] (queued to the daemon, the one writer);
//!   run or stop the daemon with [`daemon::spawn_with`] and [`daemon::stop`].

pub mod actions;
pub mod daemon;
pub mod model;
pub mod status;
pub mod store;

pub use actions::{FeatureAction, FeatureFacts, SliceAction};

// Internals: public only for the `zsf` binary (src/main.rs) and the crate's integration tests.
#[doc(hidden)]
pub mod engine;
// `git` would be crate-private, but it still has a helper nothing calls (a warning there).
#[doc(hidden)]
pub mod git;
#[doc(hidden)]
pub mod hooks;
#[doc(hidden)]
pub mod import;
#[doc(hidden)]
pub mod setup;
#[doc(hidden)]
pub mod time;
#[doc(hidden)]
pub mod zeroshot;

// Internals no caller outside the library uses.
pub(crate) mod jobs;
pub(crate) mod sys;
