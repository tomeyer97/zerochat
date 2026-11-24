//! `diagnose` and `parse_reset` on their own; how a classified failure reads on a whole run is
//! tested in projection/tests.rs.

use super::reset::DAY_MS;
use super::*;

fn diagnosis(line: &str) -> Diagnosis {
    diagnose(&[line.to_owned()]).expect("a known cause")
}

#[test]
fn rejected_credentials_are_credentials_with_their_status() {
    let d = diagnosis(
        "Node worker failed: crash after 14.9s: node execution failed: Claude provider failure: \
         Failed to authenticate. API Error: 401 OAuth access token is invalid.",
    );
    assert_eq!((d.kind, d.http), (DiagnosisKind::Credentials, Some(401)));
}

#[test]
fn a_usage_limit_keeps_its_reset_phrase() {
    let d = diagnosis(
        "Claude provider failure: You've hit your session limit · resets 9:30am (UTC); \
         provider process exited with status 1",
    );
    assert_eq!(d.kind, DiagnosisKind::UsageLimit);
    assert_eq!(d.resets.as_deref(), Some("9:30am (UTC)"));
}

#[test]
fn an_unknown_model_is_named() {
    let d = diagnosis("There's an issue with the selected model (claude-x-9). It may not exist");
    assert_eq!(
        (d.kind, d.model.as_deref()),
        (DiagnosisKind::Model, Some("claude-x-9"))
    );
}

#[test]
fn a_403_is_permission_not_credentials() {
    let d = diagnosis(
        "Claude provider failure: Failed to authenticate. API Error: 403 \
         {\"type\":\"error\",\"error\":{\"type\":\"permission_error\",\
         \"message\":\"Your account does not have permission to use this model\"}}",
    );
    assert_eq!(
        d.kind,
        DiagnosisKind::Permission,
        "403 is not a credentials problem"
    );
}

#[test]
fn text_without_a_known_cause_has_no_diagnosis() {
    assert!(diagnose(&["all good".to_owned()]).is_none());
}

#[test]
fn reset_times() {
    let day = DAY_MS;
    // 08:00 UTC on day 10: 9:30am is later the same day, 7am the next day.
    let at = 10 * day + 8 * 3_600_000;
    assert_eq!(
        parse_reset("9:30am (UTC)", at),
        Some(10 * day + 9 * 3_600_000 + 30 * 60_000)
    );
    assert_eq!(parse_reset("7am (UTC)", at), Some(11 * day + 7 * 3_600_000));
    assert_eq!(
        parse_reset("12pm (UTC)", at),
        Some(10 * day + 12 * 3_600_000)
    );
    assert_eq!(parse_reset("9:30am", at), None, "no zone, no guess");
}
