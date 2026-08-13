//! Durable eligibility and exact registration ownership for incoming seeding.

use std::error::Error;
use std::fmt;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

use rstorrent_engine::{
    FastResumeValidation, IncomingPeerError, IncomingPeerHandle, PlatformStorageFailureKind,
    PlatformStorageSpec, PublicationShape, ResumeAdmissionOutcome, ResumeValidationIntent,
    ResumeValidationRejectReason, SeedContent, SeedContentError, SeedRegistration,
    SeedRegistrationToken, SelectiveStorageError, StorageFilePool, TorrentArtifactIdentity,
    TorrentPeerHandle, decide_resume_admission, validate_published_fast_resume_content_with_path,
    validate_published_fast_resume_content_with_platform,
};
use rstorrent_protocol::content::{TorrentContent, TorrentContentProjection};
use rstorrent_protocol::metainfo::{DURABLE_METAINFO_LIMITS, Metainfo, MetainfoError};

use crate::control::{StorageState, TorrentState};
use crate::store::{ResumeRecord, StorageRootLocation};

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum SeedReconcileOutcome {
    Registered {
        validation: FastResumeValidation,
        elapsed_millis: u64,
    },
    AlreadyRegistered,
    Unregistered,
    Ineligible(&'static str),
    NeedsFullCheck {
        reason: ResumeValidationRejectReason,
        validation: FastResumeValidation,
        elapsed_millis: u64,
    },
    AwaitingStorage(String),
    NeedsRepair(String),
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
        let content = match parse_resume_content(resume) {
            Ok(content) => Arc::new(content),
            Err(error) => {
                return Ok(SeedReconcileResult {
                    outcome: SeedReconcileOutcome::Unavailable(error.to_string()),
                    token: None,
                });
            }
        };
        if resume.info_hashes != content.info_hashes() {
            return Ok(SeedReconcileResult {
                outcome: SeedReconcileOutcome::Unavailable(
                    "stored metadata does not match torrent identity".to_owned(),
                ),
                token: None,
            });
        }
        if resume.publication_name.as_deref() != Some(content.name()) {
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
        if have.pieces().len() != content.piece_count() {
            return Ok(SeedReconcileResult {
                outcome: SeedReconcileOutcome::Unavailable(
                    "durable have length does not match verified metadata".to_owned(),
                ),
                token: None,
            });
        }
        let artifact_identity = TorrentArtifactIdentity {
            torrent_id: resume.torrent_id,
            content_fingerprint: have.content_fingerprint(),
        };
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
        storage_file_pool.invalidate_storage(&resume.torrent_id.to_string());
        let root = root.expect("eligible seed has a configured root");
        let validation_started = Instant::now();
        let validation = match root {
            StorageRootLocation::Path(root) => {
                validate_published_fast_resume_content_with_path(
                    root,
                    artifact_identity,
                    content.clone(),
                    have.pieces(),
                    &skipped,
                    storage_file_pool.clone(),
                )
                .await
            }
            StorageRootLocation::PlatformCapability => {
                validate_published_fast_resume_content_with_platform(
                    platform_spec(resume, &content, storage_file_pool),
                    artifact_identity,
                    content.clone(),
                    have.pieces(),
                    &skipped,
                )
                .await
            }
        };
        let validation = match validation {
            Ok(validation) => validation,
            Err(error) => {
                return Ok(SeedReconcileResult {
                    outcome: classify_validation_error(error),
                    token: None,
                });
            }
        };
        let elapsed_millis =
            u64::try_from(validation_started.elapsed().as_millis()).unwrap_or(u64::MAX);
        match decide_resume_admission(ResumeValidationIntent::FastEligible, validation.evidence) {
            ResumeAdmissionOutcome::Accepted => {}
            ResumeAdmissionOutcome::NeedsFullCheck(reason) => {
                return Ok(SeedReconcileResult {
                    outcome: SeedReconcileOutcome::NeedsFullCheck {
                        reason,
                        validation,
                        elapsed_millis,
                    },
                    token: None,
                });
            }
            ResumeAdmissionOutcome::AwaitingStorage => {
                return Ok(SeedReconcileResult {
                    outcome: SeedReconcileOutcome::AwaitingStorage(
                        "published storage is unavailable".to_owned(),
                    ),
                    token: None,
                });
            }
            ResumeAdmissionOutcome::NeedsRepair => {
                return Ok(SeedReconcileResult {
                    outcome: SeedReconcileOutcome::NeedsRepair(
                        "published storage structure needs repair".to_owned(),
                    ),
                    token: None,
                });
            }
        }
        let opened = match root {
            StorageRootLocation::Path(root) => {
                SeedContent::open_published_content_with_pool(
                    root,
                    resume.torrent_id,
                    &content,
                    have.pieces(),
                    &skipped,
                    storage_file_pool.clone(),
                )
                .await
            }
            StorageRootLocation::PlatformCapability => {
                SeedContent::open_published_content_with_platform(
                    &platform_spec(resume, &content, storage_file_pool),
                    &content,
                    have.pieces(),
                    &skipped,
                )
                .await
            }
        };
        let swarm_key = content.swarm_key();
        let seed_content = match opened {
            Ok(content) => content,
            Err(error) => {
                return Ok(SeedReconcileResult {
                    outcome: classify_seed_open_error(error),
                    token: None,
                });
            }
        };
        let registration = match SeedRegistration::new_with_swarm_key(
            raw_info.clone(),
            swarm_key,
            seed_content,
            torrent_peers,
        ) {
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
            outcome: SeedReconcileOutcome::Registered {
                validation,
                elapsed_millis,
            },
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

fn parse_resume_content(resume: &ResumeRecord) -> Result<TorrentContent, MetainfoError> {
    match (resume.info_hashes.v1_hash(), resume.info_hashes.v2_hash()) {
        (Some(_), None) => resume
            .raw_info
            .as_deref()
            .ok_or(MetainfoError::Unsupported("missing durable v1 info"))
            .and_then(|raw_info| {
                Metainfo::from_info_bytes_with_limits(raw_info, DURABLE_METAINFO_LIMITS)
            })
            .map(TorrentContent::from_v1_metainfo),
        (None, Some(_)) => {
            let source = resume
                .metainfo_source
                .as_deref()
                .ok_or(MetainfoError::Unsupported("missing complete v2 source"))?;
            let projection =
                TorrentContentProjection::from_bytes_with_limits(source, DURABLE_METAINFO_LIMITS)?;
            if resume.raw_info.as_deref() != Some(&source[projection.info_span.clone()]) {
                return Err(MetainfoError::Unsupported(
                    "stored v2 info does not match complete source",
                ));
            }
            Ok(projection.content)
        }
        (Some(_), Some(_)) => Err(MetainfoError::Unsupported("hybrid runtime content")),
        (None, None) => Err(MetainfoError::Unsupported("missing torrent identity")),
    }
}

fn platform_spec(
    resume: &ResumeRecord,
    content: &TorrentContent,
    storage_file_pool: &StorageFilePool,
) -> PlatformStorageSpec {
    PlatformStorageSpec {
        pool: storage_file_pool.clone(),
        root_id: resume.storage_root.clone(),
        storage_id: resume.torrent_id.to_string(),
        publication_name: content.name().to_owned(),
        publication_shape: PublicationShape::from_content(content),
        namespace_generation: 1,
        managed: true,
        published: true,
    }
}

fn storage_failure_is_awaiting(kind: PlatformStorageFailureKind) -> bool {
    matches!(
        kind,
        PlatformStorageFailureKind::GrantUnavailable
            | PlatformStorageFailureKind::PermissionDenied
            | PlatformStorageFailureKind::ProviderRefused
            | PlatformStorageFailureKind::NonSeekable
            | PlatformStorageFailureKind::Cancelled
            | PlatformStorageFailureKind::DeadlineExceeded
    )
}

fn classify_validation_error(error: SelectiveStorageError) -> SeedReconcileOutcome {
    let detail = error.to_string();
    if error
        .platform_failure_kind()
        .is_some_and(storage_failure_is_awaiting)
        || matches!(error, SelectiveStorageError::Io { .. })
    {
        SeedReconcileOutcome::AwaitingStorage(detail)
    } else {
        SeedReconcileOutcome::NeedsRepair(detail)
    }
}

fn classify_seed_open_error(error: SeedContentError) -> SeedReconcileOutcome {
    let awaiting = matches!(
        &error,
        SeedContentError::Storage { source, .. }
            if source
                .platform_failure_kind()
                .is_some_and(storage_failure_is_awaiting)
    ) || matches!(error, SeedContentError::Io { .. });
    let detail = error.to_string();
    if awaiting {
        SeedReconcileOutcome::AwaitingStorage(detail)
    } else {
        SeedReconcileOutcome::NeedsRepair(detail)
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
