//! Bounded content writes, piece hashing, durability checkpoints, and exact shutdown.
//!
//! The pipeline owns its command and completion queues, independent write
//! and hash jobs, checkpoint task, cancellation, and joins. It returns the
//! same selective-storage owner to the driver only after shutdown completes.

use std::collections::{BTreeMap, VecDeque};
use std::sync::Arc;
use std::time::{Duration, Instant};

use rstorrent_protocol::peer_wire::MAX_REQUEST_BLOCK_LENGTH;
use rstorrent_protocol::storage_layout::LayoutError;
use tokio::sync::{OwnedSemaphorePermit, Semaphore, mpsc};
use tokio::task::{JoinHandle, JoinSet};
use tokio::time::timeout;
use tokio_util::sync::CancellationToken;

use super::control::StorageCommandKind;
use super::{DownloadActivityEvent, DownloadCheckpointSink, DownloadControl, DownloadError};
use crate::checkpoint::{
    CheckpointAdmission, CheckpointBatch, CheckpointBatchState, CheckpointIntent, CheckpointPolicy,
    DurabilityTarget,
};
use crate::selective_storage::{
    CheckpointHandles, SelectiveHashPlan, SelectiveStorage, SelectiveWriteJob,
};
use crate::swarm::{BlockKey, PieceGeneration, SwarmError};

const CONTENT_STORAGE_PENDING_QUEUE: usize = 2;
pub(super) const CONTENT_STORAGE_WRITE_BATCH_BLOCKS: usize = 16;
pub(super) const CONTENT_STORAGE_WRITE_BATCH_BYTES: usize = 256 * 1024;
const CHECKPOINT_MAX_AGE: Duration = Duration::from_secs(2);
pub(super) const CHECKPOINT_MAX_DIRTY_BYTES: u64 = 64 * 1024 * 1024;
const CHECKPOINT_MAX_PIECES: usize = 256;
const CHECKPOINT_INTENT_CAPACITY: usize = 256;
const CHECKPOINT_SYNC_CONCURRENCY: usize = 4;
const CHECKPOINT_BYTE_UNIT: u64 = MAX_REQUEST_BLOCK_LENGTH as u64;

pub(super) const fn content_storage_job_limit(max_buffered_payload_bytes: usize) -> usize {
    max_buffered_payload_bytes / MAX_REQUEST_BLOCK_LENGTH as usize
}

pub(super) struct ContentStorage(pub(super) Box<SelectiveStorage>);

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(super) struct ContentWriteStats {
    pub(super) selected_bytes: usize,
    pub(super) part_bytes: usize,
}

pub(super) enum ContentStorageCommand {
    Write {
        block: BlockKey,
        generation: PieceGeneration,
        offset: u64,
        bytes: Vec<u8>,
    },
    Verify {
        piece: u32,
        generation: PieceGeneration,
        length: u32,
        expected: [u8; 20],
        durable: bool,
    },
}

impl ContentStorageCommand {
    fn kind(&self) -> StorageCommandKind {
        match self {
            Self::Write { .. } => StorageCommandKind::Write,
            Self::Verify { .. } => StorageCommandKind::Hash,
        }
    }

    pub(super) fn write_bytes(&self) -> Option<usize> {
        match self {
            Self::Write { bytes, .. } => Some(bytes.len()),
            Self::Verify { .. } => None,
        }
    }
}

pub(super) struct QueuedContentStorageCommand {
    pub(super) enqueued_at: Instant,
    pub(super) command: ContentStorageCommand,
}

pub(super) struct PreparedContentWrite {
    pub(super) block: BlockKey,
    pub(super) generation: PieceGeneration,
    pub(super) offset: u64,
    pub(super) bytes: Vec<u8>,
    pub(super) stats: ContentWriteStats,
}

pub(super) struct ContentWriteMember {
    block: BlockKey,
    generation: PieceGeneration,
    stats: ContentWriteStats,
}

pub(super) struct CoalescedContentWrite {
    pub(super) piece: u32,
    pub(super) begin: u32,
    pub(super) offset: u64,
    pub(super) bytes: Vec<u8>,
    pub(super) members: Vec<ContentWriteMember>,
}

struct ContentWriteOperation(SelectiveWriteJob);

impl ContentWriteOperation {
    async fn execute(self) -> Result<(), DownloadError> {
        self.0
            .execute()
            .await
            .map(|_| ())
            .map_err(DownloadError::SelectiveStorage)
    }
}

struct PreparedPhysicalContentWrite {
    operation: ContentWriteOperation,
    members: Vec<ContentWriteMember>,
}

struct ContentWriteJob {
    writes: Vec<PreparedPhysicalContentWrite>,
}

struct ContentHashOperation(SelectiveHashPlan);

struct ContentHashJob {
    piece: u32,
    generation: PieceGeneration,
    length: u32,
    expected: [u8; 20],
    durable: bool,
    durability_targets: Vec<DurabilityTarget>,
    operation: ContentHashOperation,
}

struct ContentHashJobResult {
    piece: u32,
    generation: PieceGeneration,
    length: u32,
    expected: [u8; 20],
    durable: bool,
    durability_targets: Vec<DurabilityTarget>,
    result: Result<[u8; 20], DownloadError>,
}

enum ContentStorageJobResult {
    Write {
        started_at: Instant,
        blocks: Vec<BlockKey>,
        bytes: usize,
        completions: Vec<ContentStorageCompletion>,
    },
    Hash {
        started_at: Instant,
        result: ContentHashJobResult,
    },
}

pub(super) enum ContentStorageCompletion {
    Write {
        block: BlockKey,
        generation: PieceGeneration,
        result: Result<ContentWriteStats, DownloadError>,
    },
    Verify {
        piece: u32,
        generation: PieceGeneration,
        length: u32,
        result: Result<ContentVerification, DownloadError>,
    },
}

