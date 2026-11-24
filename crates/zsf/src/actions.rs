//! What a person can do to a feature or a slice right now: the one table both the writer's
//! callers (the CLI, zerochat) offer and refuse actions by.
//!
//! [`slice_actions`] is the set of slice actions that [`crate::engine::Engine::apply`] accepts
//! *and* that make sense to offer: `apply` itself is more lenient (it answers "already approved"
//! to a second approve, skips a slice that is already skipped), so a UI built on this table
//! never sends a request that only repeats what is done. Every rule here is a predicate on
//! [`SliceState`] or [`State`] that `apply` checks too.

use serde::Serialize;

use crate::model::{SlicePlan, SliceState, State};

/// An action on one slice, as the daemon request and the UI name it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SliceAction {
    Approve,
    Retry,
    Accept,
    Skip,
    Edit,
}

impl SliceAction {
    /// Every slice action, in the order they are offered.
    pub const ALL: [SliceAction; 5] = [
        SliceAction::Approve,
        SliceAction::Retry,
        SliceAction::Accept,
        SliceAction::Skip,
        SliceAction::Edit,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            SliceAction::Approve => "approve",
            SliceAction::Retry => "retry",
            SliceAction::Accept => "accept",
            SliceAction::Skip => "skip",
            SliceAction::Edit => "edit",
        }
    }

    /// The action named `name` (its [`Self::as_str`]), if there is one.
    pub fn parse(name: &str) -> Option<SliceAction> {
        Self::ALL.into_iter().find(|a| a.as_str() == name)
    }
}

/// An action on the whole feature.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FeatureAction {
    Start,
    Stop,
    Unpause,
    MaxParallel,
}

impl FeatureAction {
    /// Every feature action, in the order they are offered.
    pub const ALL: [FeatureAction; 4] = [
        FeatureAction::Start,
        FeatureAction::Stop,
        FeatureAction::Unpause,
        FeatureAction::MaxParallel,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            FeatureAction::Start => "start",
            FeatureAction::Stop => "stop",
            FeatureAction::Unpause => "unpause",
            FeatureAction::MaxParallel => "max_parallel",
        }
    }

    /// The action named `name` (its [`Self::as_str`]), if there is one.
    pub fn parse(name: &str) -> Option<FeatureAction> {
        Self::ALL.into_iter().find(|a| a.as_str() == name)
    }
}

/// The slice actions to offer now. `worktree_changed`: the stopped slice's worktree holds work
/// to merge (`accept` needs some; reading the worktree is the caller's job). A slice waiting for
/// review offers approve (check and merge), retry (request changes, with notes) and skip.
pub fn slice_actions(plan: &SlicePlan, s: &SliceState, worktree_changed: bool) -> Vec<SliceAction> {
    let mut out = Vec::new();
    if s.awaiting_approval(plan.checkpoint) {
        out.push(SliceAction::Approve);
    }
    if s.is_stopped() || s.waiting_for_review() {
        out.push(SliceAction::Retry);
    }
    if s.is_stopped() && worktree_changed && s.worktree_on_disk().is_some() {
        out.push(SliceAction::Accept);
    }
    if s.is_idle() && !s.status.is_finished() {
        out.push(SliceAction::Skip);
    }
    if s.is_editable() {
        out.push(SliceAction::Edit);
    }
    out
}

/// What the caller knows about a feature beyond its state.
#[derive(Clone, Copy, Debug, Default)]
pub struct FeatureFacts {
    /// Every slice is finished (nothing left to schedule).
    pub all_finished: bool,
    /// The feature's daemon is running.
    pub daemon_alive: bool,
    /// A `zsf` program is at hand to start the daemon with.
    pub can_spawn: bool,
}

/// The feature actions to offer now.
pub fn feature_actions(state: &State, facts: FeatureFacts) -> Vec<FeatureAction> {
    let FeatureFacts {
        all_finished,
        daemon_alive,
        can_spawn,
    } = facts;
    let mut out = Vec::new();
    if can_spawn && !all_finished && (!state.active || !daemon_alive) {
        out.push(FeatureAction::Start);
    }
    if state.active || daemon_alive {
        out.push(FeatureAction::Stop);
    }
    if state.paused.is_some() {
        out.push(FeatureAction::Unpause);
    }
    if !all_finished {
        out.push(FeatureAction::MaxParallel);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_slice_waiting_for_review_offers_approve_retry_and_skip() {
        use crate::model::SliceStatus;
        let plan = SlicePlan {
            checkpoint: true,
            ..Default::default()
        };
        let waiting = SliceState {
            status: SliceStatus::WaitingForReview,
            ..Default::default()
        };
        assert_eq!(
            slice_actions(&plan, &waiting, true),
            [SliceAction::Approve, SliceAction::Retry, SliceAction::Skip]
        );
        // An older zsf's merged, unapproved checkpoint: approve releases it.
        let merged = SliceState {
            status: SliceStatus::Merged,
            ..Default::default()
        };
        assert_eq!(slice_actions(&plan, &merged, false), [SliceAction::Approve]);
    }

    #[test]
    fn names_round_trip() {
        for a in SliceAction::ALL {
            assert_eq!(SliceAction::parse(a.as_str()), Some(a));
            assert_eq!(serde_json::to_value(a).unwrap(), a.as_str());
        }
        for a in FeatureAction::ALL {
            assert_eq!(FeatureAction::parse(a.as_str()), Some(a));
            assert_eq!(serde_json::to_value(a).unwrap(), a.as_str());
        }
        assert_eq!(SliceAction::parse("start"), None);
        assert_eq!(FeatureAction::parse("approve"), None);
    }
}
