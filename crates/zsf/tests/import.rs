//! `zsf import` of a synthetic refined spec (tests/fixtures/refined) with awkward INDEX.md
//! columns.

mod common;

use std::path::Path;

/// A synthetic refined spec (`SPEC.md`, `slices/INDEX.md` and one file per slice) whose
/// INDEX.md table has awkward dependency columns.
const REFINED: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/refined");

/// Copies the fixture to `dir` (a test may change or delete its copy).
fn refined(dir: &Path) {
    copy_dir(Path::new(REFINED), dir);
}

fn copy_dir(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for entry in std::fs::read_dir(from).unwrap().flatten() {
        let target = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_dir(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), target).unwrap();
        }
    }
}

#[test]
fn imports_index_with_tricky_dependency_columns() {
    let tmp = tempfile::tempdir().unwrap();
    refined(tmp.path());
    let imported = zsf::import::parse_refined(tmp.path()).unwrap();
    let deps: Vec<(String, Vec<String>)> = imported
        .plan
        .slices
        .iter()
        .map(|s| (s.id.clone(), s.deps.clone()))
        .collect();
    let expect = |id: &str, d: &[&str]| {
        (
            id.to_owned(),
            d.iter().map(|s| s.to_string()).collect::<Vec<_>>(),
        )
    };
    assert_eq!(
        deps,
        [
            expect("01", &[]),
            expect("02", &[]),
            expect("03", &["01", "02"]),
            expect("04", &["01", "02", "03"]),
            expect("05", &["04"])
        ]
    );
    let s1 = imported.plan.get("01").unwrap();
    assert_eq!(s1.title, "Add the limits table");
    let s3 = imported.plan.get("03").unwrap();
    assert_eq!(s3.commit_title.as_deref(), Some("TOY 03: enforce limits"));
    assert_eq!(
        s3.description.as_deref(),
        Some("Rejects requests over the limit.")
    );
    // "5" in the id column is slice 05, like the file it links to.
    assert!(
        imported
            .plan
            .get("05")
            .unwrap()
            .spec
            .ends_with("05-docs.md")
    );
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
}

#[test]
fn import_into_a_feature_copies_the_context_once() {
    let tmp = tempfile::tempdir().unwrap();
    let spec = tmp.path().join("refined");
    refined(&spec);
    let repo = common::new_repo(tmp.path());
    let root = tmp.path().join("state");
    let done = zsf::setup::init(
        &root,
        zsf::setup::InitOptions {
            name: "toy".into(),
            repo,
            branch: "toy".into(),
            base: "main".into(),
            plan: None,
            refined: Some(spec.clone()),
            checkpoints: vec!["03".into()],
            configure: Box::new(|_| {}),
        },
    )
    .unwrap();
    let feature = done.feature;
    let plan = feature.load_plan().unwrap();
    assert_eq!(plan.slices.len(), 5);
    assert!(plan.get("03").unwrap().checkpoint);
    // Spec paths point into the feature's own copy, relative to plan.toml.
    let s1 = plan.get("01").unwrap();
    assert_eq!(s1.spec, Path::new("context/slices/01-limits-table.md"));
    assert!(feature.spec_path(&s1.spec).is_file());
    assert!(feature.dir.join("context/SPEC.md").is_file());
    assert_eq!(feature.context_dir().unwrap(), feature.dir.join("context"));
    // The source can go away; the feature keeps working from its copy.
    std::fs::remove_dir_all(&spec).unwrap();
    assert!(feature.spec_path(&plan.get("05").unwrap().spec).is_file());
}