pub(super) struct ContentVerification {
    pub(super) actual: [u8; 20],
    pub(super) durability_targets: Vec<DurabilityTarget>,
}

pub(super) struct PendingCheckpointIntent {
    intent: CheckpointIntent,
    permit: CheckpointPermit,
}

struct CheckpointPermit {
    _item: OwnedSemaphorePermit,
    _bytes: OwnedSemaphorePermit,
}

pub(super) struct ContentCheckpointPipeline {
    pub(super) intents: Option<mpsc::Sender<PendingCheckpointIntent>>,
    item_capacity: Arc<Semaphore>,
    byte_capacity: Arc<Semaphore>,
    pub(super) failures: mpsc::Receiver<String>,
    pub(super) task: JoinHandle<Result<(), DownloadError>>,
    started_at: Instant,
}

impl ContentCheckpointPipeline {
    pub(super) fn start(
        handles: CheckpointHandles,
        checkpoints: Arc<dyn DownloadCheckpointSink>,
        control: DownloadControl,
    ) -> Result<Self, DownloadError> {
        let policy = CheckpointPolicy::new(
            CHECKPOINT_MAX_AGE,
            CHECKPOINT_MAX_DIRTY_BYTES,
            CHECKPOINT_MAX_PIECES,
        )
        .map_err(|error| DownloadError::StorageTask(error.to_string()))?;
        let (intent_sender, intent_receiver) = mpsc::channel(CHECKPOINT_INTENT_CAPACITY);
        let (failure_sender, failure_receiver) = mpsc::channel(1);
        let item_capacity = Arc::new(Semaphore::new(CHECKPOINT_INTENT_CAPACITY));
        let byte_permits = CHECKPOINT_MAX_DIRTY_BYTES / CHECKPOINT_BYTE_UNIT;
        let byte_capacity = Arc::new(Semaphore::new(
            usize::try_from(byte_permits)
                .map_err(|_| DownloadError::StorageTask("checkpoint byte bound overflow".into()))?,
        ));
        let started_at = Instant::now();
        let task_started_at = started_at;
        let task = tokio::spawn(async move {
            let result = run_content_checkpoint_task(
                intent_receiver,
                handles,
                checkpoints,
                policy,
                task_started_at,
                control,
            )
            .await;
            if let Err(error) = &result {
                let _ = failure_sender.try_send(error.to_string());
            }
            result
        });
        Ok(Self {
            intents: Some(intent_sender),
            item_capacity,
            byte_capacity,
            failures: failure_receiver,
            task,
            started_at,
        })
    }

    pub(super) async fn enqueue(
        &self,
        piece_index: usize,
        length: u32,
        targets: Vec<DurabilityTarget>,
    ) -> Result<(), DownloadError> {
        let item = self
            .item_capacity
            .clone()
            .acquire_owned()
            .await
            .map_err(|_| DownloadError::StorageTask("checkpoint item owner closed".to_owned()))?;
        let length_u64 = u64::from(length);
        let units = length_u64.div_ceil(CHECKPOINT_BYTE_UNIT);
        let maximum_units = CHECKPOINT_MAX_DIRTY_BYTES / CHECKPOINT_BYTE_UNIT;
        let units = units.min(maximum_units);
        let units = u32::try_from(units)
            .map_err(|_| DownloadError::StorageTask("checkpoint byte charge overflow".into()))?;
        let bytes = self
            .byte_capacity
            .clone()
            .acquire_many_owned(units)
            .await
            .map_err(|_| DownloadError::StorageTask("checkpoint byte owner closed".to_owned()))?;
        let intent =
            CheckpointIntent::new(piece_index, length_u64, self.started_at.elapsed(), targets)
                .map_err(|error| DownloadError::StorageTask(error.to_string()))?;
        let sender = self.intents.as_ref().ok_or_else(|| {
            DownloadError::StorageTask("checkpoint intent owner is stopped".to_owned())
        })?;
        sender
            .send(PendingCheckpointIntent {
                intent,
                permit: CheckpointPermit {
                    _item: item,
                    _bytes: bytes,
                },
            })
            .await
            .map_err(|_| DownloadError::StorageTask("checkpoint intent channel closed".to_owned()))
    }
}

pub(super) struct ContentStoragePipeline {
    commands: Option<mpsc::Sender<QueuedContentStorageCommand>>,
    completions: mpsc::Receiver<ContentStorageCompletion>,
    cancellation: CancellationToken,
    task: JoinHandle<Result<ContentStorage, DownloadError>>,
    pending_commands: VecDeque<QueuedContentStorageCommand>,
    control: DownloadControl,
    max_buffered_payload_bytes: usize,
    job_limit: usize,
    queue_capacity: usize,
    checkpoint: Option<ContentCheckpointPipeline>,
}

