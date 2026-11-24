use super::actions::dependency_loop;
use super::git::{DiffOutput, git_out, parse_log, parse_numstat, worktree_against_branch};
use super::*;
use zsf::model::{Plan, SliceStatus};

fn ctx_parts(root: &Path) -> (Roots, RunFactsCache) {
    (
        vec![("local".to_owned(), root.join("zs"))],
        RunFactsCache::default(),
    )
}

#[test]
fn list_and_detail_derive_waiting_and_actions() {
    let tmp = tempfile::tempdir().unwrap();
    fixture::build(tmp.path());
    let (roots, runs) = ctx_parts(tmp.path());
    let ctx = Ctx {
        root: tmp.path(),
        roots: &roots,
        runs: &runs,
        zsf_bin: None,
        now: 10_000,
    };
    let rows = list(&ctx);
    assert_eq!(rows.len(), 1);
    let row = &rows[0];
    assert_eq!((row.merged, row.total), (1, 4));
    assert_eq!(
        (row.approve.clone(), row.failed.clone()),
        (vec!["01".to_owned()], vec!["03".to_owned()])
    );
    assert!(row.running.is_empty() && row.daemon_pid.is_none());
    assert!(row.needs_you && row.started && row.finished_at.is_none());

    let d = detail(&ctx, "calc").unwrap().unwrap();
    // No zsf program to run a daemon: start is not offered; nothing is active to stop.
    assert_eq!(d.actions, ["max_parallel"]);
    let by_id = |id: &str| d.slices.iter().find(|s| s.id == id).unwrap().clone();
    let s01 = by_id("01");
    assert_eq!(
        (s01.status, s01.actions.clone()),
        ("waiting_for_approval", vec!["approve"])
    );
    let s02 = by_id("02");
    assert_eq!(
        (s02.status, s02.actions.clone()),
        ("pending", vec!["skip", "edit"])
    );
    assert_eq!(s02.waits_for, ["01"]);
    assert_eq!(s02.approval_of, ["01"]);
    let s03 = by_id("03");
    // Its worktree holds nothing to merge (not even a git checkout here): no Accept.
    assert_eq!(
        (s03.status, s03.actions.clone()),
        ("failed", vec!["retry", "skip"])
    );
    assert_eq!(s03.attempts.len(), 2);
    assert!(s03.attempts.iter().all(|a| a.has_task));
    assert_eq!(s03.artifacts, ["gate/out.txt"]);
    assert_eq!(s03.spec.as_deref(), Some("# 03\nDo 03.\n"));
    assert_eq!(s03.duration_ms, Some(1_000));
    let s04 = by_id("04");
    assert_eq!(s04.waits_for, ["03"]);
    assert!(s04.approval_of.is_empty());
    assert!(detail(&ctx, "nope").unwrap().is_none());
}

#[test]
fn task_diff_shows_the_notes_as_added_lines() {
    let tmp = tempfile::tempdir().unwrap();
    fixture::build(tmp.path());
    let t = task(tmp.path(), "calc", "03", 2).unwrap().unwrap();
    assert_eq!(t.previous_attempt, Some(1));
    let diff = t.diff.unwrap();
    assert!(diff.contains("+Use the existing parser."), "{diff}");
    assert!(diff.contains("+## Notes from the developer"), "{diff}");
    assert!(!diff.contains("\n-"), "{diff}");
    assert!(
        task(tmp.path(), "calc", "03", 1)
            .unwrap()
            .unwrap()
            .diff
            .is_none()
    );
    assert!(task(tmp.path(), "calc", "03", 9).unwrap().is_none());
    assert!(task(tmp.path(), "calc", "../x", 1).unwrap().is_none());
}

#[test]
fn artifacts_cannot_leave_their_directory() {
    let tmp = tempfile::tempdir().unwrap();
    fixture::build(tmp.path());
    assert_eq!(
        artifact(tmp.path(), "calc", "03", "gate/out.txt")
            .unwrap()
            .as_deref(),
        Some("gate: missing NOTES.md\n")
    );
    assert!(
        artifact(tmp.path(), "calc", "03", "../../state.json")
            .unwrap()
            .is_none()
    );
    assert!(
        artifact(tmp.path(), "calc", "03", "/etc/passwd")
            .unwrap()
            .is_none()
    );
}

