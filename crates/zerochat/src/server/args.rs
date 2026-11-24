//! Ids from the URL and the zeroshot CLI arguments built from them: checked before they reach
//! the CLI's argv.

use crate::actions::push_target;
use crate::validate;

use super::error::ApiError;

/// A run id is a UUID (`01a1a5e8-c91b-7073-b3e9-fdbd462600af`); anything else never reaches the
/// CLI's argv (an id starting with `-` would be parsed as an option).
pub fn valid_run_id(id: &str) -> Result<&str, ApiError> {
    if !validate::is_run_id(id) {
        return Err(ApiError::bad_request(
            "invalid_run_id",
            "That is not a run id.",
        ));
    }
    Ok(id)
}

/// Checkpoint ids and similar opaque ids (`validate::OPAQUE_ID`).
pub(crate) fn valid_id<'a>(id: &'a str, what: &str) -> Result<&'a str, ApiError> {
    if !validate::OPAQUE_ID.accepts(id) {
        return Err(ApiError::bad_request(
            "invalid_id",
            format!("That is not a valid {what}."),
        ));
    }
    Ok(id)
}

/// `zeroshot <command…> <options…> [--target NAME] -- <positionals…>`: options are passed as
/// `--flag=value` and positionals after `--`, so no value can turn into a flag.
pub(crate) fn cli_args(
    command: &[&str],
    options: Vec<String>,
    target: Option<&str>,
    positionals: &[&str],
) -> Vec<String> {
    let mut args: Vec<String> = command.iter().map(|c| (*c).to_owned()).collect();
    args.extend(options);
    push_target(&mut args, target);
    args.push("--".into());
    args.extend(positionals.iter().map(|p| (*p).to_owned()));
    args
}

pub(crate) fn opt(flag: &str, value: &str) -> String {
    format!("{flag}={value}")
}