impl ContentStoragePipeline {
    pub(super) async fn start(
        mut storage: ContentStorage,
        control: &DownloadControl,
        max_buffered_payload_bytes: usize,
        checkpoints: Option<Arc<dyn DownloadCheckpointSink>>,
    ) -> Result<Self, DownloadError> {
        let checkpoint = match checkpoints {
            Some(checkpoints) => {
                let handles = storage
                    .0
                    .checkpoint_handles()
                    .await
                    .map_err(DownloadError::SelectiveStorage)?;
                Some(ContentCheckpointPipeline::start(
                    handles,
                    checkpoints,
                    control.clone(),
                )?)
            }
            None => None,
        };
        control.configure_disk_runtime(max_buffered_payload_bytes);
        let job_limit = content_storage_job_limit(max_buffered_payload_bytes);
        debug_assert_ne!(job_limit, 0);
        let queue_capacity = job_limit;
        let (command_sender, command_receiver) = mpsc::channel(queue_capacity);
        let (completion_sender, completion_receiver) = mpsc::channel(queue_capacity);
        let cancellation = CancellationToken::new();
        let task = tokio::spawn(run_content_storage_task(
            storage,
            command_receiver,
            completion_sender,
            cancellation.clone(),
            control.clone(),
            queue_capacity,
        ));
        Ok(Self {
            commands: Some(command_sender),
            completions: completion_receiver,
            cancellation,
            task,
            pending_commands: VecDeque::with_capacity(CONTENT_STORAGE_PENDING_QUEUE),
            control: control.clone(),
            max_buffered_payload_bytes,
            job_limit,
            queue_capacity,
            checkpoint,
        })
    }

    pub(super) fn enqueue(&mut self, command: ContentStorageCommand) -> Result<(), DownloadError> {
        let buffered_bytes = command.write_bytes();
        if buffered_bytes.is_some_and(|bytes| {
            !self
                .control
                .try_buffer_payload(bytes, self.max_buffered_payload_bytes)
        }) {
            return Err(DownloadError::Swarm(SwarmError::Invariant(
                "received payload exceeded the storage buffer allowance",
            )));
        }
        let command = QueuedContentStorageCommand {
            enqueued_at: Instant::now(),
            command,
        };
        if !self.pending_commands.is_empty() {
            if self.pending_commands.len() >= CONTENT_STORAGE_PENDING_QUEUE {
                if let Some(bytes) = buffered_bytes {
                    self.control.abandon_queued_payload(bytes);
                }
                return Err(DownloadError::Swarm(SwarmError::Invariant(
                    "storage pending-command bound exceeded",
                )));
            }
            self.control.storage_job_started();
            self.pending_commands.push_back(command);
            return Ok(());
        }
        self.control.storage_job_started();
        let Some(sender) = &self.commands else {
            self.control.storage_job_finished();
            if let Some(bytes) = buffered_bytes {
                self.control.abandon_queued_payload(bytes);
            }
            return Err(DownloadError::StorageTask(
                "storage command owner is stopped".to_owned(),
            ));
        };
        match sender.try_send(command) {
            Ok(()) => {
                self.control.observe_storage_command_queue(
                    self.queue_capacity.saturating_sub(sender.capacity()),
                );
                Ok(())
            }
            Err(mpsc::error::TrySendError::Full(command)) => {
                self.pending_commands.push_back(command);
                Ok(())
            }
            Err(mpsc::error::TrySendError::Closed(_)) => {
                self.control.storage_job_finished();
                if let Some(bytes) = buffered_bytes {
                    self.control.abandon_queued_payload(bytes);
                }
                Err(DownloadError::StorageTask(
                    "storage command channel closed".to_owned(),
                ))
            }
        }
    }

    pub(super) fn flush_pending(&mut self) -> Result<bool, DownloadError> {
        while let Some(command) = self.pending_commands.pop_front() {
            let Some(sender) = &self.commands else {
                self.pending_commands.push_front(command);
                return Err(DownloadError::StorageTask(
                    "storage command owner is stopped".to_owned(),
                ));
            };
            match sender.try_send(command) {
                Ok(()) => {
                    self.control.observe_storage_command_queue(
                        self.queue_capacity.saturating_sub(sender.capacity()),
                    );
                }
                Err(mpsc::error::TrySendError::Full(command)) => {
                    self.pending_commands.push_front(command);
                    return Ok(false);
                }
                Err(mpsc::error::TrySendError::Closed(command)) => {
                    self.pending_commands.push_front(command);
                    return Err(DownloadError::StorageTask(
                        "storage command channel closed".to_owned(),
                    ));
                }
            }
        }
        Ok(true)
    }

    pub(super) fn is_backpressured(&self) -> bool {
        self.control.storage_backpressured()
            || !self.pending_commands.is_empty()
            || self.control.storage_jobs_at_limit(self.job_limit)
    }

    pub(super) async fn next_completion(
        &mut self,
    ) -> Result<ContentStorageCompletion, DownloadError> {
        let Some(checkpoint) = self.checkpoint.as_mut() else {
            return self.completions.recv().await.ok_or_else(|| {
                DownloadError::StorageTask("storage completion channel closed".to_owned())
            });
        };
        tokio::select! {
            completion = self.completions.recv() => completion.ok_or_else(|| {
                DownloadError::StorageTask("storage completion channel closed".to_owned())
            }),
            failure = checkpoint.failures.recv() => Err(DownloadError::Checkpoint(
                failure.unwrap_or_else(|| "checkpoint task stopped unexpectedly".to_owned())
            )),
        }
    }

    pub(super) fn completion_received(&self, completion: &ContentStorageCompletion) {
        self.control.storage_job_finished();
        if let ContentStorageCompletion::Write { block, .. } = completion {
            self.control.release_buffered_payload(block.length as usize);
        }
    }

    pub(super) async fn enqueue_checkpoint(
        &self,
        piece_index: usize,
        length: u32,
        targets: Vec<DurabilityTarget>,
    ) -> Result<(), DownloadError> {
        self.checkpoint
            .as_ref()
            .ok_or_else(|| {
                DownloadError::StorageTask("resumable storage has no checkpoint owner".to_owned())
            })?
            .enqueue(piece_index, length, targets)
            .await
    }

