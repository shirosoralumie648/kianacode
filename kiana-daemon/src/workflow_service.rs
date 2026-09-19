//! Bounded Tokio queue service owned by `DaemonHost`.
//!
//! The service owns queue coordination only.  It never invokes a capability, model or provider;
//! a claimed lease is returned to the existing ControlPlane route, and the Broker remains the
//! single effect boundary.  The channel is bounded so a scheduler tick cannot create unbounded
//! work or silently widen concurrency.

use kiana_domain::{
    WorkflowQueueClaimRequest, WorkflowQueueEffectRequest, WorkflowQueueFenceRequest,
    WorkflowQueueHeartbeatRequest, WorkflowQueueLease, WorkflowQueueLeaseStatus,
    WorkflowQueueReclaimRequest,
};
use kiana_ports::{PortError, WorkflowQueueStore};
use std::collections::BTreeMap;
use std::sync::Arc;
use tokio::sync::{mpsc, oneshot, watch, Mutex as AsyncMutex};
use tokio::task::JoinHandle;

pub const WORKFLOW_SERVICE_CHANNEL_CAPACITY: usize = 64;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WorkflowQueueShutdownReport {
    pub fenced_count: usize,
    pub recovery_required_count: usize,
}

enum WorkflowQueueCommand {
    Claim(
        WorkflowQueueClaimRequest,
        oneshot::Sender<Result<WorkflowQueueLease, PortError>>,
    ),
    Heartbeat(
        WorkflowQueueHeartbeatRequest,
        oneshot::Sender<Result<WorkflowQueueLease, PortError>>,
    ),
    Effect(
        WorkflowQueueEffectRequest,
        oneshot::Sender<Result<WorkflowQueueLease, PortError>>,
    ),
    Fence(
        WorkflowQueueFenceRequest,
        oneshot::Sender<Result<WorkflowQueueLease, PortError>>,
    ),
    Reclaim(
        WorkflowQueueReclaimRequest,
        oneshot::Sender<Result<WorkflowQueueLease, PortError>>,
    ),
    Tick(
        usize,
        oneshot::Sender<Result<Vec<kiana_domain::WorkflowQueueClaimContract>, PortError>>,
    ),
    Shutdown(
        Option<u64>,
        oneshot::Sender<Result<WorkflowQueueShutdownReport, PortError>>,
    ),
}

struct RunningService {
    sender: mpsc::Sender<WorkflowQueueCommand>,
    shutdown: watch::Sender<bool>,
    join: JoinHandle<()>,
}

#[derive(Default)]
struct ServiceState {
    running: Option<RunningService>,
}

/// One bounded queue scheduler/worker service per `DaemonHost`.
#[derive(Clone, Default)]
pub struct WorkflowQueueService {
    state: Arc<AsyncMutex<ServiceState>>,
}

impl std::fmt::Debug for WorkflowQueueService {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("WorkflowQueueService")
            .field("channel_capacity", &WORKFLOW_SERVICE_CHANNEL_CAPACITY)
            .finish_non_exhaustive()
    }
}

impl WorkflowQueueService {
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn start(&self, store: Arc<dyn WorkflowQueueStore>) -> Result<(), PortError> {
        let mut state = self.state.lock().await;
        if state.running.is_some() {
            return Err(PortError::Conflict(
                "workflow_service_already_started".to_owned(),
            ));
        }
        let (sender, receiver) = mpsc::channel(WORKFLOW_SERVICE_CHANNEL_CAPACITY);
        let (shutdown, shutdown_receiver) = watch::channel(false);
        let join = tokio::spawn(run_worker(store, receiver, shutdown_receiver));
        state.running = Some(RunningService {
            sender,
            shutdown,
            join,
        });
        Ok(())
    }

    pub async fn claim(
        &self,
        request: WorkflowQueueClaimRequest,
    ) -> Result<WorkflowQueueLease, PortError> {
        self.request(|reply| WorkflowQueueCommand::Claim(request, reply))
            .await
    }

    pub async fn heartbeat(
        &self,
        request: WorkflowQueueHeartbeatRequest,
    ) -> Result<WorkflowQueueLease, PortError> {
        self.request(|reply| WorkflowQueueCommand::Heartbeat(request, reply))
            .await
    }

    pub async fn record_effect(
        &self,
        request: WorkflowQueueEffectRequest,
    ) -> Result<WorkflowQueueLease, PortError> {
        self.request(|reply| WorkflowQueueCommand::Effect(request, reply))
            .await
    }

    pub async fn fence(
        &self,
        request: WorkflowQueueFenceRequest,
    ) -> Result<WorkflowQueueLease, PortError> {
        self.request(|reply| WorkflowQueueCommand::Fence(request, reply))
            .await
    }

    pub async fn reclaim(
        &self,
        request: WorkflowQueueReclaimRequest,
    ) -> Result<WorkflowQueueLease, PortError> {
        self.request(|reply| WorkflowQueueCommand::Reclaim(request, reply))
            .await
    }

    pub async fn tick(
        &self,
        limit: usize,
    ) -> Result<Vec<kiana_domain::WorkflowQueueClaimContract>, PortError> {
        self.request(|reply| WorkflowQueueCommand::Tick(limit, reply))
            .await
    }

