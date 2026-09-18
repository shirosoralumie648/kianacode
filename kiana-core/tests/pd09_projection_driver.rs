use kiana_core::{ProjectionDriver, ProjectionDriverStatus};
use kiana_domain::{RequestId, RuntimeEvent};
use serde_json::json;

fn events(start: u64, count: u64) -> Vec<RuntimeEvent> {
    (0..count)
        .map(|offset| {
            RuntimeEvent::new(
                RequestId::new(),
                start + offset,
                "projection.add",
                json!({"n": 1}),
            )
            .unwrap()
        })
        .collect()
}

#[test]
fn projector_driver_checkpoints_tail_and_rebuilds_deterministically() {
    let first = events(1, 2);
    let tail = events(3, 1);
    let mut driver = ProjectionDriver::new("pd09").unwrap();
    driver
        .apply(json!({"total": 0}), &first, 2, |mut state, event| {
            state["total"] = json!(
                state["total"].as_u64().unwrap() + event.data.get("n").unwrap().as_u64().unwrap()
            );
            Ok(state)
        })
        .unwrap();
    assert_eq!(driver.status(), ProjectionDriverStatus::Ready);
    assert_eq!(driver.checkpoint().unwrap().source_cursor, 2);
    driver
        .apply(json!({"total": 0}), &tail, 3, |mut state, event| {
            state["total"] = json!(
                state["total"].as_u64().unwrap() + event.data.get("n").unwrap().as_u64().unwrap()
            );
            Ok(state)
        })
        .unwrap();
    assert_eq!(driver.checkpoint().unwrap().source_cursor, 3);
    driver.rebuild();
    assert_eq!(driver.status(), ProjectionDriverStatus::RebuildRequired);
    let all = [first, tail].concat();
    driver
        .apply(json!({"total": 0}), &all, 3, |mut state, event| {
            state["total"] = json!(
                state["total"].as_u64().unwrap() + event.data.get("n").unwrap().as_u64().unwrap()
            );
            Ok(state)
        })
        .unwrap();
    assert_eq!(driver.checkpoint().unwrap().state["total"], 3);
}

#[test]
fn projector_driver_pauses_on_fold_error_and_requires_explicit_retry() {
    let mut driver = ProjectionDriver::new("pd09-error").unwrap();
    let event = events(1, 1);
    assert_eq!(
        driver
            .apply(json!({}), &event, 1, |_state, _event| Err(
                "fold_broken".to_owned()
            ))
            .unwrap_err(),
        "projection_fold_failed:fold_broken"
    );
    assert_eq!(driver.status(), ProjectionDriverStatus::Paused);
    assert_eq!(
        driver
            .apply(json!({}), &event, 1, |_state, _event| Ok(json!({})))
            .unwrap_err(),
        "projection_driver_paused"
    );
    driver.retry().unwrap();
    assert_eq!(driver.retry_count(), 1);
    driver
        .apply(json!({}), &event, 1, |_state, _event| {
            Ok(json!({"ok": true}))
        })
        .unwrap();
}