    pub(super) async fn shutdown(mut self, cancel: bool) -> Result<ContentStorage, DownloadError> {
        self.commands.take();
        if let Some(checkpoint) = self.checkpoint.as_mut() {
            checkpoint.intents.take();
        }
        if cancel {
            self.cancellation.cancel();
        }
        let storage_result = self
            .task
            .await
            .map_err(|error| DownloadError::StorageTask(error.to_string()))
            .and_then(|result| result);
        let checkpoint_result = match self.checkpoint {
            Some(checkpoint) => checkpoint
                .task
                .await
                .map_err(|error| DownloadError::StorageTask(error.to_string()))?,
            None => Ok(()),
        };
        self.control.clear_storage_jobs();
        self.control.clear_buffered_payload();
        match (storage_result, checkpoint_result) {
            (Ok(storage), Ok(())) => Ok(storage),
            (Err(error), Ok(())) | (Ok(_), Err(error)) => Err(error),
            (Err(error), Err(checkpoint)) => Err(DownloadError::StorageTask(format!(
                "{error}; additionally {checkpoint}"
            ))),
        }
    }
}

async fn run_content_checkpoint_task(
    mut intents: mpsc::Receiver<PendingCheckpointIntent>,
    handles: CheckpointHandles,
    checkpoints: Arc<dyn DownloadCheckpointSink>,
    policy: CheckpointPolicy,
    started_at: Instant,
    control: DownloadControl,
) -> Result<(), DownloadError> {
    let mut state = CheckpointBatchState::new(policy);
    let mut permits = BTreeMap::new();
    let mut pending = None;
    loop {
        let next = match pending.take() {
            Some(intent) => Some(intent),
            None if state.len() == 0 => intents.recv().await,
            None => {
                let wait = state.next_flush_in(started_at.elapsed()).ok_or_else(|| {
                    DownloadError::StorageTask(
                        "nonempty checkpoint batch has no age deadline".to_owned(),
                    )
                })?;
                match timeout(wait, intents.recv()).await {
                    Ok(intent) => intent,
                    Err(_) => {
                        flush_content_checkpoint(
                            &mut state,
                            &mut permits,
                            &handles,
                            &checkpoints,
                            &control,
                        )
                        .await?;
                        continue;
                    }
                }
            }
        };
        let Some(mut next) = next else {
            flush_content_checkpoint(&mut state, &mut permits, &handles, &checkpoints, &control)
                .await?;
            return Ok(());
        };
        let piece_index = next.intent.piece_index;
        match state
            .admit(next.intent, started_at.elapsed())
            .map_err(|error| DownloadError::StorageTask(error.to_string()))?
        {
            CheckpointAdmission::Accumulating => {
                if permits.insert(piece_index, next.permit).is_some() {
                    return Err(DownloadError::StorageTask(
                        "checkpoint permit piece is duplicated".to_owned(),
                    ));
                }
            }
            CheckpointAdmission::Ready(_) => {
                if permits.insert(piece_index, next.permit).is_some() {
                    return Err(DownloadError::StorageTask(
                        "checkpoint permit piece is duplicated".to_owned(),
                    ));
                }
                flush_content_checkpoint(
                    &mut state,
                    &mut permits,
                    &handles,
                    &checkpoints,
                    &control,
                )
                .await?;
            }
            CheckpointAdmission::FlushBefore { intent, .. } => {
                flush_content_checkpoint(
                    &mut state,
                    &mut permits,
                    &handles,
                    &checkpoints,
                    &control,
                )
                .await?;
                next.intent = intent;
                pending = Some(next);
            }
        }
    }
}

async fn flush_content_checkpoint(
    state: &mut CheckpointBatchState,
    permits: &mut BTreeMap<usize, CheckpointPermit>,
    handles: &CheckpointHandles,
    checkpoints: &Arc<dyn DownloadCheckpointSink>,
    control: &DownloadControl,
) -> Result<(), DownloadError> {
    let expected_dirty_bytes = state.dirty_bytes();
    let Some(batch) = state.take() else {
        return Ok(());
    };
    let actual_dirty_bytes = batch.intents.iter().try_fold(0_u64, |total, intent| {
        total.checked_add(intent.length).ok_or_else(|| {
            DownloadError::StorageTask("checkpoint batch byte sum overflow".to_owned())
        })
    })?;
    let oldest_verified_at = batch
        .intents
        .iter()
        .map(|intent| intent.verified_at)
        .min()
        .ok_or_else(|| DownloadError::StorageTask("checkpoint batch is empty".to_owned()))?;
    if actual_dirty_bytes != batch.dirty_bytes
        || actual_dirty_bytes != expected_dirty_bytes
        || oldest_verified_at != batch.oldest_verified_at
    {
        return Err(DownloadError::StorageTask(
            "checkpoint batch accounting diverged".to_owned(),
        ));
    }
    let mut batch_permits = Vec::with_capacity(batch.intents.len());
    let mut piece_indices = Vec::with_capacity(batch.intents.len());
    for intent in &batch.intents {
        piece_indices.push(intent.piece_index);
        batch_permits.push(permits.remove(&intent.piece_index).ok_or_else(|| {
            DownloadError::StorageTask(format!(
                "checkpoint piece {} has no capacity permit",
                intent.piece_index
            ))
        })?);
    }
    if !permits.is_empty() {
        return Err(DownloadError::StorageTask(
            "checkpoint permits escaped their batch".to_owned(),
        ));
    }
    control.disk_checkpoint_sync_started(&batch);
    let sync_started = Instant::now();
    control.wait_before_checkpoint_sync().await;
    let sync_result = if control.take_checkpoint_sync_failure() {
        Err(DownloadError::Checkpoint(
            "injected checkpoint sync failure".to_owned(),
        ))
    } else {
        sync_checkpoint_targets(handles, &batch).await
    };
    if let Err(error) = sync_result {
        control.disk_checkpoint_failed(&batch, sync_started.elapsed(), &error.to_string());
        return Err(error);
    }
    control.disk_checkpoint_sync_completed(&batch, sync_started.elapsed());
    let commit_started = Instant::now();
    control.wait_before_checkpoint_commit().await;
    let checkpoints = checkpoints.clone();
    let commit_result =
        tokio::task::spawn_blocking(move || checkpoints.pieces_durable(&piece_indices))
            .await
            .map_err(|error| DownloadError::StorageTask(error.to_string()))
            .and_then(|result| result.map_err(DownloadError::Checkpoint));
    if let Err(error) = commit_result {
        control.disk_checkpoint_failed(&batch, commit_started.elapsed(), &error.to_string());
        return Err(error);
    }
    control.disk_checkpoint_completed(&batch, commit_started.elapsed());
    drop(batch_permits);
    Ok(())
}

