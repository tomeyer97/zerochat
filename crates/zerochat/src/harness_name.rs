//! The agent CLIs ("harnesses") Zeroshot runs nodes with, by the names its runtime files and
//! zerochat's API use.

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Harness {
    Claude,
    Codex,
    Copilot,
}

impl Harness {
    pub const ALL: [Harness; 3] = [Harness::Claude, Harness::Codex, Harness::Copilot];

    pub fn name(self) -> &'static str {
        match self {
            Harness::Claude => "claude",
            Harness::Codex => "codex",
            Harness::Copilot => "copilot",
        }
    }

    pub fn from_name(name: &str) -> Option<Harness> {
        Harness::ALL.into_iter().find(|h| h.name() == name)
    }

    /// The harness whose behaviour (prices, session logs) zerochat applies to a node, by the name
    /// its runtime gave (`None`: Zeroshot's default, Claude Code). `None` for a harness zerochat
    /// does not know. Never for what a person reads: an unknown or missing harness is not Claude
    /// there (`label`).
    pub fn of(name: Option<&str>) -> Option<Harness> {
        name.map_or(Some(Harness::Claude), Harness::from_name)
    }
}

/// A harness as a person reads it: its id as the runtime gave it (`pi`, `hermes`, …), or
/// `unknown` without one. Never a guess.
pub fn label(name: Option<&str>) -> &str {
    name.filter(|n| !n.is_empty()).unwrap_or("unknown")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_missing_or_unknown_harness_is_never_shown_as_claude() {
        assert_eq!(label(None), "unknown");
        assert_eq!(label(Some("")), "unknown");
        assert_eq!(label(Some("hermes")), "hermes");
        assert_eq!(label(Some("codex")), "codex");
        assert_eq!(Harness::from_name("hermes"), None);
    }
}
