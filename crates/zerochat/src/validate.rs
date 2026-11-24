//! Safe ids and arguments: every rule for a string from a request (or from a file another tool
//! wrote) that zerochat joins onto a path or passes on a command line. They live together so
//! the rules can be compared side by side.
//!
//! Two dangers decide the rules: a value that starts with `-` becomes an option of the CLI that
//! receives it, and a value with `/`, `\` or `..` escapes the directory it is joined onto.

/// A command-line argument: non-empty, at most `max_len` bytes, no leading `-`, and only ASCII
/// letters, digits and `extra_chars`.
pub struct Token {
    pub max_len: usize,
    pub extra_chars: &'static str,
}

impl Token {
    pub fn accepts(&self, value: &str) -> bool {
        !value.is_empty()
            && value.len() <= self.max_len
            && !value.starts_with('-')
            && value
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || self.extra_chars.contains(c))
    }
}

/// Checkpoint ids and history cursors: short opaque ids.
pub const OPAQUE_ID: Token = Token {
    max_len: 128,
    extra_chars: "-_.:",
};

/// Model, effort and harness names in a runtime (`claude-haiku-4-5`,
/// `us.anthropic.claude-opus-4-1`, `gpt-5-codex`): they reach the provider CLI's argv.
pub const RUNTIME_NAME: Token = Token {
    max_len: 200,
    extra_chars: "-_.:/@[]",
};

/// Git revisions for a workspace's git views (`HEAD~1`, `main^`, a commit hash).
pub const GIT_REVISION: Token = Token {
    max_len: 200,
    extra_chars: "/._-^~",
};

/// A Zeroshot run id: a UUID (`01a1a5e8-c91b-7073-b3e9-fdbd462600af`).
pub fn is_run_id(id: &str) -> bool {
    const GROUP_LENGTHS: [usize; 5] = [8, 4, 4, 4, 12];
    let groups: Vec<&str> = id.split('-').collect();
    groups.len() == GROUP_LENGTHS.len()
        && groups
            .iter()
            .zip(GROUP_LENGTHS)
            .all(|(group, len)| group.len() == len && group.chars().all(|c| c.is_ascii_hexdigit()))
}

/// A run id as the feature pages accept it: hex digits and `-` only, at most 64 characters.
/// Looser than `is_run_id` (any UUID passes), still safe to join onto a path.
pub fn is_hex_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 64 && id.chars().all(|c| c.is_ascii_hexdigit() || c == '-')
}

/// One path segment that is not hidden: non-empty, no `/` or `\`, and no leading `.` (so
/// neither `.` nor `..`). For names pb chose (batch, run and harness variant directories).
pub fn is_plain_name(name: &str) -> bool {
    !name.is_empty() && !name.starts_with('.') && !name.contains(['/', '\\'])
}

/// One path segment: non-empty, no `/` or `\`, and not `.` or `..` (hidden names are fine).
pub fn is_path_segment(name: &str) -> bool {
    !name.is_empty() && !name.contains(['/', '\\']) && name != "." && name != ".."
}

