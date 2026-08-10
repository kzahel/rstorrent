//! Read-only upload access to verified pieces owned by an active storage pipeline.

use std::error::Error;
use std::fmt;
use std::sync::Arc;
use std::sync::Mutex;

use rstorrent_protocol::peer_wire::BlockRequest;
use tokio::sync::{mpsc, oneshot};
use tokio_util::sync::CancellationToken;

use crate::piece_availability::PieceAvailability;
use crate::selective_storage::{SelectiveStorageError, SelectiveUploadReadPlan};

pub(crate) const ACTIVE_UPLOAD_PLAN_CAPACITY: usize = 16;

pub(crate) struct ActiveUploadPlanRequest {
    pub(crate) request: BlockRequest,
    pub(crate) route_epoch: u64,
    pub(crate) response: oneshot::Sender<Result<SelectiveUploadReadPlan, SelectiveStorageError>>,
}

#[derive(Clone, Debug)]
pub(crate) struct ActiveUploadFailureSignal {
    inner: Arc<ActiveUploadFailureState>,
}

#[derive(Debug)]
struct ActiveUploadFailureState {
    cancellation: CancellationToken,
    failure: Mutex<Option<ActiveUploadFailure>>,
}

#[derive(Debug)]
struct ActiveUploadFailure {
    piece: u32,
    error: SelectiveStorageError,
}

impl ActiveUploadFailureSignal {
    fn new() -> Self {
        Self {
            inner: Arc::new(ActiveUploadFailureState {
                cancellation: CancellationToken::new(),
                failure: Mutex::new(None),
            }),
        }
    }

    fn report(&self, piece: u32, error: SelectiveStorageError) {
        let mut failure = self
            .inner
            .failure
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if failure.is_none() {
            *failure = Some(ActiveUploadFailure { piece, error });
            self.inner.cancellation.cancel();
        }
    }

    pub(crate) async fn cancelled(&self) {
        self.inner.cancellation.cancelled().await;
    }

    pub(crate) fn take_failure(&self) -> Option<(u32, SelectiveStorageError)> {
        self.inner
            .failure
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .take()
            .map(|failure| (failure.piece, failure.error))
    }
}

#[derive(Clone, Debug)]
pub(crate) struct ActiveSeedContent {
    info_hash: [u8; 20],
    private: bool,
    piece_lengths: Arc<[u32]>,
    availability: PieceAvailability,
    planner: Arc<Mutex<mpsc::Sender<ActiveUploadPlanRequest>>>,
    failure: ActiveUploadFailureSignal,
}

impl ActiveSeedContent {
    pub(crate) fn new(
        info_hash: [u8; 20],
        private: bool,
        piece_lengths: Vec<u32>,
        availability: PieceAvailability,
        planner: mpsc::Sender<ActiveUploadPlanRequest>,
    ) -> Self {
        Self {
            info_hash,
            private,
            piece_lengths: piece_lengths.into(),
            availability,
            planner: Arc::new(Mutex::new(planner)),
            failure: ActiveUploadFailureSignal::new(),
        }
    }

    pub(crate) const fn info_hash(&self) -> [u8; 20] {
        self.info_hash
    }

    pub(crate) const fn is_private(&self) -> bool {
        self.private
    }

    pub(crate) fn piece_lengths(&self) -> Arc<[u32]> {
        self.piece_lengths.clone()
    }

    pub(crate) fn availability(&self) -> PieceAvailability {
        self.availability.clone()
    }

    pub(crate) fn failure_signal(&self) -> ActiveUploadFailureSignal {
        self.failure.clone()
    }

    pub(crate) fn replace_planner(&self, planner: mpsc::Sender<ActiveUploadPlanRequest>) {
        *self
            .planner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = planner;
    }

    pub(crate) async fn read_block(
        &self,
        request: BlockRequest,
    ) -> Result<Vec<u8>, ActiveSeedContentError> {
        let piece =
            usize::try_from(request.index).map_err(|_| ActiveSeedContentError::Unavailable)?;
        let snapshot = self.availability.snapshot();
        if !snapshot.is_available(piece) {
            return Err(ActiveSeedContentError::Unavailable);
        }
        let (response, completion) = oneshot::channel();
        let planner = self
            .planner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone();
        planner
            .send(ActiveUploadPlanRequest {
                request,
                route_epoch: snapshot.epoch,
                response,
            })
            .await
            .map_err(|_| ActiveSeedContentError::Closed)?;
        let plan = match completion
            .await
            .map_err(|_| ActiveSeedContentError::Closed)?
        {
            Ok(plan) => plan,
            Err(error) => {
                return Err(self.classify_storage_failure(request.index, snapshot.epoch, error));
            }
        };
        let before_read = self.availability.snapshot();
        if before_read.epoch != snapshot.epoch || !before_read.is_available(piece) {
            return Err(ActiveSeedContentError::Unavailable);
        }
        match plan.execute().await {
            Ok(block) => Ok(block),
            Err(error) => Err(self.classify_storage_failure(request.index, snapshot.epoch, error)),
        }
    }

    fn classify_storage_failure(
        &self,
        piece: u32,
        expected_epoch: u64,
        error: SelectiveStorageError,
    ) -> ActiveSeedContentError {
        let piece_index = usize::try_from(piece).ok();
        let current = self.availability.snapshot();
        if current.epoch != expected_epoch
            || !piece_index.is_some_and(|piece| current.is_available(piece))
        {
            return ActiveSeedContentError::Unavailable;
        }
        let detail: Arc<str> = error.to_string().into();
        if self
            .availability
            .invalidate_epoch(expected_epoch)
            .unwrap_or(true)
        {
            self.failure.report(piece, error);
        }
        ActiveSeedContentError::Storage(detail)
    }
}

#[derive(Debug)]
pub(crate) enum ActiveSeedContentError {
    Closed,
    Unavailable,
    Storage(Arc<str>),
}

impl fmt::Display for ActiveSeedContentError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Closed => formatter.write_str("active upload storage owner is closed"),
            Self::Unavailable => formatter.write_str("active upload piece is unavailable"),
            Self::Storage(error) => write!(formatter, "active upload storage failed: {error}"),
        }
    }
}

impl Error for ActiveSeedContentError {}