#[test]
fn events_filter_by_slice() {
    let tmp = tempfile::tempdir().unwrap();
    fixture::build(tmp.path());
    let (roots, _) = ctx_parts(tmp.path());
    assert_eq!(
        events(tmp.path(), &roots, "calc", None, 100)
            .unwrap()
            .unwrap()
            .len(),
        4
    );
    let only = events(tmp.path(), &roots, "calc", Some("01"), 100)
        .unwrap()
        .unwrap();
    assert_eq!(only.len(), 2);
    assert_eq!(
        events(tmp.path(), &roots, "calc", None, 1)
            .unwrap()
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn actions_outside_the_list_are_refused_before_zsf() {
    let tmp = tempfile::tempdir().unwrap();
    fixture::build(tmp.path());
    let body = |action: &str| ActionBody {
        action: action.into(),
        ..Default::default()
    };
    #[derive(Debug, PartialEq)]
    enum Expect {
        Refused,
        NotFound,
        Bad,
    }
    let kind = |result: Result<ActOutcome, ActError>| match result {
        Err(ActError::Refused(_)) => Some(Expect::Refused),
        Err(ActError::NotFound(_)) => Some(Expect::NotFound),
        Err(ActError::Bad(_)) => Some(Expect::Bad),
        _ => None,
    };
    // (feature, slice, action, expected error); an unknown action is a bad request, not a
    // refusal in this state.
    let cases = [
        ("calc", Some("02"), "approve", Expect::Refused),
        ("calc", Some("04"), "accept", Expect::Refused),
        ("calc", None, "start", Expect::Refused),
        ("calc", None, "unpause", Expect::Refused),
        ("calc", Some("99"), "skip", Expect::NotFound),
        ("nope", None, "stop", Expect::NotFound),
        ("calc", Some("03"), "explode", Expect::Bad),
    ];
    for (feature, slice, action, expect) in cases {
        let got = kind(act(tmp.path(), None, feature, slice, body(action)));
        assert_eq!(got, Some(expect), "{feature} {slice:?} {action}");
    }
    let Err(ActError::Refused(why)) = act(tmp.path(), None, "calc", Some("01"), body("retry"))
    else {
        panic!()
    };
    assert_eq!(why, "Can't retry 01: it is waiting for approval now.");
    let Err(ActError::Refused(why)) = act(tmp.path(), None, "calc", Some("03"), body("accept"))
    else {
        panic!()
    };
    assert_eq!(
        why,
        "Can't accept 03: its worktree has no changes to merge."
    );
    // Edits that would make a loop are refused before zsf sees them.
    let deps = |d: &[&str]| ActionBody {
        action: "edit".into(),
        deps: Some(d.iter().map(|s| s.to_string()).collect()),
        ..Default::default()
    };
    let Err(ActError::Bad(why)) = act(tmp.path(), None, "calc", Some("02"), deps(&["02"])) else {
        panic!()
    };
    assert_eq!(why, "02 can't come after itself");
    assert!(dependency_loop(&Plan { slices: vec![] }, "01").is_none());
}

#[test]
fn approve_edit_skip_and_max_parallel_go_through_zsf() {
    let tmp = tempfile::tempdir().unwrap();
    let feature = fixture::build(tmp.path());
    let body = |action: &str| ActionBody {
        action: action.into(),
        ..Default::default()
    };
    // No daemon: zsf applies the request in this process, as the CLI would.
    let out = act(tmp.path(), None, "calc", Some("01"), body("approve")).unwrap();
    assert!(out.ok && out.message.contains("approved"), "{out:?}");
    assert!(feature.load_state().unwrap().slices["01"].approved);

    let edit = ActionBody {
        action: "edit".into(),
        title: Some("Renamed".into()),
        deps: Some(vec!["03".into()]),
        spec: Some("# 02\nNew spec.".into()),
        ..Default::default()
    };
    act(tmp.path(), None, "calc", Some("02"), edit).unwrap();
    let plan = feature.load_plan().unwrap();
    let p = plan.get("02").unwrap();
    assert_eq!(
        (p.title.as_str(), p.deps.clone()),
        ("Renamed", vec!["03".to_owned()])
    );
    assert_eq!(
        std::fs::read_to_string(feature.spec_path(&p.spec)).unwrap(),
        "# 02\nNew spec.\n"
    );
    // Unchanged values are not an edit.
    let same = ActionBody {
        action: "edit".into(),
        title: Some("Renamed".into()),
        ..Default::default()
    };
    assert!(matches!(
        act(tmp.path(), None, "calc", Some("02"), same),
        Err(ActError::Bad(_))
    ));
    let unknown = ActionBody {
        action: "edit".into(),
        deps: Some(vec!["77".into()]),
        ..Default::default()
    };
    assert!(matches!(
        act(tmp.path(), None, "calc", Some("02"), unknown),
        Err(ActError::Bad(_))
    ));

    act(tmp.path(), None, "calc", Some("02"), body("skip")).unwrap();
    assert_eq!(
        feature.load_state().unwrap().slices["02"].status,
        SliceStatus::Skipped
    );

    let mp = ActionBody {
        action: "max_parallel".into(),
        max_parallel: Some(3),
        ..Default::default()
    };
    act(tmp.path(), None, "calc", None, mp).unwrap();
    assert_eq!(
        Feature::open(tmp.path(), "calc")
            .unwrap()
            .config
            .limits
            .max_parallel,
        3
    );
    let zero = ActionBody {
        action: "max_parallel".into(),
        max_parallel: Some(0),
        ..Default::default()
    };
    assert!(matches!(
        act(tmp.path(), None, "calc", None, zero),
        Err(ActError::Bad(_))
    ));
}

#[test]
fn runs_are_indexed_to_their_slice() {
    let tmp = tempfile::tempdir().unwrap();
    fixture::build(tmp.path());
    let index = SliceIndex::build(tmp.path());
    let run = &index.runs[fixture::RUN_03_A2];
    assert_eq!(
        context_line(run),
        "Slice 03 of calc · attempt 2 of 2 · gate failed, not merged"
    );
    let first = &index.runs["01a1ab83-0000-7000-8000-000000000031"];
    assert_eq!(
        context_line(first),
        "Slice 03 of calc · attempt 1 of 2 · gate failed, not merged, retried as attempt 2"
    );
    assert_eq!(context_label(first), "calc 03: Slice 03 · #1 gate failed");
    let merged = &index.runs["01a1ab83-0000-7000-8000-000000000001"];
    assert_eq!(
        context_line(merged),
        "Slice 01 of calc · attempt 1 · merged (abc123)"
    );
    assert_eq!(context_label(merged), "calc 01: Slice 01");
}

/// Runs git in `dir` (through the reader's own wrapper), panicking on failure; trimmed stdout.
fn sh(dir: &Path, args: &[&str]) -> String {
    git_out(dir, args, None).unwrap().trim().to_owned()
}

/// A repository with one commit (`a.txt`) on `main` and a branch `feat` at it.
fn repo_with_feature_branch(tmp: &Path) -> PathBuf {
    let repo = tmp.join("repo");
    std::fs::create_dir_all(&repo).unwrap();
    sh(&repo, &["init", "-q", "-b", "main"]);
    for (k, v) in [
        ("user.name", "T"),
        ("user.email", "t@example.com"),
        ("commit.gpgsign", "false"),
    ] {
        sh(&repo, &["config", k, v]);
    }
    std::fs::write(repo.join("a.txt"), "one\n").unwrap();
    sh(&repo, &["add", "-A"]);
    sh(&repo, &["commit", "-qm", "init"]);
    sh(&repo, &["branch", "feat"]);
    repo
}

#[test]
fn worktree_changes_include_new_files_and_leave_the_index_alone() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = repo_with_feature_branch(tmp.path());
    let wt = tmp.path().join("wt");
    sh(
        &repo,
        &[
            "worktree",
            "add",
            "-q",
            "-b",
            "slice",
            &wt.to_string_lossy(),
            "feat",
        ],
    );
    std::fs::write(wt.join("a.txt"), "one\ntwo\n").unwrap();
    std::fs::write(wt.join("new.txt"), "x\ny\n").unwrap();
    let (_, numstat) = worktree_against_branch(&wt, "feat", DiffOutput::Numstat).unwrap();
    let stat = parse_numstat(&numstat);
    assert_eq!(
        stat.files
            .iter()
            .map(|f| f.path.as_str())
            .collect::<Vec<_>>(),
        ["a.txt", "new.txt"]
    );
    assert_eq!((stat.additions, stat.deletions), (3, 0));
    assert!(
        sh(&wt, &["status", "--porcelain"]).contains("?? new.txt"),
        "the worktree's own index is untouched"
    );
    let (_, patch) = worktree_against_branch(&wt, "feat", DiffOutput::Patch).unwrap();
    assert!(
        patch.contains("+two") && patch.contains("new.txt"),
        "{patch}"
    );
}

#[test]
fn feature_changes_list_commits_with_their_slice() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = repo_with_feature_branch(tmp.path());
    // A slice commit on the base is not the feature's; the one on the feature branch is.
    sh(
        &repo,
        &[
            "commit",
            "-q",
            "--allow-empty",
            "-m",
            "feat 01: one\n\nSlice: 01\n",
        ],
    );
    sh(&repo, &["checkout", "-q", "feat"]);
    std::fs::write(repo.join("b.txt"), "b\n").unwrap();
    sh(&repo, &["add", "-A"]);
    sh(&repo, &["commit", "-qm", "feat 02: two\n\nSlice: 02\n"]);
    sh(&repo, &["checkout", "-q", "main"]);
    let root = tmp.path().join("zsf");
    let dir = root.join("feat");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("feature.toml"),
        format!(
            "name = \"feat\"\nrepo = \"{}\"\nbranch = \"feat\"\nbase = \"main\"\n",
            repo.display()
        ),
    )
    .unwrap();
    let c = changes(&root, "feat", true).unwrap().unwrap();
    assert_eq!(
        c.commits
            .iter()
            .map(|c| c.slice.clone())
            .collect::<Vec<_>>(),
        [Some("02".to_owned())]
    );
    assert_eq!(c.stat.files.len(), 1);
    assert!(c.diff.unwrap().contains("+b"));
}

