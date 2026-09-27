//! Read-only Core facade for deployment operation lifecycle replay.
//!
//! Admission, persistence and effects remain outside this source slice.  Core only exposes the
//! domain reducer so every entrypoint can use the same deterministic operation projection.

use kiana_domain::{OperationJournal, OperationTransition};

pub fn validate_operation_journal(journal: &OperationJournal) -> Result<(), String> {
    journal.validate()
}

pub fn replay_operation_journal(
    journal: &OperationJournal,
    transitions: impl IntoIterator<Item = OperationTransition>,
) -> Result<OperationJournal, String> {
    let mut replayed = OperationJournal::new(
        journal.operation_id,
        journal.revision_id.clone(),
        journal.revision_digest.clone(),
    )?;
    for transition in transitions {
        replayed.append(transition)?;
    }
    Ok(replayed)
}