async fn sync_checkpoint_targets(
    handles: &CheckpointHandles,
    batch: &CheckpointBatch,
) -> Result<(), DownloadError> {
    let references = batch
        .targets
        .iter()
        .copied()
        .map(|target| {
            handles
                .get(&target)
                .and_then(|handle| handle.get())
                .cloned()
                .map_or_else(
                    || {
                        Err(DownloadError::StorageTask(format!(
                            "checkpoint target {target:?} has no sync handle"
                        )))
                    },
                    |reference| Ok((target, reference)),
                )
        })
        .collect::<Result<Vec<_>, _>>()?;
    let mut targets = Vec::with_capacity(references.len());
    for (target, reference) in references {
        let file = reference
            .acquire()
            .await
            .map_err(DownloadError::SelectiveStorage)?;
        targets.push((target, file));
    }
    let mut targets = targets.into_iter();
    let mut running = JoinSet::new();
    let mut first_error = None;
    loop {
        while first_error.is_none() && running.len() < CHECKPOINT_SYNC_CONCURRENCY {
            let Some((target, file)) = targets.next() else {
                break;
            };
            running.spawn_blocking(move || (target, file.file().sync_data()));
        }
        let Some(result) = running.join_next().await else {
            break;
        };
        match result {
            Ok((_target, Ok(()))) => {}
            Ok((target, Err(error))) => {
                first_error.get_or_insert_with(|| {
                    DownloadError::StorageTask(format!(
                        "checkpoint target {target:?} sync failed: {error}"
                    ))
                });
            }
            Err(error) => {
                first_error.get_or_insert_with(|| DownloadError::StorageTask(error.to_string()));
            }
        }
    }
    first_error.map_or(Ok(()), Err)
}