    async fn request<T>(
        &self,
        build: impl FnOnce(oneshot::Sender<Result<T, PortError>>) -> WorkflowQueueCommand,
    ) -> Result<T, PortError> {
        let sender = {
            let state = self.state.lock().await;
            state
                .running
                .as_ref()
                .map(|running| running.sender.clone())
                .ok_or_else(|| PortError::Unavailable("workflow_service_not_started".to_owned()))?
        };
        let (reply, response) = oneshot::channel();
        sender
            .send(build(reply))
            .await
            .map_err(|_| PortError::Failed("workflow_service_command_lost".to_owned()))?;
        response
            .await
            .map_err(|_| PortError::Failed("workflow_service_ack_lost".to_owned()))?
    }

    /// Stop the worker after all commands already accepted by the bounded channel have been
    /// processed. Active leases are fenced; an in-flight effect becomes recovery-required.
    pub async fn shutdown(&self) -> Result<WorkflowQueueShutdownReport, PortError> {
        self.shutdown_at(None).await
    }

    pub async fn shutdown_at(
        &self,
        observed_at_unix_ms: Option<u64>,
    ) -> Result<WorkflowQueueShutdownReport, PortError> {
        let running = { self.state.lock().await.running.take() };
        let Some(running) = running else {
            return Ok(WorkflowQueueShutdownReport {
                fenced_count: 0,
                recovery_required_count: 0,
            });
        };
        let (reply, response) = oneshot::channel();
        running
            .sender
            .send(WorkflowQueueCommand::Shutdown(observed_at_unix_ms, reply))
            .await
            .map_err(|_| PortError::Failed("workflow_service_shutdown_command_lost".to_owned()))?;
        let report = response
            .await
            .map_err(|_| PortError::Failed("workflow_service_shutdown_ack_lost".to_owned()))??;
        let _ = running.shutdown.send(true);
        running
            .join
            .await
            .map_err(|_| PortError::Failed("workflow_service_join_failed".to_owned()))?;
        Ok(report)
    }
}

async fn run_worker(
    store: Arc<dyn WorkflowQueueStore>,
    mut receiver: mpsc::Receiver<WorkflowQueueCommand>,
    mut shutdown: watch::Receiver<bool>,
) {
    let mut active = BTreeMap::<String, WorkflowQueueLease>::new();
    loop {
        tokio::select! {
            command = receiver.recv() => {
                let Some(command) = command else { break; };
                let stop = handle_command(&*store, command, &mut active).await;
                if stop { break; }
            }
            changed = shutdown.changed() => {
                if changed.is_ok() && *shutdown.borrow() { break; }
            }
        }
    }
    receiver.close();
}

async fn handle_command(
    store: &dyn WorkflowQueueStore,
    command: WorkflowQueueCommand,
    active: &mut BTreeMap<String, WorkflowQueueLease>,
) -> bool {
    match command {
        WorkflowQueueCommand::Claim(request, reply) => {
            let result = store.claim(request).await;
            if let Ok(lease) = &result {
                active.insert(lease.item_id.clone(), lease.clone());
            }
            let _ = reply.send(result);
            false
        }
        WorkflowQueueCommand::Heartbeat(request, reply) => {
            let result = store.heartbeat(request).await;
            if let Ok(lease) = &result {
                active.insert(lease.item_id.clone(), lease.clone());
            }
            let _ = reply.send(result);
            false
        }
        WorkflowQueueCommand::Effect(request, reply) => {
            let result = store.record_effect(request).await;
            if let Ok(lease) = &result {
                if lease.status == WorkflowQueueLeaseStatus::Active {
                    active.insert(lease.item_id.clone(), lease.clone());
                } else {
                    active.remove(&lease.item_id);
                }
            }
            let _ = reply.send(result);
            false
        }
        WorkflowQueueCommand::Fence(request, reply) => {
            let result = store.fence(request).await;
            if let Ok(lease) = &result {
                active.remove(&lease.item_id);
            }
            let _ = reply.send(result);
            false
        }
        WorkflowQueueCommand::Reclaim(request, reply) => {
            let result = store.reclaim(request).await;
            if let Ok(lease) = &result {
                active.insert(lease.item_id.clone(), lease.clone());
            }
            let _ = reply.send(result);
            false
        }
        WorkflowQueueCommand::Tick(limit, reply) => {
            let _ = reply.send(store.ready(limit).await);
            false
        }
        WorkflowQueueCommand::Shutdown(observed_at_unix_ms, reply) => {
            let mut fenced_count = 0;
            let mut recovery_required_count = 0;
            let leases = active.values().cloned().collect::<Vec<_>>();
            for lease in leases {
                let observed_at = observed_at_unix_ms.unwrap_or(lease.heartbeat_at_unix_ms);
                let result = store
                    .fence(WorkflowQueueFenceRequest {
                        item_id: lease.item_id.clone(),
                        owner_id: lease.owner_id.clone(),
                        fence_token: lease.fence_token,
                        authority_epoch: lease.authority_epoch,
                        observed_at_unix_ms: observed_at,
                    })
                    .await;
                match result {
                    Ok(next) if next.status == WorkflowQueueLeaseStatus::ResultUnknown => {
                        recovery_required_count += 1;
                    }
                    Ok(_) => fenced_count += 1,
                    Err(_) => recovery_required_count += 1,
                }
            }
            active.clear();
            let _ = reply.send(Ok(WorkflowQueueShutdownReport {
                fenced_count,
                recovery_required_count,
            }));
            true
        }
    }
}