/// A path inside a workspace, as git and the file views take it: relative (no leading `/`),
/// never climbing out (no `..` component), not an option (no leading `-`), no NUL byte.
pub fn is_relative_path(path: &str) -> bool {
    !path.is_empty()
        && !path.starts_with('/')
        && !path.starts_with('-')
        && !path.contains('\0')
        && !path.split('/').any(|part| part == "..")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every rule, on the inputs that tell them apart. Each row is the decision the call site
    /// made with its own copy of the rule before they moved here; changing one is a behaviour
    /// change for that call site.
    #[test]
    fn every_call_site_keeps_its_decisions() {
        let long = |n: usize| "a".repeat(n);
        let uuid = "01a1a5e8-c91b-7073-b3e9-fdbd462600af";
        /// (call site, the check, the value, accepted).
        type Row = (&'static str, fn(&str) -> bool, String, bool);
        #[rustfmt::skip]
        let rows: Vec<Row> = vec![
            // server: checkpoint ids and the checkpoint cursor (`valid_id`).
            ("opaque id", |v| OPAQUE_ID.accepts(v), "cp-1_a.b:c".into(), true),
            ("opaque id", |v| OPAQUE_ID.accepts(v), long(128), true),
            ("opaque id", |v| OPAQUE_ID.accepts(v), long(129), false),
            ("opaque id", |v| OPAQUE_ID.accepts(v), "".into(), false),
            ("opaque id", |v| OPAQUE_ID.accepts(v), "-x".into(), false),
            ("opaque id", |v| OPAQUE_ID.accepts(v), "a/b".into(), false),
            ("opaque id", |v| OPAQUE_ID.accepts(v), "a b".into(), false),
            // server: replacement runtime names (`runtime_problem`).
            ("runtime name", |v| RUNTIME_NAME.accepts(v), "us.anthropic.claude-opus-4-1".into(), true),
            ("runtime name", |v| RUNTIME_NAME.accepts(v), "org/model@v1[x]:y_z".into(), true),
            ("runtime name", |v| RUNTIME_NAME.accepts(v), long(200), true),
            ("runtime name", |v| RUNTIME_NAME.accepts(v), long(201), false),
            ("runtime name", |v| RUNTIME_NAME.accepts(v), "--model=x".into(), false),
            ("runtime name", |v| RUNTIME_NAME.accepts(v), "a b".into(), false),
            ("runtime name", |v| RUNTIME_NAME.accepts(v), "a~1".into(), false),
            // actions: git revisions (`safe_rev`).
            ("git revision", |v| GIT_REVISION.accepts(v), "HEAD~1".into(), true),
            ("git revision", |v| GIT_REVISION.accepts(v), "origin/main^".into(), true),
            ("git revision", |v| GIT_REVISION.accepts(v), long(200), true),
            ("git revision", |v| GIT_REVISION.accepts(v), long(201), false),
            ("git revision", |v| GIT_REVISION.accepts(v), "--output=/tmp/x".into(), false),
            ("git revision", |v| GIT_REVISION.accepts(v), "a:b".into(), false),
            ("git revision", |v| GIT_REVISION.accepts(v), "".into(), false),
            // server: run ids in every run route (`valid_run_id`).
            ("run id", is_run_id, uuid.into(), true),
            ("run id", is_run_id, uuid.to_uppercase(), true),
            ("run id", is_run_id, "01a1a5e8-c91b-7073-b3e9".into(), false),
            ("run id", is_run_id, "-1a1a5e8-c91b-7073-b3e9-fdbd462600af".into(), false),
            ("run id", is_run_id, "01a1a5e8-c91b-7073-b3e9-fdbd462600ag".into(), false),
            // features: run ids of a slice's runs (`valid_run_id` there).
            ("hex id", is_hex_id, uuid.into(), true),
            ("hex id", is_hex_id, "abc".into(), true),
            ("hex id", is_hex_id, "-".into(), true),
            ("hex id", is_hex_id, "f".repeat(64), true),
            ("hex id", is_hex_id, "f".repeat(65), false),
            ("hex id", is_hex_id, "".into(), false),
            ("hex id", is_hex_id, "../x".into(), false),
            ("hex id", is_hex_id, "g".into(), false),
            // benchmarks and pb_exec: pb's batch, run and variant names (`plain_id`).
            ("plain name", is_plain_name, "b-1 x".into(), true),
            ("plain name", is_plain_name, "a..b".into(), true),
            ("plain name", is_plain_name, "".into(), false),
            ("plain name", is_plain_name, ".".into(), false),
            ("plain name", is_plain_name, "..".into(), false),
            ("plain name", is_plain_name, ".hidden".into(), false),
            ("plain name", is_plain_name, "a/b".into(), false),
            ("plain name", is_plain_name, "a\\b".into(), false),
            // harness: pb run ids (`safe_component`).
            ("path segment", is_path_segment, "r1".into(), true),
            ("path segment", is_path_segment, ".hidden".into(), true),
            ("path segment", is_path_segment, "".into(), false),
            ("path segment", is_path_segment, ".".into(), false),
            ("path segment", is_path_segment, "..".into(), false),
            ("path segment", is_path_segment, "a/b".into(), false),
            ("path segment", is_path_segment, "a\\b".into(), false),
            // workspace and actions: file paths (`safe_relative`).
            ("relative path", is_relative_path, "src/main.rs".into(), true),
            ("relative path", is_relative_path, "a/./b..c".into(), true),
            ("relative path", is_relative_path, "".into(), false),
            ("relative path", is_relative_path, "/etc/passwd".into(), false),
            ("relative path", is_relative_path, "../etc/passwd".into(), false),
            ("relative path", is_relative_path, "a/../b".into(), false),
            ("relative path", is_relative_path, "-rf".into(), false),
            ("relative path", is_relative_path, "a\0b".into(), false),
        ];
        for (rule, accepts, value, expected) in rows {
            assert_eq!(accepts(&value), expected, "{rule}: {value:?}");
        }
    }
}