async fn run_content_storage_task(
    mut storage: ContentStorage,
    mut commands: mpsc::Receiver<QueuedContentStorageCommand>,
    completions: mpsc::Sender<ContentStorageCompletion>,
    cancellation: CancellationToken,
    control: DownloadControl,
    queue_capacity: usize,
) -> Result<ContentStorage, DownloadError> {
    let mut ready_writes = VecDeque::new();
    let mut ready_hashes = VecDeque::new();
    let mut pending_completions = VecDeque::new();
    let mut running = JoinSet::new();
    let mut active_writes = 0_usize;
    let mut active_hashes = 0_usize;
    let mut commands_closed = false;
    let mut cancelled = false;
    let (write_concurrency, hash_concurrency) = control.storage_execution_limits();

    loop {
        if !cancelled {
            loop {
                match commands.try_recv() {
                    Ok(command) => {
                        queue_content_storage_command(command, &mut ready_writes, &mut ready_hashes)
                    }
                    Err(mpsc::error::TryRecvError::Empty) => break,
                    Err(mpsc::error::TryRecvError::Disconnected) => {
                        commands_closed = true;
                        break;
                    }
                }
            }
            flush_ready_content_storage_completions(
                &completions,
                &mut pending_completions,
                &control,
                queue_capacity,
            )?;
        }

        while !cancelled && active_writes < write_concurrency && !ready_writes.is_empty() {
            let batch = collect_ready_content_write_batch(&mut ready_writes);
            let block_keys = batch
                .iter()
                .filter_map(|command| match &command.command {
                    ContentStorageCommand::Write { block, .. } => Some(*block),
                    ContentStorageCommand::Verify { .. } => None,
                })
                .collect::<Vec<_>>();
            let bytes = batch.iter().fold(0_usize, |total, command| {
                total.saturating_add(command.command.write_bytes().unwrap_or(0))
            });
            let enqueued_at = batch[0].enqueued_at;
            let started_at = Instant::now();
            control.storage_write_batch_started(enqueued_at, started_at, &block_keys, bytes);
            match prepare_content_storage_writes(&mut storage, batch).await {
                Ok(job) => {
                    active_writes += 1;
                    let job_control = control.clone();
                    running.spawn(async move {
                        let _session_permit = job_control.wait_before_storage().await;
                        ContentStorageJobResult::Write {
                            started_at,
                            blocks: block_keys,
                            bytes,
                            completions: execute_content_write_job(job).await,
                        }
                    });
                }
                Err(failed) => {
                    control.storage_write_batch_completed(
                        started_at,
                        Instant::now(),
                        &block_keys,
                        bytes,
                    );
                    pending_completions.extend(failed);
                }
            }
        }

        while !cancelled && active_hashes < hash_concurrency && !ready_hashes.is_empty() {
            let command = ready_hashes
                .pop_front()
                .expect("nonempty hash-ready queue has a command");
            let ContentStorageCommand::Verify { piece, length, .. } = &command.command else {
                unreachable!("hash-ready queue contains only verify commands");
            };
            control.disk_piece_hashing(*piece, *length);
            control.emit(DownloadActivityEvent::PieceHashing {
                piece_index: *piece,
            });
            let started_at = Instant::now();
            control.storage_command_started(
                StorageCommandKind::Hash,
                command.enqueued_at,
                started_at,
            );
            match prepare_content_storage_hash(&storage, command.command) {
                Ok(job) => {
                    active_hashes += 1;
                    let job_control = control.clone();
                    running.spawn(async move {
                        let _session_permit = job_control.wait_before_storage_hash().await;
                        ContentStorageJobResult::Hash {
                            started_at,
                            result: execute_content_hash_job(job).await,
                        }
                    });
                }
                Err(failed) => {
                    control.storage_command_completed(
                        StorageCommandKind::Hash,
                        started_at,
                        Instant::now(),
                    );
                    pending_completions.push_back(failed);
                }
            }
        }

        if cancelled {
            ready_writes.clear();
            ready_hashes.clear();
            pending_completions.clear();
            while let Some(joined) = running.join_next().await {
                complete_cancelled_content_storage_job(
                    joined.map_err(|error| DownloadError::StorageTask(error.to_string()))?,
                    &control,
                );
            }
            return Ok(storage);
        }

        if commands_closed
            && ready_writes.is_empty()
            && ready_hashes.is_empty()
            && running.is_empty()
            && pending_completions.is_empty()
        {
            return Ok(storage);
        }

        tokio::select! {
            biased;
            _ = cancellation.cancelled() => {
                cancelled = true;
                commands.close();
            }
            joined = running.join_next(), if !running.is_empty() => {
                let result = joined
                    .expect("nonempty storage job set has a completion")
                    .map_err(|error| DownloadError::StorageTask(error.to_string()))?;
                match result {
                    ContentStorageJobResult::Write {
                        started_at,
                        blocks,
                        bytes,
                        completions: completed,
                    } => {
                        active_writes = active_writes.checked_sub(1).ok_or_else(|| {
                            DownloadError::StorageTask("active write job underflow".to_owned())
                        })?;
                        control.storage_write_batch_completed(
                            started_at,
                            Instant::now(),
                            &blocks,
                            bytes,
                        );
                        pending_completions.extend(completed);
                    }
                    ContentStorageJobResult::Hash { started_at, result } => {
                        active_hashes = active_hashes.checked_sub(1).ok_or_else(|| {
                            DownloadError::StorageTask("active hash job underflow".to_owned())
                        })?;
                        control.storage_command_completed(
                            StorageCommandKind::Hash,
                            started_at,
                            Instant::now(),
                        );
                        pending_completions.push_back(finish_content_hash_job(
                            &mut storage,
                            result,
                            &control,
                        ));
                    }
                }
            }
            permit = completions.reserve(), if !pending_completions.is_empty() => {
                let permit = permit.map_err(|_| {
                    DownloadError::StorageTask("storage completion channel closed".to_owned())
                })?;
                let projected_depth = queue_capacity
                    .saturating_sub(completions.capacity())
                    .saturating_add(1)
                    .min(queue_capacity);
                control.observe_storage_completion_queue(projected_depth);
                permit.send(
                    pending_completions
                        .pop_front()
                        .expect("reserved completion has a pending value"),
                );
            }
            command = commands.recv(), if !commands_closed => {
                match command {
                    Some(command) => queue_content_storage_command(
                        command,
                        &mut ready_writes,
                        &mut ready_hashes,
                    ),
                    None => commands_closed = true,
                }
            }
        }
    }
}

fn queue_content_storage_command(
    command: QueuedContentStorageCommand,
    writes: &mut VecDeque<QueuedContentStorageCommand>,
    hashes: &mut VecDeque<QueuedContentStorageCommand>,
) {
    match command.command.kind() {
        StorageCommandKind::Write => writes.push_back(command),
        StorageCommandKind::Hash => hashes.push_back(command),
    }
}

fn flush_ready_content_storage_completions(
    completions: &mpsc::Sender<ContentStorageCompletion>,
    pending: &mut VecDeque<ContentStorageCompletion>,
    control: &DownloadControl,
    queue_capacity: usize,
) -> Result<(), DownloadError> {
    while let Some(completion) = pending.pop_front() {
        match completions.try_send(completion) {
            Ok(()) => {
                let depth = queue_capacity
                    .saturating_sub(completions.capacity())
                    .min(queue_capacity);
                control.observe_storage_completion_queue(depth);
            }
            Err(mpsc::error::TrySendError::Full(completion)) => {
                pending.push_front(completion);
                break;
            }
            Err(mpsc::error::TrySendError::Closed(_)) => {
                return Err(DownloadError::StorageTask(
                    "storage completion channel closed".to_owned(),
                ));
            }
        }
    }
    Ok(())
}

fn collect_ready_content_write_batch(
    writes: &mut VecDeque<QueuedContentStorageCommand>,
) -> Vec<QueuedContentStorageCommand> {
    let first = writes
        .pop_front()
        .expect("nonempty write-ready queue has a command");
    let mut bytes = first.command.write_bytes().unwrap_or(0);
    let mut batch = Vec::with_capacity(CONTENT_STORAGE_WRITE_BATCH_BLOCKS);
    batch.push(first);
    while batch.len() < CONTENT_STORAGE_WRITE_BATCH_BLOCKS {
        let Some(next_bytes) = writes
            .front()
            .and_then(|command| command.command.write_bytes())
        else {
            break;
        };
        let Some(projected) = bytes.checked_add(next_bytes) else {
            break;
        };
        if projected > CONTENT_STORAGE_WRITE_BATCH_BYTES {
            break;
        }
        bytes = projected;
        batch.push(
            writes
                .pop_front()
                .expect("inspected write-ready command remains present"),
        );
    }
    batch
}

