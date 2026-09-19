use kiana_domain::{
    json_digest, memory_source_snapshot, EvidenceStatus, RetrievalReceipt, RetrievalReceiptEntry,
    RetrievalReceiptStage,
};
use serde_json::json;

fn digest(value: &str) -> String {
    json_digest(&json!({"value": value}))
}

fn entry(
    candidate_id: &str,
    stage: RetrievalReceiptStage,
    evidence: EvidenceStatus,
) -> RetrievalReceiptEntry {
    let snapshot = memory_source_snapshot(
        candidate_id,
        "project:code",
        "revision:7",
        &digest(candidate_id),
        evidence,
    )
    .unwrap();
    RetrievalReceiptEntry::new(
        candidate_id,
        stage,
        snapshot,
        digest("retrieval"),
        false,
        None,
    )
    .unwrap()
}

fn receipt(entries: Vec<RetrievalReceiptEntry>) -> RetrievalReceipt {
    RetrievalReceipt::new(
        "receipt:cm27",
        "find the bounded source",
        json_digest(&json!({"query": "find the bounded source"})),
        digest("scope"),
        "bm25+rrf.v1",
        7,
        false,
        Vec::new(),
        entries,
        Vec::new(),
    )
    .unwrap()
}

#[test]
fn receipt_distinguishes_retrieved_from_sent() {
    let receipt = receipt(vec![
        entry(
            "memory-1",
            RetrievalReceiptStage::Retrieved,
            EvidenceStatus::Attributed,
        ),
        entry(
            "memory-1",
            RetrievalReceiptStage::Selected,
            EvidenceStatus::Attributed,
        ),
    ]);
    receipt.validate().unwrap();
    assert!(receipt.entries.iter().any(|entry| {
        entry.candidate_id == "memory-1" && entry.stage == RetrievalReceiptStage::Retrieved
    }));
    assert!(!receipt
        .entries
        .iter()
        .any(|entry| entry.stage == RetrievalReceiptStage::Sent));
    assert!(receipt.cite("memory-1", digest("quote")).is_err());
}

#[test]
fn reviewer_cannot_cite_unverifiable_memory() {
    let retrieved = entry(
        "memory-unverifiable",
        RetrievalReceiptStage::Retrieved,
        EvidenceStatus::Unverifiable,
    );
    let selected = entry(
        "memory-unverifiable",
        RetrievalReceiptStage::Selected,
        EvidenceStatus::Unverifiable,
    );
    let sent = entry(
        "memory-unverifiable",
        RetrievalReceiptStage::Sent,
        EvidenceStatus::Unverifiable,
    );
    let source = sent.source_snapshot.clone();
    let cited = RetrievalReceiptEntry {
        candidate_id: "memory-unverifiable".to_owned(),
        stage: RetrievalReceiptStage::Cited,
        source_snapshot: source,
        source_revision: "revision:7".to_owned(),
        retrieval_evidence_digest: digest("retrieval"),
        degraded: false,
        omission: None,
    };
    assert_eq!(
        RetrievalReceipt::new(
            "receipt:cm27-unverifiable",
            "find the bounded source",
            json_digest(&json!({"query": "find the bounded source"})),
            digest("scope"),
            "bm25+rrf.v1",
            7,
            false,
            Vec::new(),
            vec![retrieved, selected, sent, cited],
            Vec::new(),
        )
        .unwrap_err(),
        "retrieval_citation_provenance_unverifiable"
    );
}