#[test]
fn log_records_carry_the_slice_trailer() {
    let log = "abc\u{1f}feat 03: three\u{1f}feat 03: three\n\nSlice: 03\n\u{1e}\ndef\u{1f}fixup\u{1f}fixup\n\u{1e}\n";
    let commits = parse_log(log);
    let got: Vec<(&str, &str, Option<&str>)> = commits
        .iter()
        .map(|c| (c.sha.as_str(), c.subject.as_str(), c.slice.as_deref()))
        .collect();
    assert_eq!(
        got,
        [
            ("abc", "feat 03: three", Some("03")),
            ("def", "fixup", None)
        ]
    );
}

#[test]
fn run_outcomes_read_the_same_in_the_line_and_the_label() {
    use super::run_context::{NextAttempt, Outcome};
    use zsf::model::AttemptKind;
    let not_merged = |why: &str, next: Option<NextAttempt>| Outcome::NotMerged {
        why: why.to_owned(),
        next,
    };
    let rerun = Some(NextAttempt {
        n: 2,
        kind: AttemptKind::Rerun,
    });
    let cases = [
        (
            Outcome::ResumedLater,
            "controller lost, resumed in a later run",
            "controller lost",
        ),
        (
            Outcome::Merged {
                gated: true,
                commit: Some("0123456789".into()),
            },
            "merged (0123456)",
            "merged",
        ),
        (
            Outcome::Merged {
                gated: false,
                commit: None,
            },
            "merged without the gate",
            "merged without the gate",
        ),
        (
            not_merged("gate failed, not merged", rerun),
            "gate failed, not merged, re-run as attempt 2",
            "gate failed",
        ),
        (
            not_merged("conflicts with 02 in src/a.rs", None),
            "conflicts with 02 in src/a.rs",
            "conflicts with 02",
        ),
        (
            not_merged(
                "gate failed, not merged",
                Some(NextAttempt {
                    n: 2,
                    kind: AttemptKind::GateRerun,
                }),
            ),
            "gate failed, not merged, re-run automatically as attempt 2",
            "gate failed",
        ),
        (
            not_merged(
                "changes requested",
                Some(NextAttempt {
                    n: 3,
                    kind: AttemptKind::ManualRetry,
                }),
            ),
            "changes requested, retried as attempt 3",
            "changes requested",
        ),
        (
            Outcome::Replaced { by: 3 },
            "replaced by attempt 3",
            "replaced by attempt 3",
        ),
        (
            Outcome::SliceSkipped,
            "slice skipped, not merged",
            "slice skipped",
        ),
    ];
    for (outcome, sentence, short) in cases {
        assert_eq!(
            (outcome.sentence().as_str(), outcome.short().as_str()),
            (sentence, short)
        );
    }
}

