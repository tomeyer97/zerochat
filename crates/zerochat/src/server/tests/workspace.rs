//! Where a local run's files are read.

use serde_json::{Value, json};

use crate::source::{self, RunData};
use crate::workspace::location::workspace_in;

#[test]
fn a_removed_zsf_worktree_is_read_from_its_feature_repository() {
    let tmp = tempfile::tempdir().unwrap();
    let (root, repo) = (tmp.path().join("zsf"), tmp.path().join("repo"));
    std::fs::create_dir_all(root.join("feat")).unwrap();
    std::fs::create_dir_all(&repo).unwrap();
    let toml = format!(
        "name = \"feat\"\nrepo = {:?}\nbranch = \"b\"\nbase = \"main\"\n",
        repo.display().to_string()
    );
    std::fs::write(root.join("feat/feature.toml"), toml).unwrap();
    let event = |seq, body: Value| source::Event { seq, body };
    let data = |workspace: &std::path::Path| RunData {
        run_id: "r".into(),
        title: "t".into(),
        admitted: Value::Null,
        snapshot: Value::Null,
        events: vec![
            event(
                1,
                json!({"kind": "node_completed", "workspace": {"base": "b1", "commit": "c1"}}),
            ),
            event(
                2,
                json!({"kind": "node_completed", "workspace": {"base": "c1", "commit": "c2"}}),
            ),
            event(3, json!({"kind": "terminal"})),
        ],
        local: Some(source::LocalFacts {
            workspace: Some(workspace.display().to_string()),
            ..Default::default()
        }),
        live_usage: Default::default(),
    };
    // Still there: the workspace itself, "now" is its working tree.
    let present = workspace_in(&data(&repo), &root).unwrap();
    assert_eq!(
        (
            present.path.as_path(),
            present.removed,
            present.now(None).unwrap()
        ),
        (repo.as_path(), false, None)
    );
    // A slice worktree zsf removed: the feature's repository, "now" is the last snapshot.
    let removed = workspace_in(&data(&root.join("feat/worktrees/01-x")), &root).unwrap();
    assert_eq!(
        (removed.path.as_path(), removed.removed),
        (repo.as_path(), true)
    );
    assert_eq!(removed.now(None).unwrap().as_deref(), Some("c2"));
    assert_eq!(
        removed.now(Some("c1".into())).unwrap().as_deref(),
        Some("c1")
    );
    // Anything else that is gone stays unavailable.
    assert!(workspace_in(&data(&tmp.path().join("elsewhere")), &root).is_none());
}
