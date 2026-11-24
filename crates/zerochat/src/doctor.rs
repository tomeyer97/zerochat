//! `zerochat doctor`: the banner's checks (`discovery::Setup`) without starting the server; exits
//! non-zero when something is wrong (Zeroshot missing or too old, the state dir unreadable).

use std::path::Path;

use crate::discovery::Setup;

/// What `zerochat doctor` prints, and whether it succeeds.
pub fn report(setup: &Setup, home: &Path, json: bool) -> (String, bool) {
    if json {
        return (
            serde_json::to_string_pretty(setup).expect("the setup serialises") + "\n",
            setup.ok,
        );
    }
    let mut text = format!("zerochat {}\n", setup.version);
    for line in setup.lines(home) {
        text.push_str(&line);
        text.push('\n');
    }
    match setup.problems.len() {
        0 => text.push_str("All good.\n"),
        1 => text.push_str("1 problem: see ✗ above.\n"),
        n => text.push_str(&format!("{n} problems: see ✗ above.\n")),
    }
    (text, setup.ok)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::discovery::{self, PbCheck, RunsCheck, ZeroshotFacts, ZsfCheck};

    fn setup(found: bool, version: &str) -> Setup {
        let zeroshot = discovery::zeroshot_check(ZeroshotFacts {
            program: "/opt/bin/zeroshot".into(),
            found,
            version_output: Some(version.into()),
            // `zeroshot steer` exists from 16.1 on.
            capabilities: if discovery::parse_version(version)
                .is_some_and(|v| v >= discovery::ZEROSHOT_MINIMUM)
            {
                vec!["steer".into()]
            } else {
                vec![]
            },
        });
        let runs = RunsCheck {
            state_dir: "/home/u/.local/state/zeroshot".into(),
            exists: true,
            readable: true,
            count: 3,
        };
        Setup::new(
            zeroshot,
            (runs, None),
            ZsfCheck {
                found: false,
                program: None,
                version: None,
                features: 0,
            },
            PbCheck {
                configured: false,
                root: None,
            },
        )
    }

    #[test]
    fn a_working_setup_passes() {
        let (text, ok) = report(&setup(true, "16.1.0"), Path::new("/home/u"), false);
        assert!(ok);
        assert!(text.ends_with("All good.\n"), "{text}");
        assert!(
            text.contains("  Zeroshot   16.1.0 at /opt/bin/zeroshot   ✓ stop/resume"),
            "{text}"
        );
        assert!(
            text.contains("  Runs       ~/.local/state/zeroshot   3 runs"),
            "{text}"
        );
    }

    #[test]
    fn a_missing_or_old_zeroshot_fails_with_the_fix() {
        for s in [setup(false, ""), setup(true, "11.0.0")] {
            let (text, ok) = report(&s, Path::new("/home/u"), false);
            assert!(!ok);
            assert!(
                text.contains("✗ Zeroshot") && text.contains(discovery::ZEROSHOT_INSTALL),
                "{text}"
            );
            assert!(text.ends_with("1 problem: see ✗ above.\n"), "{text}");
        }
    }

    #[test]
    fn a_missing_zsf_names_how_to_get_it_and_is_not_a_problem() {
        let (text, ok) = report(&setup(true, "16.1.0"), Path::new("/home/u"), false);
        assert!(ok, "zsf is optional");
        let line = text
            .lines()
            .find(|l| l.starts_with("  Features "))
            .expect("a Features line");
        assert!(line.contains("not found → "), "{line}");
        assert!(line.contains(discovery::ZSF_INSTALL), "{line}");
        for how in ["npm package", "`make`", "cargo install --path crates/zsf"] {
            assert!(line.contains(how), "{line} names {how}");
        }
        assert!(
            text.contains(
                "pb (ProgramBench runner) not configured → optional; pb is not public yet"
            ),
            "{text}"
        );
    }

    #[test]
    fn json_is_the_api_answer() {
        let (text, ok) = report(&setup(true, "11.0.0"), Path::new("/home/u"), true);
        assert!(!ok);
        let v: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(v["app"], "zerochat");
        assert_eq!(v["ok"], false);
        assert_eq!(v["problems"][0]["fix"], discovery::ZEROSHOT_INSTALL);
    }
}