#[test]
fn line_diff_hunks() {
    assert_eq!(line_diff("a\nb\n", "a\nb\n", "x", "y"), "");
    let d = line_diff(
        "1\n2\n3\n4\n5\n6\n7\n8\n9\n10\n",
        "1\n2\n3\n4\nfive\n6\n7\n8\n9\n10\n11\n",
        "x",
        "y",
    );
    assert!(d.starts_with("--- a/x\n+++ b/y\n@@ -2,9 +2,10 @@\n"), "{d}");
    assert!(d.contains("\n-5\n+five\n") && d.ends_with("+11\n"), "{d}");
    let far = line_diff(
        &(1..40).map(|i| format!("{i}\n")).collect::<String>(),
        &(1..40)
            .map(|i| {
                if i == 2 || i == 35 {
                    "x\n".to_owned()
                } else {
                    format!("{i}\n")
                }
            })
            .collect::<String>(),
        "x",
        "y",
    );
    assert_eq!(far.matches("@@ ").count(), 2, "{far}");
}

#[test]
fn safe_relative_paths() {
    assert!(safe_relative("a/b.txt").is_some());
    assert!(
        safe_relative("../a").is_none()
            && safe_relative("/a").is_none()
            && safe_relative("").is_none()
            && safe_relative("a/./b").is_some()
    );
}