fn complete_cancelled_content_storage_job(
    result: ContentStorageJobResult,
    control: &DownloadControl,
) {
    match result {
        ContentStorageJobResult::Write {
            started_at,
            blocks,
            bytes,
            ..
        } => control.storage_write_batch_completed(started_at, Instant::now(), &blocks, bytes),
        ContentStorageJobResult::Hash { started_at, .. } => {
            control.storage_command_completed(StorageCommandKind::Hash, started_at, Instant::now())
        }
    }
}

#[cfg(test)]
pub(super) fn collect_content_write_batch(
    first: QueuedContentStorageCommand,
    commands: &mut mpsc::Receiver<QueuedContentStorageCommand>,
    deferred: &mut Option<QueuedContentStorageCommand>,
) -> Vec<QueuedContentStorageCommand> {
    debug_assert!(first.command.write_bytes().is_some());
    let mut bytes = first.command.write_bytes().unwrap_or(0);
    let mut batch = Vec::with_capacity(CONTENT_STORAGE_WRITE_BATCH_BLOCKS);
    batch.push(first);

    while batch.len() < CONTENT_STORAGE_WRITE_BATCH_BLOCKS {
        let next = match commands.try_recv() {
            Ok(command) => command,
            Err(mpsc::error::TryRecvError::Empty | mpsc::error::TryRecvError::Disconnected) => {
                break;
            }
        };
        let Some(next_bytes) = next.command.write_bytes() else {
            *deferred = Some(next);
            break;
        };
        let Some(projected) = bytes.checked_add(next_bytes) else {
            *deferred = Some(next);
            break;
        };
        if projected > CONTENT_STORAGE_WRITE_BATCH_BYTES {
            *deferred = Some(next);
            break;
        }
        bytes = projected;
        batch.push(next);
    }
    batch
}

#[cfg(test)]
pub(super) async fn execute_content_storage_writes(
    storage: &mut ContentStorage,
    commands: Vec<QueuedContentStorageCommand>,
    control: &DownloadControl,
) -> Vec<ContentStorageCompletion> {
    let _session_permit = control.wait_before_storage().await;
    match prepare_content_storage_writes(storage, commands).await {
        Ok(job) => execute_content_write_job(job).await,
        Err(completions) => completions,
    }
}

async fn prepare_content_storage_writes(
    storage: &mut ContentStorage,
    commands: Vec<QueuedContentStorageCommand>,
) -> Result<ContentWriteJob, Vec<ContentStorageCompletion>> {
    let mut prepared = Vec::with_capacity(commands.len());
    for command in commands {
        let ContentStorageCommand::Write {
            block,
            generation,
            offset,
            bytes,
        } = command.command
        else {
            unreachable!("write batches contain only write commands");
        };
        let stats = storage
            .0
            .write_stats(block.piece, block.begin, bytes.len())
            .map(|stats| ContentWriteStats {
                selected_bytes: stats.wanted_bytes,
                part_bytes: stats.skipped_bytes,
            })
            .map_err(DownloadError::SelectiveStorage);
        let stats = match stats {
            Ok(stats) => stats,
            Err(error) => {
                return Err(failed_content_write_batch(
                    block,
                    generation,
                    error,
                    prepared.into_iter(),
                ));
            }
        };
        prepared.push(PreparedContentWrite {
            block,
            generation,
            offset,
            bytes,
            stats,
        });
    }

    let writes = match coalesce_content_writes(prepared) {
        Ok(writes) => writes,
        Err((block, generation, error)) => {
            return Err(vec![ContentStorageCompletion::Write {
                block,
                generation,
                result: Err(error),
            }]);
        }
    };
    let mut physical = Vec::with_capacity(writes.len());
    let mut writes = writes.into_iter();
    while let Some(write) = writes.next() {
        let operation = storage
            .0
            .prepare_write(write.piece, write.begin, write.bytes)
            .await
            .map(ContentWriteOperation)
            .map_err(DownloadError::SelectiveStorage);
        match operation {
            Ok(operation) => physical.push(PreparedPhysicalContentWrite {
                operation,
                members: write.members,
            }),
            Err(error) => {
                let mut members = write.members.into_iter();
                let first = members
                    .next()
                    .expect("coalesced write retains at least one logical member");
                let mut completions = vec![ContentStorageCompletion::Write {
                    block: first.block,
                    generation: first.generation,
                    result: Err(error),
                }];
                completions.extend(members.map(failed_prepared_content_write));
                completions.extend(
                    physical
                        .into_iter()
                        .flat_map(|write| write.members)
                        .map(failed_prepared_content_write),
                );
                completions.extend(
                    writes
                        .flat_map(|write| write.members)
                        .map(failed_prepared_content_write),
                );
                return Err(completions);
            }
        }
    }
    Ok(ContentWriteJob { writes: physical })
}

async fn execute_content_write_job(job: ContentWriteJob) -> Vec<ContentStorageCompletion> {
    let mut completed = Vec::new();
    for write in job.writes {
        let result = write.operation.execute().await;
        if let Err(error) = result {
            let mut members = write.members.into_iter();
            let first = members
                .next()
                .expect("coalesced write retains at least one logical member");
            completed.push(ContentStorageCompletion::Write {
                block: first.block,
                generation: first.generation,
                result: Err(error),
            });
            completed.extend(members.map(|member| ContentStorageCompletion::Write {
                block: member.block,
                generation: member.generation,
                result: Err(DownloadError::StorageTask(
                    "coalesced physical write failed".to_owned(),
                )),
            }));
            return completed;
        }
        completed.extend(
            write
                .members
                .into_iter()
                .map(|member| ContentStorageCompletion::Write {
                    block: member.block,
                    generation: member.generation,
                    result: Ok(member.stats),
                }),
        );
    }
    completed
}

