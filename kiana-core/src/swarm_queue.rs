//! SW-06 read-only ControlPlane facade for queue claim/fence evidence.

use kiana_domain::{QueueEntryId, SwarmQueueEntry, SwarmQueueLedger};

pub fn enqueue_swarm_entry(
    ledger: &mut SwarmQueueLedger,
    entry: SwarmQueueEntry,
) -> Result<(), &'static str> {
    ledger.enqueue(entry)
}

pub fn claim_swarm_entry(
    ledger: &mut SwarmQueueLedger,
    now_unix_ms: u64,
) -> Result<SwarmQueueEntry, &'static str> {
    ledger.claim(now_unix_ms)
}

pub fn complete_swarm_entry(
    ledger: &mut SwarmQueueLedger,
    entry_id: QueueEntryId,
    fence_epoch: u64,
) -> Result<(), &'static str> {
    ledger.complete(entry_id, fence_epoch)
}
