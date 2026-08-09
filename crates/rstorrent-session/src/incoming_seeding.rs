//! Durable eligibility and exact registration ownership for incoming seeding.

use std::error::Error;
use std::fmt;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use rstorrent_engine::{
    IncomingPeerError, IncomingPeerHandle, PlatformStorageSpec, PublicationShape, SeedContent,
    SeedRegistration, SeedRegistrationToken, StorageFilePool, TorrentPeerHandle,
};
use rstorrent_protocol::metainfo::{DURABLE_METAINFO_LIMITS, Metainfo};

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
pub(crate) struct SeedReconcileResult {
    pub(crate) outcome: SeedReconcileOutcome,
    pub(crate) token: Option<SeedRegistrationToken>,
}

pub(crate) struct SeedReconcileInput<'a> {
    pub(crate) resume: &'a ResumeRecord,
    pub(crate) catalog_eligible: bool,
    pub(crate) root: Option<&'a StorageRootLocation>,
    pub(crate) active_download: bool,
    pub(crate) current: Option<SeedRegistrationToken>,
    pub(crate) torrent_peers: TorrentPeerHandle,
    pub(crate) storage_file_pool: &'a StorageFilePool,
}

#[derive(Clone, Debug)]
pub(crate) struct IncomingSeeding {
    handle: IncomingPeerHandle,
    enabled: Arc<AtomicBool>,
}

impl IncomingSeeding {
    pub(crate) fn new(handle: IncomingPeerHandle) -> Self {
        Self {
            handle,
            enabled: Arc::new(AtomicBool::new(true)),
        }
    }

    pub(crate) async fn reconcile(
        &self,
        input: SeedReconcileInput<'_>,
    ) -> Result<SeedReconcileResult, IncomingSeedingError> {
        let SeedReconcileInput {
            resume,
            catalog_eligible,
            root,
            active_download,
            current,
            torrent_peers,
            storage_file_pool,
        } = input;
        if !self.enabled.load(Ordering::Acquire) {
            return Ok(SeedReconcileResult {
                outcome: SeedReconcileOutcome::Ineligible("incoming peer service is stopping"),
                token: current,
            });
        }
        let ineligible = eligibility_reason(resume, catalog_eligible, root, active_download);
        if let Some(reason) = ineligible {
            let removed = match current {
                Some(token) => self.unregister(token).await?,
                None => false,
            };
            return Ok(SeedReconcileResult {
                outcome: if removed {
                    SeedReconcileOutcome::Unregistered
                } else {
                    SeedReconcileOutcome::Ineligible(reason)
                },
                token: None,
            });
        }
        if current.is_some() {
            return Ok(SeedReconcileResult {
                outcome: SeedReconcileOutcome::AlreadyRegistered,
                token: current,
            });
        }
        let raw_info = resume
            .raw_info
            .as_ref()
            .expect("eligible seed has verified metadata");
        let metainfo =
            match Metainfo::from_info_bytes_with_limits(raw_info, DURABLE_METAINFO_LIMITS) {
                Ok(metainfo) => metainfo,
                Err(error) => {
                    return Ok(SeedReconcileResult {
                        outcome: SeedReconcileOutcome::Unavailable(error.to_string()),
                        token: None,
                    });
                }
            };
        let identity = decode_info_hash(&resume.torrent_id).ok_or_else(|| {
            IncomingSeedingError::InvalidDurableState("invalid torrent identity".to_owned())
        })?;
        if identity != metainfo.info_hash {
            return Ok(SeedReconcileResult {
                outcome: SeedReconcileOutcome::Unavailable(
                    "stored metadata does not match torrent identity".to_owned(),
                ),
                token: None,
            });
        }
        if resume.publication_name.as_deref() != Some(metainfo.name.as_str()) {
            return Ok(SeedReconcileResult {
                outcome: SeedReconcileOutcome::Unavailable(
                    "published name does not match verified metadata".to_owned(),
                ),
                token: None,
            });
        }
        let have = resume
            .have
            .as_ref()
            .expect("eligible seed has durable have state");
        if have.pieces().len() != metainfo.piece_count() {
            return Ok(SeedReconcileResult {
                outcome: SeedReconcileOutcome::Unavailable(
                    "durable have length does not match verified metadata".to_owned(),
                ),
                token: None,
            });
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
        storage_file_pool.invalidate_storage(&resume.torrent_id);
        let opened = match root.expect("eligible seed has a configured root") {
            StorageRootLocation::Path(root) => {
                SeedContent::open_published_with_pool(
                    root,
                    &metainfo,
                    have.pieces(),
                    &skipped,
                    storage_file_pool.clone(),
                    &resume.torrent_id,
                )
                .await
            }
            StorageRootLocation::PlatformCapability => {
                SeedContent::open_published_with_platform(
                    &PlatformStorageSpec {
                        pool: storage_file_pool.clone(),
                        root_id: resume.storage_root.clone(),
                        storage_id: resume.torrent_id.clone(),
                        publication_name: metainfo.name.clone(),
                        publication_shape: PublicationShape::from_metainfo(&metainfo),
                        namespace_generation: 1,
                        managed: true,
                        published: true,
                    },
                    &metainfo,
                    have.pieces(),
                    &skipped,
                )
                .await
            }
        };
        let content = match opened {
            Ok(content) => content,
            Err(error) => {
                return Ok(SeedReconcileResult {
                    outcome: SeedReconcileOutcome::Unavailable(error.to_string()),
                    token: None,
                });
            }
        };
        let registration = match SeedRegistration::new(raw_info.clone(), content, torrent_peers) {
            Ok(registration) => registration,
            Err(error) => {
                return Ok(SeedReconcileResult {
                    outcome: SeedReconcileOutcome::Unavailable(error.to_string()),
                    token: None,
                });
            }
        };
        let token = match self.handle.register(registration).await {
            Ok(token) => token,
            Err(IncomingPeerError::RegistrationLimit { maximum }) => {
                return Ok(SeedReconcileResult {
                    outcome: SeedReconcileOutcome::Unavailable(format!(
                        "seed registration limit {maximum} reached"
                    )),
                    token: None,
                });
            }
            Err(error) => return Err(error.into()),
        };
        Ok(SeedReconcileResult {
            outcome: SeedReconcileOutcome::Registered,
            token: Some(token),
        })
    }

    pub(crate) async fn unregister(
        &self,
        token: SeedRegistrationToken,
    ) -> Result<bool, IncomingSeedingError> {
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
    storage_root_eligibility_reason(root)
}

fn storage_root_eligibility_reason(root: Option<&StorageRootLocation>) -> Option<&'static str> {
    match root {
        Some(StorageRootLocation::Path(_) | StorageRootLocation::PlatformCapability) => None,
        None => Some("torrent storage root is unavailable"),
    }
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

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::storage_root_eligibility_reason;
    use crate::store::StorageRootLocation;

    #[test]
    fn seed_storage_eligibility_accepts_both_configured_backend_kinds() {
        let path = StorageRootLocation::Path(PathBuf::from("payload"));
        let platform = StorageRootLocation::PlatformCapability;

        assert_eq!(storage_root_eligibility_reason(Some(&path)), None);
        assert_eq!(storage_root_eligibility_reason(Some(&platform)), None);
        assert_eq!(
            storage_root_eligibility_reason(None),
            Some("torrent storage root is unavailable")
        );
    }
}