fn failed_prepared_content_write(member: ContentWriteMember) -> ContentStorageCompletion {
    ContentStorageCompletion::Write {
        block: member.block,
        generation: member.generation,
        result: Err(DownloadError::StorageTask(
            "coalesced write batch preparation failed".to_owned(),
        )),
    }
}

fn failed_content_write_batch(
    failed_block: BlockKey,
    failed_generation: PieceGeneration,
    error: DownloadError,
    prepared: impl Iterator<Item = PreparedContentWrite>,
) -> Vec<ContentStorageCompletion> {
    let mut completions = vec![ContentStorageCompletion::Write {
        block: failed_block,
        generation: failed_generation,
        result: Err(error),
    }];
    completions.extend(prepared.map(|write| ContentStorageCompletion::Write {
        block: write.block,
        generation: write.generation,
        result: Err(DownloadError::StorageTask(
            "coalesced write batch validation failed".to_owned(),
        )),
    }));
    completions
}

pub(super) fn coalesce_content_writes(
    mut writes: Vec<PreparedContentWrite>,
) -> Result<Vec<CoalescedContentWrite>, (BlockKey, PieceGeneration, DownloadError)> {
    writes.sort_unstable_by_key(|write| (write.block.piece, write.block.begin));
    let mut coalesced: Vec<CoalescedContentWrite> = Vec::with_capacity(writes.len());
    for write in writes {
        if let Some(previous) = coalesced.last_mut()
            && previous.piece == write.block.piece
        {
            let previous_piece_end = previous
                .begin
                .checked_add(u32::try_from(previous.bytes.len()).unwrap_or(u32::MAX));
            if previous_piece_end.is_some_and(|end| write.block.begin < end) {
                return Err((
                    write.block,
                    write.generation,
                    DownloadError::StorageTask(
                        "overlapping logical writes entered one storage batch".to_owned(),
                    ),
                ));
            }
            let previous_file_end = previous.offset.checked_add(previous.bytes.len() as u64);
            if previous_piece_end == Some(write.block.begin)
                && previous_file_end == Some(write.offset)
            {
                previous.bytes.extend_from_slice(&write.bytes);
                previous.members.push(ContentWriteMember {
                    block: write.block,
                    generation: write.generation,
                    stats: write.stats,
                });
                continue;
            }
        }
        coalesced.push(CoalescedContentWrite {
            piece: write.block.piece,
            begin: write.block.begin,
            offset: write.offset,
            bytes: write.bytes,
            members: vec![ContentWriteMember {
                block: write.block,
                generation: write.generation,
                stats: write.stats,
            }],
        });
    }
    Ok(coalesced)
}

#[cfg(test)]
pub(super) async fn execute_content_storage_verification(
    storage: &mut ContentStorage,
    command: ContentStorageCommand,
    control: &DownloadControl,
) -> ContentStorageCompletion {
    let job = match prepare_content_storage_hash(storage, command) {
        Ok(job) => job,
        Err(completion) => return completion,
    };
    let _session_permit = control.wait_before_storage_hash().await;
    let result = execute_content_hash_job(job).await;
    finish_content_hash_job(storage, result, control)
}

fn prepare_content_storage_hash(
    storage: &ContentStorage,
    command: ContentStorageCommand,
) -> Result<ContentHashJob, ContentStorageCompletion> {
    let ContentStorageCommand::Verify {
        piece,
        generation,
        length,
        expected,
        durable,
    } = command
    else {
        unreachable!("write commands execute through the bounded batch path");
    };
    let durability_targets = if durable {
        storage
            .0
            .durability_targets(piece)
            .map_err(DownloadError::SelectiveStorage)
    } else {
        Ok(Vec::new())
    };
    let prepared = durability_targets.and_then(|durability_targets| {
        storage
            .0
            .prepare_hash(piece)
            .map(|operation| (ContentHashOperation(operation), durability_targets))
            .map_err(DownloadError::SelectiveStorage)
    });
    match prepared {
        Ok((operation, durability_targets)) => Ok(ContentHashJob {
            piece,
            generation,
            length,
            expected,
            durable,
            durability_targets,
            operation,
        }),
        Err(error) => Err(ContentStorageCompletion::Verify {
            piece,
            generation,
            length,
            result: Err(error),
        }),
    }
}

async fn execute_content_hash_job(job: ContentHashJob) -> ContentHashJobResult {
    let result = job
        .operation
        .0
        .execute()
        .await
        .map_err(DownloadError::SelectiveStorage);
    ContentHashJobResult {
        piece: job.piece,
        generation: job.generation,
        length: job.length,
        expected: job.expected,
        durable: job.durable,
        durability_targets: job.durability_targets,
        result,
    }
}

fn finish_content_hash_job(
    storage: &mut ContentStorage,
    result: ContentHashJobResult,
    control: &DownloadControl,
) -> ContentStorageCompletion {
    let verification = result.result.and_then(|actual| {
        if actual == result.expected {
            let piece_index = usize::try_from(result.piece)
                .map_err(|_| DownloadError::Layout(LayoutError::ArithmeticOverflow))?;
            storage
                .0
                .record_verified(piece_index)
                .map_err(DownloadError::SelectiveStorage)?;
        }
        Ok(ContentVerification {
            actual,
            durability_targets: if actual == result.expected {
                result.durability_targets
            } else {
                Vec::new()
            },
        })
    });
    if verification
        .as_ref()
        .is_ok_and(|verification| verification.actual == result.expected)
    {
        control.disk_piece_hash_verified(result.piece, result.length, result.durable);
    }
    ContentStorageCompletion::Verify {
        piece: result.piece,
        generation: result.generation,
        length: result.length,
        result: verification,
    }
}
