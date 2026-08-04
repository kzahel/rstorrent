//! Durable eligibility and exact registration ownership for incoming seeding.

use std::collections::BTreeMap;
use std::error::Error;
use std::fmt;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use rstorrent_engine::{
    IncomingPeerError, IncomingPeerHandle, SeedContent, SeedRegistration, SeedRegistrationToken,
    StorageFilePool,
};
use rstorrent_protocol::metainfo::{DURABLE_METAINFO_LIMITS, Metainfo};
use tokio::sync::Mutex;

use crate::control::{StorageState, TorrentState, decode_info_hash};
use crate::store::{ResumeRecord, StorageRootLocation};

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum SeedReconcileOutcome {
    Registered,
    AlreadyRegistered,
    Unregistered,
    Ineligible(&'static str),
    Unavailable(String),
}

#[derive(Clone, Debug)]
pub(crate) struct IncomingSeeding {
    handle: IncomingPeerHandle,
    registrations: Arc<Mutex<BTreeMap<String, SeedRegistrationToken>>>,
    enabled: Arc<AtomicBool>,
}

impl IncomingSeeding {
    pub(crate) fn new(handle: IncomingPeerHandle) -> Self {
        Self {
            handle,
            registrations: Arc::new(Mutex::new(BTreeMap::new())),
            enabled: Arc::new(AtomicBool::new(true)),
        }
    }

    pub(crate) async fn reconcile(
        &self,
        resume: &ResumeRecord,
        catalog_eligible: bool,
        root: Option<&StorageRootLocation>,
        active_download: bool,
        storage_file_pool: &StorageFilePool,
    ) -> Result<SeedReconcileOutcome, IncomingSeedingError> {
        if !self.enabled.load(Ordering::Acquire) {
            return Ok(SeedReconcileOutcome::Ineligible(
                "incoming peer service is stopping",
            ));
        }
        let ineligible = eligibility_reason(resume, catalog_eligible, root, active_download);
        if let Some(reason) = ineligible {
            let removed = self.unregister(&resume.torrent_id).await?;
            return Ok(if removed {
                SeedReconcileOutcome::Unregistered
            } else {
                SeedReconcileOutcome::Ineligible(reason)
            });
        }
        if self
            .registrations
            .lock()
            .await
            .contains_key(&resume.torrent_id)
        {
            return Ok(SeedReconcileOutcome::AlreadyRegistered);
        }
        let raw_info = resume
            .raw_info
            .as_ref()
            .expect("eligible seed has verified metadata");
        let metainfo =
            match Metainfo::from_info_bytes_with_limits(raw_info, DURABLE_METAINFO_LIMITS) {
                Ok(metainfo) => metainfo,
                Err(error) => return Ok(SeedReconcileOutcome::Unavailable(error.to_string())),
            };
        let identity = decode_info_hash(&resume.torrent_id).ok_or_else(|| {
            IncomingSeedingError::InvalidDurableState("invalid torrent identity".to_owned())
        })?;
        if identity != metainfo.info_hash {
            return Ok(SeedReconcileOutcome::Unavailable(
                "stored metadata does not match torrent identity".to_owned(),
            ));
        }
        if resume.publication_name.as_deref() != Some(metainfo.name.as_str()) {
            return Ok(SeedReconcileOutcome::Unavailable(
                "published name does not match verified metadata".to_owned(),
            ));
        }
        let have = resume
            .have
            .as_ref()
            .expect("eligible seed has durable have state");
        if have.pieces().len() != metainfo.piece_count() {
            return Ok(SeedReconcileOutcome::Unavailable(
                "durable have length does not match verified metadata".to_owned(),
            ));
        }
        let skipped = resume
            .skip_files
            .iter()
            .map(|index| {
                usize::try_from(*index).map_err(|_| {
                    IncomingSeedingError::InvalidDurableState(
                        "file selection index overflow".to_owned(),
                    )
                })
            })
            .collect::<Result<Vec<_>, _>>()?;
        let StorageRootLocation::Path(root) = root.expect("eligible seed has a path root") else {
            unreachable!("platform roots are rejected by eligibility")
        };
        storage_file_pool.invalidate_storage(&resume.torrent_id);
        let content =
            match SeedContent::open_published(root, &metainfo, have.pieces(), &skipped).await {
                Ok(content) => content,
                Err(error) => return Ok(SeedReconcileOutcome::Unavailable(error.to_string())),
            };
        let registration = match SeedRegistration::new(raw_info.clone(), content) {
            Ok(registration) => registration,
            Err(error) => return Ok(SeedReconcileOutcome::Unavailable(error.to_string())),
        };
        let token = match self.handle.register(registration).await {
            Ok(token) => token,
            Err(IncomingPeerError::RegistrationLimit { maximum }) => {
                return Ok(SeedReconcileOutcome::Unavailable(format!(
                    "seed registration limit {maximum} reached"
                )));
            }
            Err(error) => return Err(error.into()),
        };
        self.registrations
            .lock()
            .await
            .insert(resume.torrent_id.clone(), token);
        Ok(SeedReconcileOutcome::Registered)
    }

    pub(crate) async fn unregister(&self, torrent_id: &str) -> Result<bool, IncomingSeedingError> {
        let Some(token) = self.registrations.lock().await.remove(torrent_id) else {
            return Ok(false);
        };
        Ok(self.handle.unregister(token).await?)
    }

    pub(crate) fn stop(&self) {
        self.enabled.store(false, Ordering::Release);
    }
}

fn eligibility_reason(
    resume: &ResumeRecord,
    catalog_eligible: bool,
    root: Option<&StorageRootLocation>,
    active_download: bool,
) -> Option<&'static str> {
    if !catalog_eligible {
        return Some("torrent is archived or being removed");
    }
    if active_download {
        return Some("torrent has active engine work");
    }
    if !resume.desired_running {
        return Some("torrent is paused");
    }
    if resume.state != TorrentState::Complete || resume.storage_state != StorageState::Published {
        return Some("torrent is not durably complete and published");
    }
    if resume.raw_info.is_none() || resume.have.is_none() {
        return Some("torrent lacks verified metadata or have state");
    }
    if !matches!(root, Some(StorageRootLocation::Path(_))) {
        return Some("torrent storage root is not path backed");
    }
    None
}

#[derive(Debug)]
pub(crate) enum IncomingSeedingError {
    Engine(IncomingPeerError),
    InvalidDurableState(String),
}

impl fmt::Display for IncomingSeedingError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Engine(error) => write!(formatter, "{error}"),
            Self::InvalidDurableState(error) => write!(formatter, "{error}"),
        }
    }
}

impl Error for IncomingSeedingError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Engine(error) => Some(error),
            Self::InvalidDurableState(_) => None,
        }
    }
}

impl From<IncomingPeerError> for IncomingSeedingError {
    fn from(error: IncomingPeerError) -> Self {
        Self::Engine(error)
    }
}