#[test]
fn a_pr_url_is_a_pull_or_merge_request_link() {
    use super::files::is_pr_url;
    for yes in [
        "https://github.com/o/r/pull/12",
        "https://github.com/o/r/pull/12/",
        "http://gitlab.example/g/p/-/merge_requests/3",
        "https://api.github.com/repos/o/r/pulls/7",
    ] {
        assert!(is_pr_url(yes), "{yes}");
    }
    for no in [
        "https://github.com/o/r",
        "https://github.com/o/r/pull/",
        "https://github.com/o/r/pull/12/files",
        "github.com/o/r/pull/12",
        "ftp://x/pull/1",
        "https://x/pull/1a",
    ] {
        assert!(!is_pr_url(no), "{no}");
    }
}

#[test]
fn the_pr_link_is_read_from_the_declared_artifacts_worktree_first() {
    let tmp = tempfile::tempdir().unwrap();
    let mut feature = fixture::build(tmp.path());
    let worktree = tmp.path().join("wt");
    let saved = feature.artifacts_dir().join("03");
    let write = |path: PathBuf, text: &str| {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    };
    write(
        worktree.join(".zeroshot-pr/PR.md"),
        "Opened: <https://github.com/o/r/pull/9>\n",
    );
    write(
        saved.join(".zeroshot-pr/PR.md"),
        "https://github.com/o/r/pull/8\n",
    );
    // Not declared: nothing is read.
    assert_eq!(files::pr_url(&feature, "03", Some(&worktree)), None);
    feature.config.artifacts = vec![".zeroshot-pr/".to_owned()];
    assert_eq!(
        files::pr_url(&feature, "03", Some(&worktree)).as_deref(),
        Some("https://github.com/o/r/pull/9")
    );
    // No worktree (merged, or removed): what zsf saved.
    assert_eq!(
        files::pr_url(&feature, "03", None).as_deref(),
        Some("https://github.com/o/r/pull/8")
    );
    // A declared PR.md file, and one without a link.
    feature.config.artifacts = vec!["PR.md".to_owned()];
    write(worktree.join("PR.md"), "no link yet\n");
    assert_eq!(files::pr_url(&feature, "03", Some(&worktree)), None);
}
