//! The Start-a-batch form's options, from pb.toml.

use std::path::Path;

use super::super::*;
use super::write_file;

#[test]
fn harness_options_reads_task_sets_repo_tasks_variants_and_limits() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    // Task ids as ProgramBench names them: `<owner>__<repo>.<commit>`.
    write_file(
        root,
        "pb.toml",
        r#"
        [paths]
        tasks_dir = "tasks"
        [limits]
        max_cost_usd = 6.0
        max_wall_minutes = 45
        max_steps = 24
        max_concurrent_runs = 3
        [task_sets]
        dev = ["tomnomnom__gron.88a6234", "yaa110__nomino.f892499"]
        smoke = ["tomnomnom__gron.88a6234"]
        "#,
    );
    write_file(root, "tasks/k8s-job-backoff-limit-per-index/task.toml", "");
    write_file(root, "tasks/not-a-task/README", ""); // no task.toml: excluded
    write_file(root, "harness/variants/clone-v1/graph.json", "{}");
    write_file(root, "harness/variants/clone-v1/runtime.json", "{}");
    write_file(root, "harness/variants/incomplete/graph.json", "{}"); // no runtime.json: excluded
    write_file(root, "harness/variants/adversarial_lib.py", ""); // a file, not a variant dir

    let opts = harness_options(root).unwrap();
    assert_eq!(
        opts.task_sets
            .iter()
            .map(|s| s.name.clone())
            .collect::<Vec<_>>(),
        vec!["dev".to_owned(), "smoke".to_owned()]
    );
    assert_eq!(
        opts.tasks,
        vec![
            "tomnomnom__gron.88a6234".to_owned(),
            "yaa110__nomino.f892499".to_owned()
        ]
    );
    assert_eq!(
        opts.repo_tasks,
        vec!["k8s-job-backoff-limit-per-index".to_owned()]
    );
    assert_eq!(opts.variants, vec!["clone-v1".to_owned()]);
    assert_eq!(opts.default_max_cost_usd, 6.0);
    assert_eq!(opts.default_max_wall_minutes, 45.0);
    assert_eq!(opts.default_max_steps, 24);
    assert_eq!(opts.max_concurrent_runs, 3);
}

#[test]
fn harness_options_defaults_limits_when_pb_toml_omits_them() {
    let dir = tempfile::tempdir().unwrap();
    write_file(dir.path(), "pb.toml", "[task_sets]\n");
    let opts = harness_options(dir.path()).unwrap();
    assert_eq!(opts.default_max_cost_usd, 5.0);
    assert_eq!(opts.default_max_wall_minutes, 45.0);
    assert_eq!(opts.default_max_steps, 12);
    assert_eq!(opts.max_concurrent_runs, 4, "pb's own default ceiling");
    assert!(opts.variants.is_empty() && opts.repo_tasks.is_empty());
}

#[test]
fn harness_options_errors_on_a_missing_pb_toml() {
    assert!(harness_options(Path::new("/does/not/exist")).is_err());
}
