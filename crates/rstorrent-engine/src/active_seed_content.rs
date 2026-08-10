//! Read-only upload access to verified pieces owned by an active storage pipeline.

use std::error::Error;
use std::fmt;
use std::sync::Arc;
use std::sync::Mutex;

use rstorrent_protocol::peer_wire::BlockRequest;
use tokio::sync::{mpsc, oneshot};

use crate::piece_availability::PieceAvailability;
use crate::selective_storage::{SelectiveStorageError, SelectiveUploadReadPlan};

pub(crate) const ACTIVE_UPLOAD_PLAN_CAPACITY: usize = 16;

pub(crate) struct ActiveUploadPlanRequest {
    pub(crate) request: BlockRequest,
    pub(crate) route_epoch: u64,
    pub(crate) response: oneshot::Sender<Result<SelectiveUploadReadPlan, SelectiveStorageError>>,
}

#[derive(Clone, Debug)]
pub(crate) struct ActiveSeedContent {
    info_hash: [u8; 20],
    private: bool,
    piece_lengths: Arc<[u32]>,
    availability: PieceAvailability,
    planner: Arc<Mutex<mpsc::Sender<ActiveUploadPlanRequest>>>,
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
        let plan = completion
            .await
            .map_err(|_| ActiveSeedContentError::Closed)??;
        let before_read = self.availability.snapshot();
        if before_read.epoch != snapshot.epoch || !before_read.is_available(piece) {
            return Err(ActiveSeedContentError::Unavailable);
        }
        plan.execute().await.map_err(Into::into)
    }
}

#[derive(Debug)]
pub(crate) enum ActiveSeedContentError {
    Closed,
    Unavailable,
    Storage(SelectiveStorageError),
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

impl Error for ActiveSeedContentError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Storage(error) => Some(error),
            Self::Closed | Self::Unavailable => None,
        }
    }
}

impl From<SelectiveStorageError> for ActiveSeedContentError {
    fn from(error: SelectiveStorageError) -> Self {
        Self::Storage(error)
    }
}
