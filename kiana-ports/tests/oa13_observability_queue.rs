use kiana_ports::{
    ObservabilityQueue, ObservabilityQueueClass, ObservabilityQueueError, QueuedObservabilityItem,
};

fn item(class: ObservabilityQueueClass, cursor: u64) -> QueuedObservabilityItem {
    QueuedObservabilityItem::critical(class, cursor)
}

#[test]
fn critical_items_evict_only_best_effort_and_never_silently_drop() {
    let queue = ObservabilityQueue::new(2).unwrap();
    queue
        .try_enqueue(QueuedObservabilityItem::critical(
            ObservabilityQueueClass::Log,
            1,
        ))
        .unwrap();
    queue
        .try_enqueue(item(ObservabilityQueueClass::Event, 2))
        .unwrap();
    queue
        .try_enqueue(item(ObservabilityQueueClass::Audit, 3))
        .unwrap();

    let stats = queue.stats();
    assert_eq!(stats.depth, 2);
    assert_eq!(stats.dropped_best_effort_total, 1);
    assert_eq!(stats.critical_rejected_total, 0);
    assert_eq!(
        queue.try_dequeue().unwrap().class,
        ObservabilityQueueClass::Event
    );
    assert_eq!(
        queue.try_dequeue().unwrap().class,
        ObservabilityQueueClass::Audit
    );

    queue
        .try_enqueue(item(ObservabilityQueueClass::Approval, 4))
        .unwrap();
    queue
        .try_enqueue(item(ObservabilityQueueClass::Recovery, 5))
        .unwrap();
    let error = queue
        .try_enqueue(item(ObservabilityQueueClass::Terminal, 6))
        .unwrap_err();
    assert_eq!(
        error,
        ObservabilityQueueError::CriticalQueueFull {
            class: ObservabilityQueueClass::Terminal
        }
    );
    assert_eq!(queue.stats().critical_rejected_total, 1);
    assert_eq!(queue.try_dequeue().unwrap().source_cursor, 4);
    assert_eq!(queue.try_dequeue().unwrap().source_cursor, 5);
}

#[tokio::test]
async fn best_effort_drop_has_reason_and_flush_shutdown_reopen_ack() {
    let queue = ObservabilityQueue::new(1).unwrap();
    queue
        .try_enqueue(QueuedObservabilityItem::critical(
            ObservabilityQueueClass::Trace,
            1,
        ))
        .unwrap();
    assert!(matches!(
        queue.try_enqueue(QueuedObservabilityItem::critical(
            ObservabilityQueueClass::Metric,
            2,
        )),
        Err(ObservabilityQueueError::BestEffortDropped { .. })
    ));
    assert_eq!(
        queue.stats().last_drop_reason.as_deref(),
        Some("best_effort_queue_full")
    );
    assert_eq!(queue.dequeue().await.unwrap().source_cursor, 1);
    assert_eq!(queue.flush().await, 1);
    let shutdown = queue.shutdown();
    assert!(shutdown.closed);
    assert!(matches!(
        queue.try_enqueue(item(ObservabilityQueueClass::Event, 3)),
        Err(ObservabilityQueueError::Closed)
    ));
    let reopened = queue.reopen();
    assert!(!reopened.closed);
    queue
        .try_enqueue(item(ObservabilityQueueClass::Terminal, 4))
        .unwrap();
    assert_eq!(queue.dequeue().await.unwrap().source_cursor, 4);
    assert_eq!(queue.flush().await, 2);
}

#[tokio::test]
async fn cancellable_flush_returns_without_blocking_when_cancelled() {
    let queue = ObservabilityQueue::new(1).unwrap();
    queue
        .try_enqueue(item(ObservabilityQueueClass::Event, 1))
        .unwrap();
    let (sender, receiver) = tokio::sync::watch::channel(false);
    let waiter = tokio::spawn({
        let queue = queue.clone();
        async move { queue.flush_cancellable(receiver).await }
    });
    sender.send(true).unwrap();
    assert_eq!(
        waiter.await.unwrap().unwrap_err(),
        ObservabilityQueueError::FlushCancelled
    );
    assert_eq!(queue.try_dequeue().unwrap().source_cursor, 1);
}
