//! The features (zsf) API, and a slice run's feature context.

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::Json;
use serde_json::json;

use crate::server::api::features::{
    EventsQuery, feature_detail, feature_events, features_list, slice_action, slice_artifact,
    slice_task,
};

use super::zsf_state;

#[tokio::test]
async fn features_api_lists_details_and_refuses_what_zsf_would() {
    let dir = tempfile::tempdir().unwrap();
    let state = zsf_state(dir.path());
    let Json(list) = features_list(State(state.clone())).await.unwrap();
    assert_eq!(list["features"][0]["name"], "calc");
    assert_eq!(
        (
            list["features"][0]["approve"].clone(),
            list["features"][0]["failed"].clone()
        ),
        (json!(["01"]), json!(["03"]))
    );
    assert_eq!(list["features"][0]["needsYou"], true);
    let Json(detail) = feature_detail(State(state.clone()), Path("calc".to_owned()))
        .await
        .unwrap();
    assert_eq!(detail["slices"][0]["status"], "waiting_for_approval");
    assert_eq!(detail["slices"][0]["actions"], json!(["approve"]));
    assert_eq!(detail["slices"][1]["approvalOf"], json!(["01"]));
    assert_eq!(detail["actions"], json!(["max_parallel"]));
    assert_eq!(
        feature_detail(State(state.clone()), Path("nope".to_owned()))
            .await
            .unwrap_err()
            .status,
        StatusCode::NOT_FOUND
    );

    let Json(task) = slice_task(
        State(state.clone()),
        Path(("calc".to_owned(), "03".to_owned(), 2)),
    )
    .await
    .unwrap();
    assert!(
        task["diff"]
            .as_str()
            .unwrap()
            .contains("+Use the existing parser.")
    );
    let Json(events) = feature_events(
        State(state.clone()),
        Path("calc".to_owned()),
        Query(EventsQuery {
            slice: Some("03".into()),
        }),
    )
    .await
    .unwrap();
    assert_eq!(events["events"].as_array().unwrap().len(), 1);
    let missing = slice_artifact(
        State(state.clone()),
        Path((
            "calc".to_owned(),
            "03".to_owned(),
            "../state.json".to_owned(),
        )),
    )
    .await
    .unwrap_err();
    assert_eq!(missing.status, StatusCode::NOT_FOUND);

    let body = |action: &str| {
        Json(crate::features::ActionBody {
            action: action.into(),
            ..Default::default()
        })
    };
    let refused = slice_action(
        State(state.clone()),
        Path(("calc".to_owned(), "02".to_owned())),
        body("approve"),
    )
    .await
    .unwrap_err();
    assert_eq!(
        (refused.status, refused.code.as_str()),
        (StatusCode::CONFLICT, "refused")
    );
    let Json(ok) = slice_action(
        State(state.clone()),
        Path(("calc".to_owned(), "01".to_owned())),
        body("approve"),
    )
    .await
    .unwrap();
    assert_eq!(ok["ok"], true);
    let Json(after) = feature_detail(State(state.clone()), Path("calc".to_owned()))
        .await
        .unwrap();
    assert_eq!(after["slices"][0]["status"], "merged");
    assert_eq!(after["slices"][0]["actions"], json!([]));
}

#[tokio::test]
async fn a_slice_run_has_its_feature_context() {
    let dir = tempfile::tempdir().unwrap();
    let state = zsf_state(dir.path());
    let context = state
        .context(crate::features::fixture::RUN_03_A2)
        .await
        .unwrap();
    assert_eq!(
        (context.kind, context.line.as_str()),
        (
            "slice",
            "Slice 03 of calc · attempt 2 of 2 · gate failed, not merged"
        )
    );
    assert_eq!(
        (context.feature.as_deref(), context.slice.as_deref()),
        (Some("calc"), Some("03"))
    );
    assert!(
        state
            .context("01a1ab83-ffff-7000-8000-000000000000")
            .await
            .is_none()
    );
}

#[tokio::test]
async fn request_changes_on_a_slice_waiting_for_review_needs_notes() {
    let dir = tempfile::tempdir().unwrap();
    let state = zsf_state(dir.path());
    // 03 (failed, worktree kept) as a slice whose run is done and waits for review.
    let feature = zsf::store::Feature::open(&dir.path().join("zsf"), "calc").unwrap();
    let mut s = feature.load_state().unwrap();
    s.slices.get_mut("03").unwrap().status = zsf::model::SliceStatus::WaitingForReview;
    feature.save_state(&mut s).unwrap();

    let Json(detail) = feature_detail(State(state.clone()), Path("calc".to_owned()))
        .await
        .unwrap();
    let s03 = detail["slices"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["id"] == "03")
        .unwrap()
        .clone();
    assert_eq!(
        (s03["status"].clone(), s03["actions"].clone()),
        (
            json!("waiting_for_review"),
            json!(["approve", "retry", "skip"])
        )
    );
    assert_eq!(s03["prUrl"], json!(null));

    for notes in [None, Some(" \n ".to_owned())] {
        let body = Json(crate::features::ActionBody {
            action: "retry".into(),
            notes,
            ..Default::default()
        });
        let refused = slice_action(
            State(state.clone()),
            Path(("calc".to_owned(), "03".to_owned())),
            body,
        )
        .await
        .unwrap_err();
        assert_eq!(
            (refused.status, refused.code.as_str()),
            (StatusCode::BAD_REQUEST, "invalid_request")
        );
        assert!(
            refused.message.starts_with("Request changes needs notes"),
            "{}",
            refused.message
        );
    }
}
