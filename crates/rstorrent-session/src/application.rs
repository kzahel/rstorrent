use std::collections::BTreeMap;
use std::error::Error;
use std::fmt;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use rstorrent_engine::dht::{DhtConfig, DhtError, DhtService};
use rstorrent_engine::{
    DescriptorFile, DescriptorStorage, DescriptorStoragePlan, DiskCheckpointStage,
    DownloadActivityEvent, DownloadActivitySink, DownloadCheckpointSink, DownloadControl,
    DownloadError, DownloadResourceLimits, NetworkConfig, PreparedFileHash,
    ResumableMagnetDownloadConfig, ResumedStorage, download_magnet_metadata_with_dht,
    plan_descriptor_storage, resume_magnet_to_descriptors_with_control, resume_magnet_with_control,
    selective_part_path, selective_staging_path, verify_prepared_descriptors,
};
use rstorrent_protocol::metainfo::Metainfo;
use tokio::task::JoinHandle;

use crate::control::{
    Command, ErrorCode, RemovalDataPolicy, RemovalState, RequestEnvelope, ResponseEnvelope,
    ResponseOutcome, StorageRootSnapshot, StorageState, TorrentState,
};
use crate::diagnostics::{
    DiagnosticCategory, DiagnosticDraft, DiagnosticField, DiagnosticSeverity, DiagnosticSubject,
    category,
};
use crate::file_views::FileProgressModel;
use crate::have::HaveState;
use crate::store::{
    ConfiguredStorageRoot, PreparedFileRecord, RemovalRecord, ResumeRecord, SessionStore,
    StorageRootLocation, StoreError, StoredStorageRoot,
};
use crate::tracker_views::TrackerViewModel;
use crate::view_sets::{VIEW_SET_REAPER_INTERVAL_MILLIS, ViewSetLeaseReaper};
use crate::views::{
    DurableTorrentViewState, ProgressInputs, SubscriptionError, SubscriptionSpec, TorrentActivity,
    ViewHub, ViewSubscription, ranges_from_pieces,
};
use crate::{
    OpenViewSetRequest, OpenViewSetResponse, UpdateViewSetRequest, ViewSet, ViewSetError,
    ViewSetOwner,
};

#[derive(Clone, Debug)]
pub struct ApplicationConfig {
    pub profile_root: PathBuf,
    pub profile_id: String,
    pub storage_roots: Vec<ConfiguredStorageRoot>,
    pub network: NetworkConfig,
    pub download_resource_limits: DownloadResourceLimits,
    pub dht: DhtConfig,
    pub view_set_lease: Duration,
    pub view_set_reaper_interval: Duration,
    #[doc(hidden)]
    pub storage_write_delay_for_testing: Duration,
    #[doc(hidden)]
    pub storage_write_concurrency_for_testing: usize,
    #[doc(hidden)]
    pub storage_hash_concurrency_for_testing: usize,
    #[doc(hidden)]
    pub checkpoint_sync_delay_for_testing: Duration,
    #[doc(hidden)]
    pub checkpoint_commit_delay_for_testing: Duration,
    #[doc(hidden)]
    pub checkpoint_stage_trace_for_testing: bool,
}

impl ApplicationConfig {
    pub fn new(
        profile_root: PathBuf,
        profile_id: String,
        storage_roots: Vec<ConfiguredStorageRoot>,
        network: NetworkConfig,
    ) -> Self {
        let dht = DhtConfig::for_network(network.policy);
        Self {
            profile_root,
            profile_id,
            storage_roots,
            network,
            download_resource_limits: DownloadResourceLimits::DESKTOP,
            dht,
            view_set_lease: Duration::from_millis(crate::view_sets::VIEW_SET_LEASE_MILLIS),
            view_set_reaper_interval: Duration::from_millis(VIEW_SET_REAPER_INTERVAL_MILLIS),
            storage_write_delay_for_testing: Duration::ZERO,
            storage_write_concurrency_for_testing: 4,
            storage_hash_concurrency_for_testing: 4,
            checkpoint_sync_delay_for_testing: Duration::ZERO,
            checkpoint_commit_delay_for_testing: Duration::ZERO,
            checkpoint_stage_trace_for_testing: false,
        }
    }
}

#[derive(Debug)]
struct ActiveDownload {
    torrent_id: String,
    control: DownloadControl,
    task: JoinHandle<Result<(), String>>,
}

#[derive(Debug)]
enum ApplicationTaskReport {
    Metadata,
    Download,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PlatformRemovalPlan {
    pub operation_id: String,
    pub torrent_id: String,
    pub storage_root: String,
    pub name: String,
}

#[derive(Debug)]
pub struct ApplicationService {
    store: Arc<Mutex<SessionStore>>,
    storage_roots: Arc<BTreeMap<String, StorageRootLocation>>,
    network: NetworkConfig,
    download_resource_limits: DownloadResourceLimits,
    storage_write_delay_for_testing: Duration,
    storage_write_concurrency_for_testing: usize,
    storage_hash_concurrency_for_testing: usize,
    checkpoint_sync_delay_for_testing: Duration,
    checkpoint_commit_delay_for_testing: Duration,
    checkpoint_stage_trace_for_testing: bool,
    active: Option<ActiveDownload>,
    dht: Option<DhtService>,
    views: ViewHub,
    view_set_reaper: Option<ViewSetLeaseReaper>,
}

impl ApplicationService {
    pub async fn open(config: ApplicationConfig) -> Result<Self, ApplicationError> {
        if config.network.peer_connect_timeout.is_zero() {
            return Err(ApplicationError::Configuration(
                "peer connect timeout must be nonzero".to_owned(),
            ));
        }
        if config.network.peer_io_timeout.is_zero() {
            return Err(ApplicationError::Configuration(
                "peer I/O timeout must be nonzero".to_owned(),
            ));
        }
        if config.view_set_lease.is_zero()
            || config.view_set_reaper_interval.is_zero()
            || config.view_set_reaper_interval > config.view_set_lease
        {
            return Err(ApplicationError::Configuration(
                "view-set lease timing must be nonzero and the reaper interval cannot exceed the lease"
                    .to_owned(),
            ));
        }
        if config.storage_write_delay_for_testing > Duration::from_secs(10)
            || config.checkpoint_sync_delay_for_testing > Duration::from_secs(60)
            || config.checkpoint_commit_delay_for_testing > Duration::from_secs(60)
        {
            return Err(ApplicationError::Configuration(
                "test storage delay exceeds its fixed maximum".to_owned(),
            ));
        }
        if !(1..=8).contains(&config.storage_write_concurrency_for_testing)
            || !(1..=8).contains(&config.storage_hash_concurrency_for_testing)
        {
            return Err(ApplicationError::Configuration(
                "test storage concurrency must be between 1 and 8".to_owned(),
            ));
        }
        let mut configured_root_ids = BTreeMap::new();
        for root in &config.storage_roots {
            if configured_root_ids
                .insert(root.id.clone(), root.location.clone())
                .is_some()
            {
                return Err(ApplicationError::Configuration(format!(
                    "storage root {} is duplicated",
                    root.id
                )));
            }
            if let StorageRootLocation::Path(path) = &root.location {
                std::fs::create_dir_all(path).map_err(|source| ApplicationError::Io {
                    operation: "create configured storage root",
                    source,
                })?;
            }
        }
        let store = SessionStore::open(
            &config.profile_root,
            &config.profile_id,
            &config.storage_roots,
        )?;
        let storage_roots = available_storage_roots(store.storage_roots()?);
        let (initial_dht_snapshot, dht_state_warning) = match store.load_dht_snapshot() {
            Ok(snapshot) => (snapshot, None),
            Err(error) => (None, Some(error.to_string())),
        };
        let mut dht_config = config.dht;
        dht_config.network_policy = config.network.policy;
        dht_config.initial_snapshot = initial_dht_snapshot;
        let dht = DhtService::start(dht_config).await?;
        let snapshot = store.snapshot()?;
        let views = ViewHub::new_with_view_set_lease(&snapshot, config.view_set_lease)?;
        let view_set_reaper =
            ViewSetLeaseReaper::start(views.clone(), config.view_set_reaper_interval);
        let mut service = Self {
            store: Arc::new(Mutex::new(store)),
            storage_roots: Arc::new(storage_roots),
            network: config.network,
            download_resource_limits: config.download_resource_limits,
            storage_write_delay_for_testing: config.storage_write_delay_for_testing,
            storage_write_concurrency_for_testing: config.storage_write_concurrency_for_testing,
            storage_hash_concurrency_for_testing: config.storage_hash_concurrency_for_testing,
            checkpoint_sync_delay_for_testing: config.checkpoint_sync_delay_for_testing,
            checkpoint_commit_delay_for_testing: config.checkpoint_commit_delay_for_testing,
            checkpoint_stage_trace_for_testing: config.checkpoint_stage_trace_for_testing,
            active: None,
            dht: Some(dht),
            views,
            view_set_reaper: Some(view_set_reaper),
        };
        service.views.record_diagnostic(
            DiagnosticSeverity::Info,
            category::LIFECYCLE_SESSION,
            "application_opened",
            None,
            "Application profile opened",
            &[
                ("profile", &config.profile_id),
                ("network_policy", config.network.policy.as_str()),
            ],
        )?;
        if let Some(detail) = dht_state_warning {
            service.views.record_diagnostic(
                DiagnosticSeverity::Warning,
                category::DISCOVERY_DHT,
                "dht_state_rejected",
                None,
                "Saved DHT state was rejected; using cold bootstrap",
                &[("detail", &detail)],
            )?;
        }
        service.refresh_views()?;
        service.restore_removals().await?;
        service.restore_running().await?;
        service.refresh_views()?;
        Ok(service)
    }

    pub async fn dispatch(
        &mut self,
        request: RequestEnvelope,
    ) -> Result<ResponseEnvelope, ApplicationError> {
        self.reap_finished().await?;
        let command = request.command.clone();
        let selected_root = match &command {
            Command::AddMagnet { storage_root, .. }
            | Command::SetDefaultStorageRoot { storage_root } => Some(storage_root.as_str()),
            _ => None,
        };
        if let Some(storage_root) = selected_root
            && !self.storage_roots.contains_key(storage_root)
        {
            let snapshot = self.store_mut()?.snapshot()?;
            let known = snapshot
                .storage
                .roots
                .iter()
                .any(|root| root.root_id == storage_root);
            return Ok(ResponseEnvelope::error(
                request.request_id,
                self.store_mut()?.revision()?,
                if known {
                    ErrorCode::StorageNeedsRepair
                } else {
                    ErrorCode::UnknownStorageRoot
                },
                if known {
                    format!("storage root {storage_root} is unavailable and needs repair")
                } else {
                    format!("storage root {storage_root} is not configured")
                },
            ));
        }
        if let Some(active) = &self.active {
            let target = match &command {
                Command::AddMagnet { magnet, .. } => {
                    rstorrent_protocol::magnet::Magnet::parse(magnet)
                        .ok()
                        .map(|magnet| encode_info_hash(magnet.info_hash))
                }
                Command::Resume { torrent_id } => Some(torrent_id.to_ascii_lowercase()),
                _ => None,
            };
            if target.is_some_and(|target| target != active.torrent_id) {
                return Ok(ResponseEnvelope::error(
                    request.request_id,
                    self.store_mut()?.revision()?,
                    ErrorCode::Busy,
                    format!(
                        "torrent {} already owns the download slot",
                        active.torrent_id
                    ),
                ));
            }
        }
        let response = self.store_mut()?.handle_durable(&request)?;
        if !matches!(response.outcome, ResponseOutcome::Success { .. }) {
            return Ok(response);
        }
        self.refresh_views()?;

        match command {
            Command::AddMagnet { magnet, .. } => {
                let torrent_id = rstorrent_protocol::magnet::Magnet::parse(&magnet)
                    .map(|magnet| encode_info_hash(magnet.info_hash))
                    .map_err(|error| ApplicationError::Configuration(error.to_string()))?;
                self.views.record_diagnostic(
                    DiagnosticSeverity::Info,
                    category::LIFECYCLE_TORRENT,
                    "torrent_added",
                    Some(&torrent_id),
                    "Torrent added to the session",
                    &[],
                )?;
                self.start_if_possible(&torrent_id).await?;
            }
            Command::Resume { torrent_id } => {
                let torrent_id = torrent_id.to_ascii_lowercase();
                self.views.record_diagnostic(
                    DiagnosticSeverity::Info,
                    category::LIFECYCLE_TORRENT,
                    "torrent_resumed",
                    Some(&torrent_id),
                    "Torrent resume requested",
                    &[],
                )?;
                self.start_if_possible(&torrent_id).await?;
            }
            Command::Pause { torrent_id } => {
                let torrent_id = torrent_id.to_ascii_lowercase();
                self.views.record_diagnostic(
                    DiagnosticSeverity::Info,
                    category::LIFECYCLE_TORRENT,
                    "torrent_paused",
                    Some(&torrent_id),
                    "Torrent pause requested",
                    &[],
                )?;
                self.pause(&torrent_id).await?;
            }
            Command::Archive { torrent_id } => {
                self.views.record_diagnostic(
                    DiagnosticSeverity::Info,
                    category::LIFECYCLE_TORRENT,
                    "torrent_archived",
                    Some(&torrent_id),
                    "Torrent archived",
                    &[],
                )?;
            }
            Command::RestoreArchive { torrent_id } => {
                self.views.record_diagnostic(
                    DiagnosticSeverity::Info,
                    category::LIFECYCLE_TORRENT,
                    "torrent_archive_restored",
                    Some(&torrent_id),
                    "Torrent restored from archive",
                    &[],
                )?;
            }
            Command::RemoveTorrent { torrent_id, .. } => {
                let torrent_id = torrent_id.to_ascii_lowercase();
                self.views.record_diagnostic(
                    DiagnosticSeverity::Info,
                    category::LIFECYCLE_TORRENT,
                    "torrent_removal_started",
                    Some(&torrent_id),
                    "Torrent removal started",
                    &[],
                )?;
                self.drive_removal(&torrent_id).await?;
            }
            Command::RemoveStorageRoot { .. } => {
                self.reload_storage_roots()?;
            }
            Command::SetDefaultStorageRoot { .. } | Command::SetShowAddOptions { .. } => {}
            Command::Shutdown => {
                self.shutdown().await?;
            }
            Command::Snapshot => {}
        }
        Ok(response)
    }

    pub fn revision(&self) -> Result<u64, ApplicationError> {
        Ok(self.store_mut()?.revision()?)
    }

    pub fn storage_snapshot(&self) -> Result<crate::StorageSettingsSnapshot, ApplicationError> {
        Ok(self.store_mut()?.snapshot()?.storage)
    }

    pub fn suggested_storage_root_path(
        &self,
        repair_root: Option<&str>,
    ) -> Result<Option<PathBuf>, ApplicationError> {
        let store = self.store_mut()?;
        let snapshot = store.snapshot()?;
        let roots = store.storage_roots()?;
        let preferred = repair_root
            .map(str::to_owned)
            .or(snapshot.storage.default_root);
        let candidate = preferred
            .as_deref()
            .and_then(|root_id| roots.iter().find(|root| root.id == root_id))
            .or_else(|| {
                roots.iter().find(|root| {
                    matches!(&root.location, StorageRootLocation::Path(path) if path.is_dir())
                })
            });
        Ok(candidate.and_then(|root| match &root.location {
            StorageRootLocation::Path(path) if path.is_dir() => Some(path.clone()),
            StorageRootLocation::Path(path) => path.parent().map(Path::to_path_buf),
            StorageRootLocation::PlatformCapability => None,
        }))
    }

    pub fn install_path_storage_root(
        &mut self,
        selected_path: &Path,
    ) -> Result<StorageRootSnapshot, ApplicationError> {
        let path = validate_selected_directory(selected_path)?;
        let label = storage_root_label(&path);
        let root_id = self.allocate_storage_root_id()?;
        let (_, installed_id) = self
            .store_mut()?
            .install_path_storage_root(&root_id, &label, &path)?;
        self.reload_storage_roots()?;
        self.refresh_views()?;
        self.storage_snapshot()?
            .roots
            .into_iter()
            .find(|root| root.root_id == installed_id)
            .ok_or_else(|| {
                ApplicationError::Configuration(
                    "installed storage root is missing from the durable snapshot".to_owned(),
                )
            })
    }

    pub fn repair_path_storage_root(
        &mut self,
        root_id: &str,
        selected_path: &Path,
    ) -> Result<StorageRootSnapshot, ApplicationError> {
        let snapshot = self.storage_snapshot()?;
        let root = snapshot
            .roots
            .iter()
            .find(|root| root.root_id == root_id)
            .ok_or_else(|| {
                ApplicationError::Configuration(format!("storage root {root_id} is not configured"))
            })?;
        if root.availability == crate::StorageRootAvailability::Available {
            return Err(ApplicationError::Configuration(
                "an available root cannot be re-selected; torrent relocation is not implemented"
                    .to_owned(),
            ));
        }
        if let Some(active) = &self.active {
            let resume = self.store_mut()?.load_resume(&active.torrent_id)?;
            if resume.storage_root == root_id {
                return Err(ApplicationError::Busy(active.torrent_id.clone()));
            }
        }
        let path = validate_selected_directory(selected_path)?;
        let label = storage_root_label(&path);
        self.store_mut()?
            .repair_path_storage_root(root_id, &label, &path)?;
        self.reload_storage_roots()?;
        self.refresh_views()?;
        self.storage_snapshot()?
            .roots
            .into_iter()
            .find(|root| root.root_id == root_id)
            .ok_or_else(|| {
                ApplicationError::Configuration(
                    "repaired storage root is missing from the durable snapshot".to_owned(),
                )
            })
    }

    fn allocate_storage_root_id(&self) -> Result<String, ApplicationError> {
        let existing = self
            .store_mut()?
            .storage_roots()?
            .into_iter()
            .map(|root| root.id)
            .collect::<std::collections::BTreeSet<_>>();
        for _ in 0..4 {
            let mut bytes = [0_u8; 16];
            getrandom::fill(&mut bytes)
                .map_err(|error| ApplicationError::Configuration(error.to_string()))?;
            let mut id = String::with_capacity(37);
            id.push_str("root_");
            for byte in bytes {
                use std::fmt::Write as _;
                write!(&mut id, "{byte:02x}").expect("writing to String cannot fail");
            }
            if !existing.contains(&id) {
                return Ok(id);
            }
        }
        Err(ApplicationError::Configuration(
            "could not allocate a unique storage root ID".to_owned(),
        ))
    }

    fn reload_storage_roots(&mut self) -> Result<(), ApplicationError> {
        let roots = self.store_mut()?.storage_roots()?;
        self.storage_roots = Arc::new(available_storage_roots(roots));
        Ok(())
    }

    pub fn api_hello(&self) -> crate::ApiHello {
        let mut hello = crate::ApiHello::default();
        hello.limits.lease_millis = self.views.view_set_lease().as_millis().to_string();
        hello
    }

    pub fn subscribe(&self, spec: SubscriptionSpec) -> Result<ViewSubscription, ApplicationError> {
        Ok(self.views.subscribe(spec)?)
    }

    pub fn open_view_set(
        &self,
        owner: ViewSetOwner,
        request: OpenViewSetRequest,
    ) -> Result<OpenViewSetResponse, ViewSetError> {
        self.views.open_view_set(owner, request)
    }

    pub fn update_view_set(
        &self,
        owner: &ViewSetOwner,
        view_set_id: &str,
        request: UpdateViewSetRequest,
    ) -> Result<(), ViewSetError> {
        self.views.update_view_set(owner, view_set_id, request)
    }

    pub fn view_set(
        &self,
        owner: &ViewSetOwner,
        view_set_id: &str,
    ) -> Result<ViewSet, ViewSetError> {
        self.views.view_set(owner, view_set_id)
    }

    pub fn close_view_set(
        &self,
        owner: &ViewSetOwner,
        view_set_id: &str,
    ) -> Result<(), ViewSetError> {
        self.views.close_view_set(owner, view_set_id)
    }

    pub fn close_view_sets(&self) {
        self.views.close_all_view_sets();
    }

    pub async fn descriptor_storage_plan(
        &mut self,
        torrent_id: &str,
    ) -> Result<DescriptorStoragePlan, ApplicationError> {
        self.reap_finished().await?;
        let resume = self.load_resume_conservative(&torrent_id.to_ascii_lowercase())?;
        if !matches!(
            self.storage_roots.get(&resume.storage_root),
            Some(StorageRootLocation::PlatformCapability)
        ) {
            return Err(ApplicationError::Configuration(
                "torrent does not use a platform storage root".to_owned(),
            ));
        }
        let raw_info = resume.raw_info.ok_or_else(|| {
            ApplicationError::Configuration("torrent metadata is not available".to_owned())
        })?;
        let metainfo = Metainfo::from_info_bytes(&raw_info)
            .map_err(|error| ApplicationError::Configuration(error.to_string()))?;
        let skip_files = resume
            .skip_files
            .into_iter()
            .map(|index| index as usize)
            .collect::<Vec<_>>();
        plan_descriptor_storage(&metainfo, &skip_files, &[])
            .map_err(|error| ApplicationError::Configuration(error.to_string()))
    }

    pub async fn start_with_descriptors(
        &mut self,
        torrent_id: &str,
        descriptors: DescriptorStorage,
    ) -> Result<(), ApplicationError> {
        self.reap_finished().await?;
        let torrent_id = torrent_id.to_ascii_lowercase();
        if let Some(active) = &self.active {
            return Err(ApplicationError::Busy(active.torrent_id.clone()));
        }
        let resume = self.load_resume_conservative(&torrent_id)?;
        if !resume.desired_running
            || matches!(
                resume.state,
                TorrentState::Paused | TorrentState::Complete | TorrentState::AwaitingPublication
            )
        {
            return Ok(());
        }
        if resume.state == TorrentState::NeedsRepair {
            return Err(ApplicationError::Configuration(format!(
                "torrent cannot accept storage in state {}",
                resume.state.as_str()
            )));
        }
        if !matches!(
            self.storage_roots.get(&resume.storage_root),
            Some(StorageRootLocation::PlatformCapability)
        ) {
            return Err(ApplicationError::Configuration(
                "torrent does not use a platform storage root".to_owned(),
            ));
        }
        let raw_info = resume.raw_info.ok_or_else(|| {
            ApplicationError::Configuration("torrent metadata is not available".to_owned())
        })?;
        let initialize_storage = resume.storage_state == StorageState::None;
        let skip_files = resume
            .skip_files
            .into_iter()
            .map(|index| index as usize)
            .collect::<Vec<_>>();
        let verified_pieces = resume
            .have
            .as_ref()
            .map_or_else(Vec::new, |have| have.pieces().to_vec());
        let config = ResumableMagnetDownloadConfig {
            magnet: resume.magnet,
            output_path: PathBuf::new(),
            network: self.network,
            resource_limits: self.download_resource_limits,
            skip_files,
            verified_info: Some(raw_info),
            verified_pieces,
            dht: self.dht.as_ref().map(DhtService::handle),
        };
        let checkpoints: Arc<dyn DownloadCheckpointSink> = Arc::new(StoreCheckpointSink {
            store: self.store.clone(),
            storage_roots: self.storage_roots.clone(),
            torrent_id: torrent_id.clone(),
            views: self.views.clone(),
        });
        let control = self.download_control(&torrent_id);
        let task_control = control.clone();
        let operation = async move {
            resume_magnet_to_descriptors_with_control(
                config,
                descriptors,
                initialize_storage,
                checkpoints,
                task_control,
            )
            .await
            .map(|_| ApplicationTaskReport::Download)
        };
        let task = self.spawn_supervised_task(&torrent_id, operation)?;
        self.active = Some(ActiveDownload {
            torrent_id,
            control,
            task,
        });
        Ok(())
    }

    pub async fn prepared_files(
        &mut self,
        torrent_id: &str,
    ) -> Result<Vec<PreparedFileRecord>, ApplicationError> {
        self.reap_finished().await?;
        Ok(self
            .store_mut()?
            .load_prepared_files(&torrent_id.to_ascii_lowercase())?)
    }

    pub async fn confirm_descriptor_publication(
        &mut self,
        torrent_id: &str,
        descriptors: Vec<DescriptorFile>,
    ) -> Result<(), ApplicationError> {
        self.reap_finished().await?;
        let torrent_id = torrent_id.to_ascii_lowercase();
        let resume = self.load_resume_conservative(&torrent_id)?;
        if resume.state == TorrentState::Complete && resume.storage_state == StorageState::Published
        {
            return Ok(());
        }
        let prepared = self.store_mut()?.load_prepared_files(&torrent_id)?;
        let expected = prepared
            .into_iter()
            .map(|file| PreparedFileHash {
                file_index: file.file_index,
                length: file.length,
                sha1: file.sha1,
            })
            .collect::<Vec<_>>();
        if expected.is_empty() {
            return Err(ApplicationError::Configuration(
                "torrent has no prepared publication manifest".to_owned(),
            ));
        }
        verify_prepared_descriptors(descriptors, &expected)
            .await
            .map_err(|error| ApplicationError::Configuration(error.to_string()))?;
        self.store_mut()?
            .confirm_prepared_publication(&torrent_id)?;
        self.refresh_views()?;
        Ok(())
    }

    pub async fn mark_storage_unavailable(
        &mut self,
        torrent_id: &str,
        message: &str,
    ) -> Result<(), ApplicationError> {
        self.reap_finished().await?;
        let torrent_id = torrent_id.to_ascii_lowercase();
        if self
            .active
            .as_ref()
            .is_some_and(|active| active.torrent_id == torrent_id)
        {
            return Err(ApplicationError::Busy(torrent_id));
        }
        self.store_mut()?
            .mark_awaiting_storage(&torrent_id, Some(message))?;
        self.refresh_views()?;
        Ok(())
    }

    pub async fn platform_removal_plan(
        &mut self,
        torrent_id: &str,
    ) -> Result<PlatformRemovalPlan, ApplicationError> {
        self.reap_finished().await?;
        let torrent_id = torrent_id.to_ascii_lowercase();
        let removal = self.store_mut()?.load_removal(&torrent_id)?;
        if removal.policy != RemovalDataPolicy::DeleteManaged
            || removal.state != RemovalState::AwaitingPlatform
            || !matches!(
                self.storage_roots.get(&removal.storage_root),
                Some(StorageRootLocation::PlatformCapability)
            )
        {
            return Err(ApplicationError::Configuration(
                "torrent is not awaiting platform data removal".to_owned(),
            ));
        }
        let raw_info = removal.raw_info.ok_or_else(|| {
            ApplicationError::Configuration("torrent metadata is not available".to_owned())
        })?;
        let metainfo = Metainfo::from_info_bytes(&raw_info)
            .map_err(|error| ApplicationError::Configuration(error.to_string()))?;
        let plan = plan_descriptor_storage(&metainfo, &[], &[])
            .map_err(|error| ApplicationError::Configuration(error.to_string()))?;
        Ok(PlatformRemovalPlan {
            operation_id: removal.operation_id,
            torrent_id,
            storage_root: removal.storage_root,
            name: plan.name,
        })
    }

    pub async fn confirm_platform_removal(
        &mut self,
        torrent_id: &str,
        operation_id: &str,
    ) -> Result<(), ApplicationError> {
        self.reap_finished().await?;
        let torrent_id = torrent_id.to_ascii_lowercase();
        let removal = self.store_mut()?.load_removal(&torrent_id)?;
        if removal.state != RemovalState::AwaitingPlatform || removal.operation_id != operation_id {
            return Err(ApplicationError::Configuration(
                "platform removal confirmation is stale".to_owned(),
            ));
        }
        self.store_mut()?
            .finalize_removal(&torrent_id, operation_id)?;
        self.refresh_views()?;
        self.views.record_diagnostic(
            DiagnosticSeverity::Info,
            category::PLATFORM_ADAPTER,
            "torrent_removal_completed",
            Some(&torrent_id),
            "Platform-managed torrent data and catalog entry removed",
            &[],
        )?;
        Ok(())
    }

    pub async fn fail_platform_removal(
        &mut self,
        torrent_id: &str,
        operation_id: &str,
        message: &str,
    ) -> Result<(), ApplicationError> {
        self.reap_finished().await?;
        let torrent_id = torrent_id.to_ascii_lowercase();
        let removal = self.store_mut()?.load_removal(&torrent_id)?;
        if removal.state != RemovalState::AwaitingPlatform || removal.operation_id != operation_id {
            return Err(ApplicationError::Configuration(
                "platform removal failure is stale".to_owned(),
            ));
        }
        self.store_mut()?.set_removal_state(
            &torrent_id,
            operation_id,
            RemovalState::Failed,
            Some(message),
        )?;
        self.refresh_views()?;
        self.views.record_diagnostic(
            DiagnosticSeverity::Error,
            category::PLATFORM_ADAPTER,
            "torrent_removal_failed",
            Some(&torrent_id),
            "Platform-managed torrent data could not be removed",
            &[("detail", message)],
        )?;
        Ok(())
    }

    pub async fn shutdown(&mut self) -> Result<(), ApplicationError> {
        let mut active_join_error = None;
        if let Some(mut reaper) = self.view_set_reaper.take()
            && let Err(error) = reaper.shutdown().await
        {
            active_join_error = Some(format!("view-set lease reaper: {error}"));
        }
        self.close_view_sets();
        if let Some(active) = self.active.take() {
            active.control.cancel();
            match active.task.await {
                Ok(Ok(())) => {}
                Ok(Err(error)) if active_join_error.is_none() => active_join_error = Some(error),
                Err(error) if error.is_cancelled() => {}
                Err(error) if active_join_error.is_none() => {
                    active_join_error = Some(error.to_string());
                }
                Ok(Err(_)) | Err(_) => {}
            }
        }
        if let Some(dht) = self.dht.take() {
            let snapshot = dht.shutdown().await?;
            self.store_mut()?.save_dht_snapshot(snapshot)?;
        }
        if let Some(error) = active_join_error {
            return Err(ApplicationError::Join(error));
        }
        self.views.record_diagnostic(
            DiagnosticSeverity::Info,
            category::LIFECYCLE_TORRENT,
            "application_shutdown",
            None,
            "Application shutdown completed",
            &[],
        )?;
        Ok(())
    }

    fn store_mut(&self) -> Result<MutexGuard<'_, SessionStore>, ApplicationError> {
        self.store
            .lock()
            .map_err(|_| ApplicationError::StorePoisoned)
    }

    async fn restore_running(&mut self) -> Result<(), ApplicationError> {
        let torrent_ids = self
            .store_mut()?
            .snapshot()?
            .torrents
            .into_iter()
            .filter(|torrent| !matches!(torrent.state, TorrentState::Paused))
            .map(|torrent| torrent.torrent_id)
            .collect::<Vec<_>>();
        for torrent_id in torrent_ids {
            let desired_running = match self.load_resume_conservative(&torrent_id) {
                Ok(resume) => resume.desired_running,
                Err(error) => {
                    self.store_mut()?
                        .mark_needs_repair(&torrent_id, &error.to_string())?;
                    continue;
                }
            };
            if desired_running {
                self.start_if_possible(&torrent_id).await?;
                break;
            }
        }
        Ok(())
    }

    async fn restore_removals(&mut self) -> Result<(), ApplicationError> {
        let removals = self.store_mut()?.load_removals()?;
        for removal in removals {
            if removal.state == RemovalState::Pending {
                self.drive_removal(&removal.torrent_id).await?;
            }
        }
        Ok(())
    }

    async fn drive_removal(&mut self, torrent_id: &str) -> Result<(), ApplicationError> {
        let removal = match self.store_mut()?.load_removal(torrent_id) {
            Ok(removal) => removal,
            Err(StoreError::UnknownTorrent(_)) => return Ok(()),
            Err(error) => return Err(error.into()),
        };
        if removal.state != RemovalState::Pending {
            return Ok(());
        }
        if let Err(error) = self.pause(torrent_id).await {
            return self.fail_removal(&removal, &error.to_string());
        }
        match removal.policy {
            RemovalDataPolicy::Keep => self.complete_removal(&removal),
            RemovalDataPolicy::DeleteManaged if removal.raw_info.is_none() => {
                self.complete_removal(&removal)
            }
            RemovalDataPolicy::DeleteManaged => {
                match self.storage_roots.get(&removal.storage_root).cloned() {
                    Some(StorageRootLocation::Path(root)) => {
                        let owned_torrent_id = torrent_id.to_owned();
                        match tokio::task::spawn_blocking(move || {
                            delete_path_artifacts(&root, &owned_torrent_id)
                        })
                        .await
                        {
                            Ok(Ok(())) => self.complete_removal(&removal),
                            Ok(Err(error)) => self.fail_removal(&removal, &error.to_string()),
                            Err(error) => self.fail_removal(
                                &removal,
                                &format!("managed-data cleanup task failed: {error}"),
                            ),
                        }
                    }
                    Some(StorageRootLocation::PlatformCapability) => {
                        self.store_mut()?.set_removal_state(
                            torrent_id,
                            &removal.operation_id,
                            RemovalState::AwaitingPlatform,
                            None,
                        )?;
                        self.refresh_views()
                    }
                    None => self.fail_removal(
                        &removal,
                        "configured storage root is unavailable for removal",
                    ),
                }
            }
        }
    }

    fn complete_removal(&self, removal: &RemovalRecord) -> Result<(), ApplicationError> {
        self.store_mut()?
            .finalize_removal(&removal.torrent_id, &removal.operation_id)?;
        self.refresh_views()?;
        self.views.record_diagnostic(
            DiagnosticSeverity::Info,
            category::LIFECYCLE_TORRENT,
            "torrent_removal_completed",
            Some(&removal.torrent_id),
            "Torrent removed from the session",
            &[],
        )?;
        Ok(())
    }

    fn fail_removal(&self, removal: &RemovalRecord, message: &str) -> Result<(), ApplicationError> {
        self.store_mut()?.set_removal_state(
            &removal.torrent_id,
            &removal.operation_id,
            RemovalState::Failed,
            Some(message),
        )?;
        self.refresh_views()?;
        self.views.record_diagnostic(
            DiagnosticSeverity::Error,
            category::STORAGE_IO,
            "torrent_removal_failed",
            Some(&removal.torrent_id),
            "Torrent data could not be removed",
            &[("detail", message)],
        )?;
        Ok(())
    }

    async fn start_if_possible(&mut self, torrent_id: &str) -> Result<(), ApplicationError> {
        self.reap_finished().await?;
        if let Some(active) = &self.active {
            if active.torrent_id == torrent_id {
                return Ok(());
            }
            return Err(ApplicationError::Busy(active.torrent_id.clone()));
        }
        let resume = match self.load_resume_conservative(torrent_id) {
            Ok(resume) => resume,
            Err(error) => {
                self.store_mut()?
                    .mark_needs_repair(torrent_id, &error.to_string())?;
                return Ok(());
            }
        };
        if !resume.desired_running
            || matches!(
                resume.state,
                TorrentState::Paused | TorrentState::NeedsRepair
            )
        {
            return Ok(());
        }
        if let Some(raw_info) = &resume.raw_info {
            let metainfo = match Metainfo::from_info_bytes(raw_info) {
                Ok(metainfo) => metainfo,
                Err(error) => {
                    self.store_mut()?
                        .mark_needs_repair(torrent_id, &error.to_string())?;
                    return Ok(());
                }
            };
            if encode_info_hash(metainfo.info_hash) != torrent_id {
                self.store_mut()?.mark_needs_repair(
                    torrent_id,
                    "stored metadata does not match torrent identity",
                )?;
                return Ok(());
            }
        }
        let root = match self.storage_roots.get(&resume.storage_root).cloned() {
            Some(root) => root,
            None => {
                self.store_mut()?
                    .mark_needs_repair(torrent_id, "configured storage root is unavailable")?;
                return Ok(());
            }
        };
        if matches!(root, StorageRootLocation::PlatformCapability) {
            if resume.state == TorrentState::AwaitingPublication {
                return Ok(());
            }
            if resume.raw_info.is_some() {
                if resume.state != TorrentState::AwaitingStorage {
                    self.store_mut()?.mark_awaiting_storage(torrent_id, None)?;
                    self.refresh_views()?;
                }
                return Ok(());
            }
            let checkpoints = Arc::new(StoreCheckpointSink {
                store: self.store.clone(),
                storage_roots: self.storage_roots.clone(),
                torrent_id: torrent_id.to_owned(),
                views: self.views.clone(),
            });
            let control = self.download_control(torrent_id);
            let task_control = control.clone();
            let magnet = resume.magnet;
            let network = self.network;
            let dht = self.dht.as_ref().map(DhtService::handle);
            let operation = async move {
                let raw_info =
                    download_magnet_metadata_with_dht(magnet, network, task_control, dht).await?;
                checkpoints
                    .metadata_verified(&raw_info)
                    .map_err(DownloadError::Checkpoint)?;
                checkpoints
                    .waiting_for_storage()
                    .map_err(DownloadError::Checkpoint)?;
                Ok(ApplicationTaskReport::Metadata)
            };
            let task = self.spawn_supervised_task(torrent_id, operation)?;
            self.active = Some(ActiveDownload {
                torrent_id: torrent_id.to_owned(),
                control,
                task,
            });
            return Ok(());
        }
        let StorageRootLocation::Path(root) = root else {
            unreachable!("platform root returned above")
        };
        let skip_files = resume
            .skip_files
            .iter()
            .map(|index| {
                usize::try_from(*index).map_err(|_| {
                    ApplicationError::Configuration("file selection index overflow".to_owned())
                })
            })
            .collect::<Result<Vec<_>, _>>()?;
        let verified_pieces = resume
            .have
            .as_ref()
            .map_or_else(Vec::new, |have| have.pieces().to_vec());
        let config = ResumableMagnetDownloadConfig {
            magnet: resume.magnet,
            output_path: root.join(torrent_id),
            network: self.network,
            resource_limits: self.download_resource_limits,
            skip_files,
            verified_info: resume.raw_info,
            verified_pieces,
            dht: self.dht.as_ref().map(DhtService::handle),
        };
        let checkpoints: Arc<dyn DownloadCheckpointSink> = Arc::new(StoreCheckpointSink {
            store: self.store.clone(),
            storage_roots: self.storage_roots.clone(),
            torrent_id: torrent_id.to_owned(),
            views: self.views.clone(),
        });
        let control = self.download_control(torrent_id);
        let task_control = control.clone();
        let operation = async move {
            resume_magnet_with_control(config, checkpoints, task_control)
                .await
                .map(|_| ApplicationTaskReport::Download)
        };
        let task = self.spawn_supervised_task(torrent_id, operation)?;
        self.active = Some(ActiveDownload {
            torrent_id: torrent_id.to_owned(),
            control,
            task,
        });
        Ok(())
    }

    fn download_control(&self, torrent_id: &str) -> DownloadControl {
        let control = DownloadControl::new();
        control.set_storage_write_delay(self.storage_write_delay_for_testing);
        control
            .set_storage_execution_limits_for_testing(
                self.storage_write_concurrency_for_testing,
                self.storage_hash_concurrency_for_testing,
            )
            .expect("application configuration validated diagnostic storage limits");
        control.set_checkpoint_sync_delay_for_testing(self.checkpoint_sync_delay_for_testing);
        control.set_checkpoint_commit_delay_for_testing(self.checkpoint_commit_delay_for_testing);
        control.set_activity_sink(Arc::new(ViewActivitySink {
            torrent_id: torrent_id.to_owned(),
            views: self.views.clone(),
            trace_checkpoint_stages: self.checkpoint_stage_trace_for_testing,
            last_checkpoint_stage: Mutex::new(None),
        }));
        control
    }

    fn spawn_supervised_task<F>(
        &self,
        torrent_id: &str,
        operation: F,
    ) -> Result<JoinHandle<Result<(), String>>, ApplicationError>
    where
        F: Future<Output = Result<ApplicationTaskReport, DownloadError>> + Send + 'static,
    {
        self.views.set_progress_inputs(
            torrent_id,
            ProgressInputs {
                task_active: true,
                ..ProgressInputs::default()
            },
        )?;
        self.views.record_diagnostic(
            DiagnosticSeverity::Info,
            category::LIFECYCLE_TORRENT,
            "engine_task_started",
            Some(torrent_id),
            "Engine task started",
            &[],
        )?;
        let store = self.store.clone();
        let storage_roots = self.storage_roots.clone();
        let views = self.views.clone();
        let torrent_id = torrent_id.to_owned();
        Ok(tokio::spawn(async move {
            let outcome = operation.await;
            handle_task_outcome(&store, &storage_roots, &views, &torrent_id, outcome)
        }))
    }

    fn load_resume_conservative(&self, torrent_id: &str) -> Result<ResumeRecord, ApplicationError> {
        let mut store = self.store_mut()?;
        match store.load_resume(torrent_id) {
            Ok(resume) => Ok(resume),
            Err(StoreError::Have(_)) => {
                store.reset_have_from_metadata(torrent_id)?;
                Ok(store.load_resume(torrent_id)?)
            }
            Err(error) => Err(error.into()),
        }
    }

    async fn pause(&mut self, torrent_id: &str) -> Result<(), ApplicationError> {
        if self
            .active
            .as_ref()
            .is_none_or(|active| active.torrent_id != torrent_id)
        {
            return Ok(());
        }
        let active = self.active.take().expect("matching active task exists");
        active.control.cancel_when_safe();
        match active.task.await {
            Ok(Ok(())) => Ok(()),
            Ok(Err(error)) => Err(ApplicationError::Join(error)),
            Err(error) if error.is_cancelled() => Ok(()),
            Err(error) => Err(ApplicationError::Join(error.to_string())),
        }
    }

    async fn reap_finished(&mut self) -> Result<(), ApplicationError> {
        if self
            .active
            .as_ref()
            .is_none_or(|active| !active.task.is_finished())
        {
            return Ok(());
        }
        let active = self.active.take().expect("finished active task exists");
        match active.task.await {
            Ok(Ok(())) => Ok(()),
            Ok(Err(error)) => Err(ApplicationError::Join(error)),
            Err(error) if error.is_cancelled() => Ok(()),
            Err(error) => Err(ApplicationError::Join(error.to_string())),
        }
    }

    fn refresh_views(&self) -> Result<(), ApplicationError> {
        let (snapshot, durable) = {
            let store = self.store_mut()?;
            durable_view_state(&store, &self.storage_roots)?
        };
        self.views.replace_durable(&snapshot, &durable)?;
        Ok(())
    }
}

impl Drop for ApplicationService {
    fn drop(&mut self) {
        if let Some(active) = &self.active {
            active.control.cancel();
        }
    }
}

fn delete_path_artifacts(root: &Path, torrent_id: &str) -> Result<(), ApplicationError> {
    let output = root.join(torrent_id);
    let staging = selective_staging_path(&output)
        .map_err(|error| ApplicationError::Configuration(error.to_string()))?;
    let part = selective_part_path(&output)
        .map_err(|error| ApplicationError::Configuration(error.to_string()))?;
    remove_managed_directory(&output)?;
    remove_managed_directory(&staging)?;
    remove_managed_file(&part)?;
    Ok(())
}

fn remove_managed_directory(path: &Path) -> Result<(), ApplicationError> {
    let metadata = match std::fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(source) => {
            return Err(ApplicationError::Io {
                operation: "inspect managed torrent directory",
                source,
            });
        }
    };
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        std::fs::remove_file(path).map_err(|source| ApplicationError::Io {
            operation: "remove managed torrent artifact",
            source,
        })
    } else {
        std::fs::remove_dir_all(path).map_err(|source| ApplicationError::Io {
            operation: "remove managed torrent directory",
            source,
        })
    }
}

fn remove_managed_file(path: &Path) -> Result<(), ApplicationError> {
    let metadata = match std::fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(source) => {
            return Err(ApplicationError::Io {
                operation: "inspect managed torrent part file",
                source,
            });
        }
    };
    if metadata.is_dir() && !metadata.file_type().is_symlink() {
        return Err(ApplicationError::Configuration(format!(
            "managed part-file path is a directory: {}",
            path.display()
        )));
    }
    std::fs::remove_file(path).map_err(|source| ApplicationError::Io {
        operation: "remove managed torrent part file",
        source,
    })
}

fn handle_task_outcome(
    store: &Arc<Mutex<SessionStore>>,
    storage_roots: &BTreeMap<String, StorageRootLocation>,
    views: &ViewHub,
    torrent_id: &str,
    outcome: Result<ApplicationTaskReport, DownloadError>,
) -> Result<(), String> {
    views
        .clear_disk_runtime(torrent_id)
        .map_err(|error| error.to_string())?;
    views
        .clear_piece_runtime(torrent_id)
        .map_err(|error| error.to_string())?;
    match outcome {
        Ok(report) => {
            views
                .set_progress_inputs(torrent_id, ProgressInputs::default())
                .map_err(|error| error.to_string())?;
            let operation = match report {
                ApplicationTaskReport::Metadata => "metadata",
                ApplicationTaskReport::Download => "download",
            };
            views
                .record_diagnostic(
                    DiagnosticSeverity::Info,
                    category::LIFECYCLE_TORRENT,
                    "engine_task_completed",
                    Some(torrent_id),
                    "Engine task completed",
                    &[("operation", operation)],
                )
                .map_err(|error| error.to_string())
        }
        Err(DownloadError::Cancelled) => {
            views
                .set_progress_inputs(torrent_id, ProgressInputs::default())
                .map_err(|error| error.to_string())?;
            views
                .record_diagnostic(
                    DiagnosticSeverity::Info,
                    category::LIFECYCLE_TORRENT,
                    "engine_task_cancelled",
                    Some(torrent_id),
                    "Engine task stopped at a cancellation point",
                    &[],
                )
                .map_err(|error| error.to_string())
        }
        Err(DownloadError::NetworkDisabled) => {
            views
                .set_progress_inputs(
                    torrent_id,
                    ProgressInputs {
                        network_disabled: true,
                        ..ProgressInputs::default()
                    },
                )
                .map_err(|error| error.to_string())?;
            views
                .record_diagnostic(
                    DiagnosticSeverity::Info,
                    category::DISCOVERY_PEER,
                    "network_disabled",
                    Some(torrent_id),
                    "Outbound networking is disabled by application policy",
                    &[],
                )
                .map_err(|error| error.to_string())
        }
        Err(error) if is_discovery_exhaustion(&error) => {
            views
                .set_progress_inputs(
                    torrent_id,
                    ProgressInputs {
                        discovery_exhausted: true,
                        ..ProgressInputs::default()
                    },
                )
                .map_err(|error| error.to_string())?;
            let detail = error.to_string();
            if matches!(error, DownloadError::NoUsableTrackerAddress) {
                views
                    .record_diagnostic(
                        DiagnosticSeverity::Warning,
                        category::TRACKER_ANNOUNCE,
                        "tracker_address_rejected",
                        Some(torrent_id),
                        "Tracker supplied no address allowed by the current network policy",
                        &[("detail", &detail)],
                    )
                    .map_err(|error| error.to_string())?;
            }
            views
                .record_diagnostic(
                    DiagnosticSeverity::Warning,
                    category::DISCOVERY_PEER,
                    "discovery_exhausted",
                    Some(torrent_id),
                    "No enabled discovery source can currently supply an eligible peer",
                    &[("detail", &detail)],
                )
                .map_err(|error| error.to_string())
        }
        Err(error) => {
            let cleanup_failed = matches!(&error, DownloadError::PeerCleanup { .. });
            let detail = error.to_string();
            {
                let mut store = store
                    .lock()
                    .map_err(|_| "session store lock is poisoned".to_owned())?;
                if matches!(error, DownloadError::SelectiveStorage(_)) {
                    store
                        .mark_needs_repair(torrent_id, &detail)
                        .map_err(|error| error.to_string())?;
                } else {
                    store
                        .mark_error(torrent_id, &detail)
                        .map_err(|error| error.to_string())?;
                }
                let (snapshot, durable) =
                    durable_view_state(&store, storage_roots).map_err(|error| error.to_string())?;
                drop(store);
                views
                    .replace_durable(&snapshot, &durable)
                    .map_err(|error| error.to_string())?;
            }
            views
                .set_progress_inputs(torrent_id, ProgressInputs::default())
                .map_err(|error| error.to_string())?;
            views
                .record_diagnostic(
                    DiagnosticSeverity::Error,
                    category::LIFECYCLE_TORRENT,
                    "engine_task_failed",
                    Some(torrent_id),
                    "Engine task failed",
                    &[("detail", &detail)],
                )
                .map_err(|error| error.to_string())?;
            if cleanup_failed { Err(detail) } else { Ok(()) }
        }
    }
}

fn is_discovery_exhaustion(error: &DownloadError) -> bool {
    matches!(
        error,
        DownloadError::NetworkPolicyDenied { .. }
            | DownloadError::UdpTracker(_)
            | DownloadError::NoUsablePeer
            | DownloadError::NoUsableTrackerAddress
            | DownloadError::UdpTrackerResponseTooLarge { .. }
            | DownloadError::UdpTrackerTimedOut { .. }
            | DownloadError::NetworkTimedOut { .. }
            | DownloadError::PeerClosed
            | DownloadError::PeerTimedOut { .. }
    )
}

#[derive(Debug)]
struct StoreCheckpointSink {
    store: Arc<Mutex<SessionStore>>,
    storage_roots: Arc<BTreeMap<String, StorageRootLocation>>,
    torrent_id: String,
    views: ViewHub,
}

impl StoreCheckpointSink {
    fn store(&self) -> Result<MutexGuard<'_, SessionStore>, String> {
        self.store
            .lock()
            .map_err(|_| "session store lock is poisoned".to_owned())
    }

    fn refresh(&self) -> Result<(), String> {
        let (snapshot, durable) = {
            let store = self.store()?;
            durable_view_state(&store, &self.storage_roots).map_err(|error| error.to_string())?
        };
        self.views
            .replace_durable(&snapshot, &durable)
            .map_err(|error| error.to_string())
    }

    fn waiting_for_storage(&self) -> Result<(), String> {
        self.store().and_then(|mut store| {
            store
                .mark_awaiting_storage(&self.torrent_id, None)
                .map(|_| ())
                .map_err(|error| error.to_string())
        })?;
        self.refresh()?;
        self.views
            .record_diagnostic(
                DiagnosticSeverity::Info,
                category::STORAGE_IO,
                "storage_selection_required",
                Some(&self.torrent_id),
                "Verified metadata is waiting for platform storage",
                &[],
            )
            .map_err(|error| error.to_string())
    }
}

impl DownloadCheckpointSink for StoreCheckpointSink {
    fn metadata_verified(&self, raw_info: &[u8]) -> Result<(), String> {
        self.store().and_then(|mut store| {
            store
                .record_metadata(&self.torrent_id, raw_info)
                .map(|_| ())
                .map_err(|error| error.to_string())
        })?;
        self.refresh()?;
        self.views
            .record_diagnostic(
                DiagnosticSeverity::Info,
                category::METADATA_EXCHANGE,
                "metadata_verified",
                Some(&self.torrent_id),
                "Torrent metadata verified",
                &[],
            )
            .map_err(|error| error.to_string())
    }

    fn storage_prepared(&self, storage: ResumedStorage) -> Result<(), String> {
        let storage_state = match storage {
            ResumedStorage::Created | ResumedStorage::Staging => StorageState::Staging,
            ResumedStorage::Published => StorageState::Published,
        };
        self.store().and_then(|mut store| {
            store
                .mark_storage_prepared(&self.torrent_id, storage_state)
                .map(|_| ())
                .map_err(|error| error.to_string())
        })?;
        self.refresh()?;
        self.views
            .record_diagnostic(
                DiagnosticSeverity::Info,
                category::STORAGE_IO,
                "storage_prepared",
                Some(&self.torrent_id),
                "Torrent storage prepared",
                &[],
            )
            .map_err(|error| error.to_string())
    }

    fn have_rechecked(&self, verified_pieces: &[bool]) -> Result<(), String> {
        let resume = self.store().and_then(|store| {
            store
                .load_resume(&self.torrent_id)
                .map_err(|error| error.to_string())
        })?;
        let have = resume
            .have
            .ok_or_else(|| "metadata checkpoint did not create have state".to_owned())?;
        let replacement = HaveState::from_pieces(have.info_hash(), verified_pieces.to_vec())
            .map_err(|error| error.to_string())?;
        self.store().and_then(|mut store| {
            store
                .replace_have(&self.torrent_id, &replacement)
                .map(|_| ())
                .map_err(|error| error.to_string())
        })?;
        self.refresh()?;
        self.views
            .record_diagnostic(
                DiagnosticSeverity::Info,
                category::INTEGRITY_HASH,
                "have_rechecked",
                Some(&self.torrent_id),
                "Existing piece state rechecked",
                &[],
            )
            .map_err(|error| error.to_string())
    }

    fn pieces_durable(&self, piece_indices: &[usize]) -> Result<(), String> {
        if piece_indices.is_empty() {
            return Err("durable piece batch must be nonempty".to_owned());
        }
        let mut durable_indices = piece_indices.to_vec();
        durable_indices.sort_unstable();
        durable_indices.dedup();
        let view_indices = durable_indices
            .iter()
            .copied()
            .map(|piece_index| {
                u32::try_from(piece_index)
                    .map_err(|_| "durable piece index overflows u32".to_owned())
            })
            .collect::<Result<Vec<_>, _>>()?;
        let revision = self.store().and_then(|mut store| {
            store
                .record_pieces(&self.torrent_id, &durable_indices)
                .map_err(|error| error.to_string())
        })?;
        self.views
            .record_pieces_durable(&self.torrent_id, &view_indices, revision)
            .map_err(|error| error.to_string())?;
        let piece_count = durable_indices.len().to_string();
        self.views
            .record_diagnostic(
                DiagnosticSeverity::Debug,
                category::PIECE_BLOCK,
                "pieces_durable",
                Some(&self.torrent_id),
                "Verified pieces became durable",
                &[("piece_count", &piece_count)],
            )
            .map_err(|error| error.to_string())
    }

    fn descriptor_prepared(&self, files: &[PreparedFileHash]) -> Result<(), String> {
        self.store().and_then(|mut store| {
            store
                .record_prepared_files(&self.torrent_id, files)
                .map(|_| ())
                .map_err(|error| error.to_string())
        })?;
        self.refresh()?;
        self.views
            .record_diagnostic(
                DiagnosticSeverity::Info,
                category::STORAGE_IO,
                "publication_prepared",
                Some(&self.torrent_id),
                "Payload files prepared for publication",
                &[],
            )
            .map_err(|error| error.to_string())
    }

    fn published(&self) -> Result<(), String> {
        self.store().and_then(|mut store| {
            store
                .mark_complete(&self.torrent_id)
                .map(|_| ())
                .map_err(|error| error.to_string())
        })?;
        self.refresh()?;
        self.views
            .record_diagnostic(
                DiagnosticSeverity::Info,
                category::LIFECYCLE_TORRENT,
                "torrent_completed",
                Some(&self.torrent_id),
                "Torrent completed",
                &[],
            )
            .map_err(|error| error.to_string())
    }
}

#[derive(Debug)]
struct ViewActivitySink {
    torrent_id: String,
    views: ViewHub,
    trace_checkpoint_stages: bool,
    last_checkpoint_stage: Mutex<Option<DiskCheckpointStage>>,
}

const fn checkpoint_stage_name(stage: DiskCheckpointStage) -> &'static str {
    match stage {
        DiskCheckpointStage::Idle => "idle",
        DiskCheckpointStage::Syncing => "syncing",
        DiskCheckpointStage::Committing => "committing",
        DiskCheckpointStage::Error => "error",
    }
}

fn piece_diagnostic_context(
    event: &DownloadActivityEvent,
) -> (Vec<DiagnosticSubject>, Vec<DiagnosticField>) {
    let (piece_index, attempt) = match event {
        DownloadActivityEvent::PieceStarted {
            piece_index,
            attempt,
            ..
        } => (*piece_index, Some(*attempt)),
        DownloadActivityEvent::BlockRequested { piece_index, .. }
        | DownloadActivityEvent::BlockReceived { piece_index, .. }
        | DownloadActivityEvent::BlockStored { piece_index, .. }
        | DownloadActivityEvent::PieceHashing { piece_index }
        | DownloadActivityEvent::PieceVerified { piece_index } => (*piece_index, None),
        _ => return (Vec::new(), Vec::new()),
    };
    let fields = match event {
        DownloadActivityEvent::PieceStarted { piece_length, .. } => {
            vec![DiagnosticField::bytes(
                "piece_length",
                u64::from(*piece_length),
            )]
        }
        DownloadActivityEvent::BlockRequested { begin, length, .. }
        | DownloadActivityEvent::BlockReceived { begin, length, .. }
        | DownloadActivityEvent::BlockStored { begin, length, .. } => vec![
            DiagnosticField::bytes("block_offset", u64::from(*begin)),
            DiagnosticField::bytes("block_length", u64::from(*length)),
        ],
        _ => Vec::new(),
    };
    (
        vec![DiagnosticSubject::Piece {
            piece_index,
            attempt,
        }],
        fields,
    )
}

impl DownloadActivitySink for ViewActivitySink {
    fn record(&self, event: DownloadActivityEvent) {
        if let DownloadActivityEvent::PeerConnections { captured_at, peers } = &event {
            let _ = self.views.record_peer_connections(
                &self.torrent_id,
                *captured_at,
                peers.as_slice(),
            );
            return;
        }
        if let DownloadActivityEvent::TrackerState(snapshot) = &event {
            let _ = self.views.record_tracker_state(&self.torrent_id, snapshot);
            return;
        }
        if let DownloadActivityEvent::StorageState(snapshot) = &event {
            if self.trace_checkpoint_stages {
                let mut previous = self
                    .last_checkpoint_stage
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                if *previous != Some(snapshot.checkpoint_stage) {
                    *previous = Some(snapshot.checkpoint_stage);
                    eprintln!(
                        "checkpoint_stage={} dirty_pieces={} dirty_bytes={} batches_completed={}",
                        checkpoint_stage_name(snapshot.checkpoint_stage),
                        snapshot.checkpoint_dirty_pieces,
                        snapshot.checkpoint_dirty_bytes,
                        snapshot.checkpoint_batches_completed,
                    );
                }
            }
            let _ = self.views.record_disk_runtime(&self.torrent_id, snapshot);
            let _ = self
                .views
                .record_piece_runtime(&self.torrent_id, &snapshot.pieces);
            return;
        }
        let piece_activity = match &event {
            DownloadActivityEvent::MetadataVerified { .. } => {
                return self.record_discovery_event(event);
            }
            DownloadActivityEvent::PieceStarted {
                piece_index,
                piece_length,
                attempt,
            } => TorrentActivity::PieceStarted {
                piece_index: *piece_index,
                piece_length: *piece_length,
                attempt: *attempt,
            },
            DownloadActivityEvent::BlockRequested {
                piece_index,
                begin,
                length,
            } => TorrentActivity::BlockRequested {
                piece_index: *piece_index,
                begin: *begin,
                length: *length,
            },
            DownloadActivityEvent::BlockReceived {
                piece_index,
                begin,
                length,
            } => TorrentActivity::BlockReceived {
                piece_index: *piece_index,
                begin: *begin,
                length: *length,
            },
            DownloadActivityEvent::BlockStored {
                piece_index,
                begin,
                length,
            } => TorrentActivity::BlockStored {
                piece_index: *piece_index,
                begin: *begin,
                length: *length,
            },
            DownloadActivityEvent::PieceVerified { piece_index } => {
                TorrentActivity::PieceVerified {
                    piece_index: *piece_index,
                }
            }
            DownloadActivityEvent::PieceHashFailed { piece_index, .. } => {
                TorrentActivity::PieceHashFailed {
                    piece_index: *piece_index,
                }
            }
            DownloadActivityEvent::PieceHashing { piece_index } => TorrentActivity::PieceHashing {
                piece_index: *piece_index,
            },
            _ => return self.record_discovery_event(event),
        };
        let _ = self.views.record_activity(&self.torrent_id, piece_activity);
        if matches!(event, DownloadActivityEvent::PieceHashFailed { .. }) {
            return self.record_discovery_event(event);
        }
        let (severity, category, code, summary) = match event {
            DownloadActivityEvent::PieceStarted { .. } => (
                DiagnosticSeverity::Debug,
                category::SCHEDULER_REQUEST,
                "piece_started",
                "Piece transfer started",
            ),
            DownloadActivityEvent::BlockRequested { .. } => (
                DiagnosticSeverity::Trace,
                category::PEER_PROTOCOL,
                "block_requested",
                "Piece block requested",
            ),
            DownloadActivityEvent::BlockReceived { .. } => (
                DiagnosticSeverity::Trace,
                category::PEER_PROTOCOL,
                "block_received",
                "Piece block received",
            ),
            DownloadActivityEvent::BlockStored { .. } => (
                DiagnosticSeverity::Trace,
                category::STORAGE_IO,
                "block_stored",
                "Piece block stored",
            ),
            DownloadActivityEvent::PieceHashing { .. } => (
                DiagnosticSeverity::Debug,
                category::INTEGRITY_HASH,
                "piece_hashing",
                "Piece hash verification started",
            ),
            DownloadActivityEvent::PieceVerified { .. } => (
                DiagnosticSeverity::Info,
                category::INTEGRITY_HASH,
                "piece_verified",
                "Piece hash verified",
            ),
            _ => unreachable!("discovery events returned before piece diagnostics"),
        };
        let diagnostic_torrent_id = self.torrent_id.clone();
        let _ =
            self.views
                .record_diagnostic_lazy(severity, category, Some(&self.torrent_id), || {
                    let (subjects, fields) = piece_diagnostic_context(&event);
                    DiagnosticDraft {
                        severity,
                        category: DiagnosticCategory::from_static(category),
                        code: code.to_owned(),
                        torrent_id: Some(diagnostic_torrent_id),
                        message: summary.to_owned(),
                        subjects,
                        fields,
                    }
                });
    }
}

impl ViewActivitySink {
    fn record_structured(
        &self,
        severity: DiagnosticSeverity,
        category: &'static str,
        code: &'static str,
        message: &'static str,
        subjects: Vec<DiagnosticSubject>,
        fields: Vec<DiagnosticField>,
    ) {
        let _ = self.views.record_structured_diagnostic(DiagnosticDraft {
            severity,
            category: DiagnosticCategory::from_static(category),
            code: code.to_owned(),
            torrent_id: Some(self.torrent_id.clone()),
            message: message.to_owned(),
            subjects,
            fields,
        });
    }

    fn record_discovery_event(&self, event: DownloadActivityEvent) {
        match event {
            DownloadActivityEvent::MetadataVerified {
                total_length,
                piece_length,
                piece_count,
                file_count,
            } => {
                self.record_structured(
                    DiagnosticSeverity::Info,
                    category::METADATA_EXCHANGE,
                    "metadata_verified",
                    "Torrent metadata verified",
                    Vec::new(),
                    vec![
                        DiagnosticField::bytes("total_length", total_length),
                        DiagnosticField::bytes("piece_length", u64::from(piece_length)),
                        DiagnosticField::count(
                            "piece_count",
                            u64::try_from(piece_count).unwrap_or(u64::MAX),
                        ),
                        DiagnosticField::count(
                            "file_count",
                            u64::try_from(file_count).unwrap_or(u64::MAX),
                        ),
                    ],
                );
            }
            DownloadActivityEvent::TrackerAnnounceStarted {
                tracker,
                tier,
                attempt,
                event,
            } => {
                let _ = self
                    .views
                    .set_discovery_activity(&self.torrent_id, true, false);
                let announce_event = format!("{event:?}").to_ascii_lowercase();
                self.record_structured(
                    DiagnosticSeverity::Info,
                    category::TRACKER_ANNOUNCE,
                    "tracker_announce_started",
                    "Contacting UDP tracker",
                    vec![DiagnosticSubject::Tracker {
                        tracker_id: tracker,
                    }],
                    vec![
                        DiagnosticField::count("tier", u64::from(tier)),
                        DiagnosticField::count("attempt", u64::from(attempt)),
                        DiagnosticField::text("event", announce_event),
                    ],
                );
            }
            DownloadActivityEvent::TrackerUdpRetransmitted { tracker, operation } => {
                let _ = self.views.record_diagnostic(
                    DiagnosticSeverity::Debug,
                    category::TRACKER_ANNOUNCE,
                    "tracker_udp_retransmitted",
                    Some(&self.torrent_id),
                    "UDP tracker request retransmitted after silence",
                    &[("tracker", &tracker), ("operation", operation)],
                );
            }
            DownloadActivityEvent::TrackerAnnounceFailed {
                tracker,
                failures,
                retry_in_seconds,
                detail,
            } => {
                self.record_structured(
                    DiagnosticSeverity::Warning,
                    category::TRACKER_ANNOUNCE,
                    "tracker_announce_failed",
                    "UDP tracker announce failed temporarily",
                    vec![DiagnosticSubject::Tracker {
                        tracker_id: tracker,
                    }],
                    vec![
                        DiagnosticField::count("failures", u64::from(failures)),
                        DiagnosticField::duration_millis(
                            "retry_in",
                            retry_in_seconds.saturating_mul(1_000),
                        ),
                        DiagnosticField::text("detail", detail),
                    ],
                );
            }
            DownloadActivityEvent::TrackerFallbackSelected { tracker, tier } => {
                let tier = tier.to_string();
                let _ = self.views.record_diagnostic(
                    DiagnosticSeverity::Info,
                    category::TRACKER_ANNOUNCE,
                    "tracker_fallback_selected",
                    Some(&self.torrent_id),
                    "Trying another tracker in the tier",
                    &[("tracker", &tracker), ("tier", &tier)],
                );
            }
            DownloadActivityEvent::TrackerRetryScheduled {
                tracker,
                retry_in_seconds,
            } => {
                let _ = self
                    .views
                    .set_discovery_activity(&self.torrent_id, false, true);
                let retry = retry_in_seconds.to_string();
                let _ = self.views.record_diagnostic(
                    DiagnosticSeverity::Info,
                    category::TRACKER_ANNOUNCE,
                    "tracker_retry_scheduled",
                    Some(&self.torrent_id),
                    "Tracker discovery will retry automatically",
                    &[("tracker", &tracker), ("retry_seconds", &retry)],
                );
            }
            DownloadActivityEvent::TrackerReannounceScheduled {
                tracker,
                announce_in_seconds,
            } => {
                let announce = announce_in_seconds.to_string();
                let _ = self.views.record_diagnostic(
                    DiagnosticSeverity::Debug,
                    category::TRACKER_ANNOUNCE,
                    "tracker_reannounce_scheduled",
                    Some(&self.torrent_id),
                    "Tracker accepted the announce interval",
                    &[("tracker", &tracker), ("announce_seconds", &announce)],
                );
            }
            DownloadActivityEvent::TrackerAnnounceSucceeded {
                tracker,
                peer_count,
                interval_seconds,
            } => {
                self.record_structured(
                    DiagnosticSeverity::Info,
                    category::TRACKER_ANNOUNCE,
                    "tracker_announce_succeeded",
                    "UDP tracker announce succeeded",
                    vec![DiagnosticSubject::Tracker {
                        tracker_id: tracker,
                    }],
                    vec![
                        DiagnosticField::count("peers", u64::from(peer_count)),
                        DiagnosticField::duration_millis(
                            "announce_interval",
                            interval_seconds.saturating_mul(1_000),
                        ),
                    ],
                );
            }
            DownloadActivityEvent::TrackerPeersUnavailable {
                tracker,
                peer_count,
            } => {
                let _ = self
                    .views
                    .set_discovery_activity(&self.torrent_id, false, true);
                let peers = peer_count.to_string();
                let _ = self.views.record_diagnostic(
                    DiagnosticSeverity::Info,
                    category::PEER_CONNECTION,
                    "tracker_peers_unavailable",
                    Some(&self.torrent_id),
                    "Tracker response has no currently eligible peer",
                    &[("tracker", &tracker), ("peers", &peers)],
                );
            }
            DownloadActivityEvent::DhtLookupStarted => {
                let _ = self
                    .views
                    .set_discovery_activity(&self.torrent_id, true, false);
                let _ = self.views.record_diagnostic(
                    DiagnosticSeverity::Info,
                    category::DISCOVERY_DHT,
                    "dht_lookup_started",
                    Some(&self.torrent_id),
                    "Searching the distributed hash table for peers",
                    &[],
                );
            }
            DownloadActivityEvent::DhtLookupSucceeded { peer_count } => {
                let peers = peer_count.to_string();
                let _ = self.views.record_diagnostic(
                    DiagnosticSeverity::Info,
                    category::DISCOVERY_DHT,
                    "dht_lookup_succeeded",
                    Some(&self.torrent_id),
                    "DHT lookup returned peers",
                    &[("peers", &peers)],
                );
            }
            DownloadActivityEvent::DhtLookupFailed { detail } => {
                let _ = self
                    .views
                    .set_discovery_activity(&self.torrent_id, false, true);
                let _ = self.views.record_diagnostic(
                    DiagnosticSeverity::Warning,
                    category::DISCOVERY_DHT,
                    "dht_lookup_failed",
                    Some(&self.torrent_id),
                    "DHT lookup ended without a peer and may be retried",
                    &[("detail", &detail)],
                );
            }
            DownloadActivityEvent::DhtRetryScheduled { retry_in_seconds } => {
                let retry = retry_in_seconds.to_string();
                let _ = self.views.record_diagnostic(
                    DiagnosticSeverity::Info,
                    category::DISCOVERY_DHT,
                    "dht_retry_scheduled",
                    Some(&self.torrent_id),
                    "DHT lookup will retry after bounded backoff",
                    &[("retry_in_seconds", &retry)],
                );
            }
            DownloadActivityEvent::DhtDisabledForPrivateTorrent => {
                let _ = self.views.record_diagnostic(
                    DiagnosticSeverity::Info,
                    category::DISCOVERY_DHT,
                    "dht_disabled_private_torrent",
                    Some(&self.torrent_id),
                    "Verified private metadata disabled decentralized discovery",
                    &[],
                );
            }
            DownloadActivityEvent::PeerDialStarted { peer: _ } => {
                let _ = self
                    .views
                    .set_discovery_activity(&self.torrent_id, true, false);
                let _ = self.views.record_diagnostic(
                    DiagnosticSeverity::Info,
                    category::PEER_CONNECTION,
                    "peer_dial_started",
                    Some(&self.torrent_id),
                    "Connecting to discovered peer",
                    &[],
                );
            }
            DownloadActivityEvent::SwarmState(snapshot) => {
                let connected = snapshot.connected_peers.to_string();
                let pending = snapshot.pending_dials.to_string();
                let unchoked = snapshot.unchoked_peers.to_string();
                let missing = snapshot.missing_blocks.to_string();
                let requested = snapshot.requested_blocks.to_string();
                let writing = snapshot.writing_blocks.to_string();
                let reserved = snapshot.outstanding_request_bytes.to_string();
                let oldest = snapshot
                    .oldest_request_age_seconds
                    .map_or_else(|| "none".to_owned(), |value| value.to_string());
                let next_expiry = snapshot
                    .next_request_expiry_seconds
                    .map_or_else(|| "none".to_owned(), |value| value.to_string());
                let next_replacement = snapshot
                    .next_replacement_seconds
                    .map_or_else(|| "none".to_owned(), |value| value.to_string());
                let reason = snapshot
                    .no_request_reason
                    .map_or_else(|| "requestable".to_owned(), |reason| format!("{reason:?}"));
                let _ = self.views.record_diagnostic(
                    DiagnosticSeverity::Debug,
                    category::PERFORMANCE_BACKPRESSURE,
                    "swarm_state_changed",
                    Some(&self.torrent_id),
                    "Torrent request state changed",
                    &[
                        ("connected_peers", &connected),
                        ("pending_dials", &pending),
                        ("unchoked_peers", &unchoked),
                        ("missing_blocks", &missing),
                        ("requested_blocks", &requested),
                        ("writing_blocks", &writing),
                        ("outstanding_request_bytes", &reserved),
                        ("oldest_request_seconds", &oldest),
                        ("next_expiry_seconds", &next_expiry),
                        ("next_replacement_seconds", &next_replacement),
                        ("no_request_reason", &reason),
                    ],
                );
            }
            DownloadActivityEvent::PieceHashFailed {
                piece_index,
                contributor_count,
                failed_bytes,
            } => {
                let piece_index = piece_index.to_string();
                let contributors = contributor_count.to_string();
                let failed_bytes = failed_bytes.to_string();
                let _ = self.views.record_diagnostic(
                    DiagnosticSeverity::Warning,
                    category::INTEGRITY_HASH,
                    "piece_hash_failed",
                    Some(&self.torrent_id),
                    "Piece hash failed; retrying the entire piece",
                    &[
                        ("piece_index", &piece_index),
                        ("contributors", &contributors),
                        ("failed_bytes", &failed_bytes),
                    ],
                );
            }
            DownloadActivityEvent::PieceStarted { .. }
            | DownloadActivityEvent::BlockRequested { .. }
            | DownloadActivityEvent::BlockReceived { .. }
            | DownloadActivityEvent::BlockStored { .. }
            | DownloadActivityEvent::PieceHashing { .. }
            | DownloadActivityEvent::PieceVerified { .. } => {
                unreachable!("piece events are handled before discovery events")
            }
            DownloadActivityEvent::PeerConnections { .. } => {
                unreachable!("peer projections are handled before diagnostic events")
            }
            DownloadActivityEvent::TrackerState(_) => {
                unreachable!("tracker projections are handled before diagnostic events")
            }
            DownloadActivityEvent::StorageState(_) => {
                unreachable!("disk projections are handled before diagnostic events")
            }
        }
    }
}

fn available_storage_roots(roots: Vec<StoredStorageRoot>) -> BTreeMap<String, StorageRootLocation> {
    roots
        .into_iter()
        .filter(|root| match &root.location {
            StorageRootLocation::Path(path) => path.is_dir() && std::fs::read_dir(path).is_ok(),
            StorageRootLocation::PlatformCapability => true,
        })
        .map(|root| (root.id, root.location))
        .collect()
}

fn validate_selected_directory(path: &Path) -> Result<PathBuf, ApplicationError> {
    let path = std::fs::canonicalize(path).map_err(|source| ApplicationError::Io {
        operation: "resolve selected storage root",
        source,
    })?;
    if !path.is_dir() {
        return Err(ApplicationError::Configuration(
            "selected storage root is not a directory".to_owned(),
        ));
    }
    std::fs::read_dir(&path).map_err(|source| ApplicationError::Io {
        operation: "read selected storage root",
        source,
    })?;
    Ok(path)
}

fn storage_root_label(path: &Path) -> String {
    let mut label = path
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .or_else(|| path.to_str())
        .unwrap_or("Download folder")
        .to_owned();
    if label.len() > crate::control::MAX_ROOT_LABEL_LENGTH {
        label.truncate(label.floor_char_boundary(crate::control::MAX_ROOT_LABEL_LENGTH));
    }
    label
}

fn durable_view_state(
    store: &SessionStore,
    storage_roots: &BTreeMap<String, StorageRootLocation>,
) -> Result<
    (
        crate::ServiceSnapshot,
        BTreeMap<String, DurableTorrentViewState>,
    ),
    ApplicationError,
> {
    let snapshot = store.snapshot()?;
    let mut durable = BTreeMap::new();
    for torrent in &snapshot.torrents {
        let Ok(resume) = store.load_resume(&torrent.torrent_id) else {
            durable.insert(
                torrent.torrent_id.clone(),
                DurableTorrentViewState {
                    display_name: None,
                    verified: Vec::new(),
                    files: None,
                    trackers: TrackerViewModel::default(),
                },
            );
            continue;
        };
        let verified_pieces = resume.have.as_ref().map_or(&[][..], |have| have.pieces());
        let verified_indices = verified_pieces
            .iter()
            .enumerate()
            .filter(|(_, verified)| **verified)
            .map(|(index, _)| u32::try_from(index))
            .collect::<Result<Vec<_>, _>>()
            .map_err(|_| ApplicationError::Configuration("piece index overflows u32".to_owned()))?;
        let metainfo = resume
            .raw_info
            .as_deref()
            .and_then(|raw_info| Metainfo::from_info_bytes(raw_info).ok());
        let display_name = metainfo.as_ref().map(|metainfo| metainfo.name.clone());
        let files = if let Some(metainfo) = metainfo.as_ref() {
            let filesystem_content_base = filesystem_content_base(
                storage_roots.get(&resume.storage_root),
                &torrent.torrent_id,
            )?;
            FileProgressModel::new(
                metainfo,
                &resume.skip_files,
                &verified_indices,
                filesystem_content_base,
            )
            .ok()
        } else {
            None
        };
        let trackers = TrackerViewModel::from_magnet(&resume.magnet);
        durable.insert(
            torrent.torrent_id.clone(),
            DurableTorrentViewState {
                display_name,
                verified: ranges_from_pieces(verified_pieces),
                files,
                trackers,
            },
        );
    }
    Ok((snapshot, durable))
}

fn filesystem_content_base(
    storage_root: Option<&StorageRootLocation>,
    torrent_id: &str,
) -> Result<Option<String>, ApplicationError> {
    let Some(StorageRootLocation::Path(root)) = storage_root else {
        return Ok(None);
    };
    let root = if root.is_absolute() {
        root.clone()
    } else {
        std::env::current_dir()
            .map_err(|source| ApplicationError::Io {
                operation: "resolve storage root",
                source,
            })?
            .join(root)
    };
    root.join(torrent_id)
        .into_os_string()
        .into_string()
        .map(Some)
        .map_err(|_| ApplicationError::Configuration("storage path is not UTF-8".to_owned()))
}

#[derive(Debug)]
pub enum ApplicationError {
    Store(StoreError),
    Io {
        operation: &'static str,
        source: std::io::Error,
    },
    Configuration(String),
    Busy(String),
    Join(String),
    StorePoisoned,
    Subscription(SubscriptionError),
    Dht(DhtError),
}

impl fmt::Display for ApplicationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Store(error) => write!(formatter, "{error}"),
            Self::Io { operation, source } => write!(formatter, "{operation}: {source}"),
            Self::Configuration(message) => {
                write!(formatter, "application configuration: {message}")
            }
            Self::Busy(torrent_id) => {
                write!(
                    formatter,
                    "torrent {torrent_id} already owns the download slot"
                )
            }
            Self::Join(message) => write!(formatter, "engine task join: {message}"),
            Self::StorePoisoned => write!(formatter, "session store lock is poisoned"),
            Self::Subscription(error) => write!(formatter, "{error}"),
            Self::Dht(error) => write!(formatter, "{error}"),
        }
    }
}

impl Error for ApplicationError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Store(error) => Some(error),
            Self::Io { source, .. } => Some(source),
            Self::Subscription(error) => Some(error),
            Self::Dht(error) => Some(error),
            _ => None,
        }
    }
}

impl From<StoreError> for ApplicationError {
    fn from(error: StoreError) -> Self {
        Self::Store(error)
    }
}

impl From<DhtError> for ApplicationError {
    fn from(error: DhtError) -> Self {
        Self::Dht(error)
    }
}

impl From<SubscriptionError> for ApplicationError {
    fn from(error: SubscriptionError) -> Self {
        Self::Subscription(error)
    }
}

fn encode_info_hash(info_hash: [u8; 20]) -> String {
    let mut output = String::with_capacity(40);
    const HEX: &[u8; 16] = b"0123456789abcdef";
    for byte in info_hash {
        output.push(char::from(HEX[usize::from(byte >> 4)]));
        output.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    output
}

pub fn application_error_response(
    request_id: String,
    revision: u64,
    error: &ApplicationError,
) -> ResponseEnvelope {
    let code = match error {
        ApplicationError::Busy(_) => ErrorCode::Busy,
        ApplicationError::Configuration(_) => ErrorCode::InvalidRequest,
        ApplicationError::Store(StoreError::UnknownTorrent(_)) => ErrorCode::UnknownTorrent,
        ApplicationError::Store(StoreError::DurableState(_) | StoreError::Have(_)) => {
            ErrorCode::InvalidDurableState
        }
        _ => ErrorCode::Internal,
    };
    ResponseEnvelope::error(request_id, revision, code, error.to_string())
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::net::{IpAddr, SocketAddr};
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicU64, Ordering};

    use rstorrent_engine::dht::BootstrapNode;
    use rstorrent_engine::{DownloadError, NetworkConfig, NetworkPolicy};
    use rstorrent_protocol::dht::{
        DhtEndpoint, DhtIp, Message as DhtMessage, NodeId, decode_message as decode_dht,
        encode_response as encode_dht_response,
    };
    use rstorrent_protocol::peer_wire::{
        HANDSHAKE_LENGTH, PeerMessage, decode_handshake, encode_handshake, encode_message,
    };
    use rusqlite::Connection;
    use sha1::{Digest, Sha1};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::{TcpListener, UdpSocket};

    use super::{ApplicationConfig, ApplicationService, handle_task_outcome};
    use crate::{
        CONTROL_VERSION, Command, ConfiguredStorageRoot, DeliveryPolicy, DiagnosticFilter,
        DiagnosticProfile, DiagnosticSeverity, OpenViewSetOptions, OpenViewSetRequest,
        ProgressDisposition, ProgressReason, RemovalDataPolicy, RemovalState, RequestEnvelope,
        ResponseOutcome, SessionStore, SubscriptionSpec, TorrentState, ViewDeliveryPolicy,
        ViewPatch, ViewProjection, ViewSelector, ViewSetError, ViewSetOwner, ViewSetUpdate,
        ViewSnapshot, ViewSpec, ViewUpdatePayload,
    };

    static NEXT_TEST: AtomicU64 = AtomicU64::new(0);

    fn test_root(label: &str) -> PathBuf {
        let sequence = NEXT_TEST.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!(
            "rstorrent-application-{label}-{}-{sequence}",
            std::process::id()
        ))
    }

    fn config(root: &Path) -> ApplicationConfig {
        ApplicationConfig::new(
            root.join("profile"),
            "test".to_owned(),
            vec![ConfiguredStorageRoot::path(
                "downloads",
                root.join("payload"),
            )],
            NetworkConfig::new(
                NetworkPolicy::LoopbackOnly,
                std::time::Duration::from_secs(5),
                std::time::Duration::from_secs(5),
            ),
        )
    }

    fn add_request(request_id: &str, torrent_id: &str) -> RequestEnvelope {
        RequestEnvelope {
            version: CONTROL_VERSION,
            request_id: request_id.to_owned(),
            expected_revision: None,
            command: Command::AddMagnet {
                magnet: format!("magnet:?xt=urn:btih:{torrent_id}&x.pe=127.0.0.1:1"),
                storage_root: "downloads".to_owned(),
                skip_files: Vec::new(),
            },
        }
    }

    fn dht_endpoint(address: SocketAddr) -> DhtEndpoint {
        let port = address.port();
        match address.ip() {
            IpAddr::V4(address) => DhtEndpoint::new(DhtIp::V4(address.octets()), port),
            IpAddr::V6(address) => DhtEndpoint::new(DhtIp::V6(address.octets()), port),
        }
    }

    #[tokio::test]
    async fn application_shutdown_closes_view_sets_and_wakes_waiters() {
        let root = test_root("view-set-shutdown");
        let mut service = ApplicationService::open(config(&root))
            .await
            .expect("open application");
        let owner = ViewSetOwner::trusted("test-owner");
        let opened = service
            .open_view_set(
                owner.clone(),
                OpenViewSetRequest {
                    views: vec![ViewSpec::TorrentList {
                        view_id: "library".to_owned(),
                        delivery: ViewDeliveryPolicy::default(),
                    }],
                    options: OpenViewSetOptions::default(),
                },
            )
            .expect("open view set");
        let view_set = service
            .view_set(&owner, &opened.view_set_id)
            .expect("view set handle");
        let cursor = opened.initial.cursor;
        let waiter = tokio::spawn(async move { view_set.next_updates(&cursor, 20_000).await });
        tokio::task::yield_now().await;

        service.shutdown().await.expect("shutdown");
        let result = tokio::time::timeout(std::time::Duration::from_secs(1), waiter)
            .await
            .expect("waiter timed out")
            .expect("waiter task");
        assert_eq!(result, Err(ViewSetError::Closed));
        drop(service);
        fs::remove_dir_all(root).expect("remove test root");
    }

    async fn answer_dht_query(router: &UdpSocket) {
        let mut packet = [0_u8; 1024];
        let (length, client) = tokio::time::timeout(
            std::time::Duration::from_secs(1),
            router.recv_from(&mut packet),
        )
        .await
        .expect("DHT query timed out")
        .expect("receive DHT query");
        let DhtMessage::Query(query) = decode_dht(&packet[..length]).expect("decode DHT query")
        else {
            panic!("DHT query expected");
        };
        let response = encode_dht_response(
            &query.transaction,
            NodeId([9; 20]),
            &[],
            &[],
            None,
            dht_endpoint(client),
        )
        .expect("encode DHT response");
        router
            .send_to(&response, client)
            .await
            .expect("send DHT response");
    }

    #[tokio::test]
    async fn application_shutdown_persists_and_revalidates_warm_dht_node() {
        let root = test_root("dht-warm-restart");
        let router = UdpSocket::bind("127.0.0.1:0")
            .await
            .expect("bind DHT router");
        let router_address = router.local_addr().expect("DHT router address");
        let mut first_config = config(&root);
        first_config.dht.bootstrap_nodes = vec![BootstrapNode::Address(router_address)];
        let mut first = ApplicationService::open(first_config)
            .await
            .expect("open first application");
        answer_dht_query(&router).await;
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        first.shutdown().await.expect("first shutdown");
        drop(first);

        let mut persisted = config(&root);
        persisted.dht.bootstrap_nodes.clear();
        let store = SessionStore::open(
            &persisted.profile_root,
            &persisted.profile_id,
            &persisted.storage_roots,
        )
        .expect("open persisted store");
        let snapshot = store
            .load_dht_snapshot()
            .expect("load DHT snapshot")
            .expect("saved DHT snapshot");
        assert_eq!(snapshot.nodes_v4[0].address, dht_endpoint(router_address));
        drop(store);

        let mut second = ApplicationService::open(persisted)
            .await
            .expect("open second application");
        answer_dht_query(&router).await;
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        second.shutdown().await.expect("second shutdown");
        drop(second);
        fs::remove_dir_all(root).expect("remove test root");
    }

    #[tokio::test]
    async fn command_mutation_publishes_a_typed_view_patch() {
        let root = test_root("view-patch");
        let torrent_id = "000102030405060708090a0b0c0d0e0f10111213";
        let mut service = ApplicationService::open(config(&root))
            .await
            .expect("open service");
        let subscription = service
            .subscribe(SubscriptionSpec {
                selector: ViewSelector::TorrentList,
                projection: ViewProjection::Summary,
                delivery: DeliveryPolicy {
                    min_interval_millis: 0,
                    max_queue_bytes: 4096,
                },
                diagnostics: None,
            })
            .expect("subscribe");
        assert!(matches!(
            subscription.next_update().await.expect("initial").payload,
            ViewUpdatePayload::Snapshot { .. }
        ));

        service
            .dispatch(add_request("add", torrent_id))
            .await
            .expect("add");
        let update = tokio::time::timeout(
            std::time::Duration::from_secs(1),
            subscription.next_update(),
        )
        .await
        .expect("view update timed out")
        .expect("view update");
        assert_eq!(update.base_revision, "0");
        assert_eq!(update.revision, "1");
        let ViewUpdatePayload::Patch { patch } = update.payload else {
            panic!("mutation must publish a patch");
        };
        assert!(
            serde_json::to_string(&patch)
                .expect("serialize patch")
                .contains(torrent_id)
        );

        service.shutdown().await.expect("shutdown");
        drop(service);
        fs::remove_dir_all(root).expect("remove test root");
    }

    #[tokio::test]
    async fn pause_is_durable_and_prevents_restart() {
        let root = test_root("pause");
        let torrent_id = "000102030405060708090a0b0c0d0e0f10111213";
        let mut service = ApplicationService::open(config(&root))
            .await
            .expect("open service");
        service
            .dispatch(add_request("add", torrent_id))
            .await
            .expect("add");
        let paused = service
            .dispatch(RequestEnvelope {
                version: CONTROL_VERSION,
                request_id: "pause".to_owned(),
                expected_revision: None,
                command: Command::Pause {
                    torrent_id: torrent_id.to_owned(),
                },
            })
            .await
            .expect("pause");
        let ResponseOutcome::Success { snapshot } = paused.outcome else {
            panic!("pause should succeed");
        };
        assert_eq!(snapshot.torrents[0].state, TorrentState::Paused);
        service.shutdown().await.expect("shutdown");
        drop(service);

        let mut reopened = ApplicationService::open(config(&root))
            .await
            .expect("reopen service");
        let snapshot = reopened
            .dispatch(RequestEnvelope {
                version: CONTROL_VERSION,
                request_id: "snapshot".to_owned(),
                expected_revision: None,
                command: Command::Snapshot,
            })
            .await
            .expect("snapshot");
        let ResponseOutcome::Success { snapshot } = snapshot.outcome else {
            panic!("snapshot should succeed");
        };
        assert_eq!(snapshot.torrents[0].state, TorrentState::Paused);
        reopened.shutdown().await.expect("shutdown reopened");
        drop(reopened);
        fs::remove_dir_all(root).expect("remove test root");
    }

    #[tokio::test]
    async fn pause_joins_content_peer_before_view_set_removal() {
        let root = test_root("pause-peer-cleanup");
        let configuration = config(&root);
        let payload = b"view";
        let mut raw_info =
            b"d5:filesld6:lengthi4e4:pathl4:testeee4:name4:root12:piece lengthi4e6:pieces20:"
                .to_vec();
        raw_info.extend_from_slice(&Sha1::digest(payload));
        raw_info.push(b'e');
        let info_hash: [u8; 20] = Sha1::digest(&raw_info).into();
        let torrent_id = super::encode_info_hash(info_hash);
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind paused content peer");
        let address = listener.local_addr().expect("content peer address");
        let peer_task = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.expect("accept content peer");
            let mut handshake = [0; HANDSHAKE_LENGTH];
            stream
                .read_exact(&mut handshake)
                .await
                .expect("read content handshake");
            decode_handshake(&handshake, info_hash).expect("content handshake identity");
            stream
                .write_all(&encode_handshake(info_hash, *b"-RS-APP-PAUSE-000000"))
                .await
                .expect("send content handshake");
            stream
                .write_all(&encode_message(&PeerMessage::Bitfield(vec![0x80])).expect("bitfield"))
                .await
                .expect("send content bitfield");
            let mut buffer = [0; 128];
            loop {
                if stream.read(&mut buffer).await.expect("wait for pause") == 0 {
                    break;
                }
            }
        });

        let mut store = SessionStore::open(
            &configuration.profile_root,
            &configuration.profile_id,
            &configuration.storage_roots,
        )
        .expect("open setup store");
        store
            .handle_durable(&RequestEnvelope {
                version: CONTROL_VERSION,
                request_id: "add-paused-content".to_owned(),
                expected_revision: None,
                command: Command::AddMagnet {
                    magnet: format!("magnet:?xt=urn:btih:{torrent_id}&x.pe={address}"),
                    storage_root: "downloads".to_owned(),
                    skip_files: Vec::new(),
                },
            })
            .expect("add content torrent");
        store
            .record_metadata(&torrent_id, &raw_info)
            .expect("record content metadata");
        drop(store);

        let mut service = ApplicationService::open(configuration)
            .await
            .expect("open content service");
        let owner = ViewSetOwner::trusted("pause-peer-owner");
        let opened = service
            .open_view_set(
                owner.clone(),
                OpenViewSetRequest {
                    views: vec![
                        ViewSpec::TorrentPeers {
                            view_id: "peers".to_owned(),
                            torrent_id: torrent_id.clone(),
                            delivery: ViewDeliveryPolicy::default(),
                        },
                        ViewSpec::TorrentSummary {
                            view_id: "summary".to_owned(),
                            torrent_id: torrent_id.clone(),
                            delivery: ViewDeliveryPolicy::default(),
                        },
                    ],
                    options: OpenViewSetOptions::default(),
                },
            )
            .expect("open peer view set");
        let view_set = service
            .view_set(&owner, &opened.view_set_id)
            .expect("peer view set handle");
        let mut cursor = opened.initial.cursor.clone();
        let mut active_peer = opened
            .initial
            .updates
            .iter()
            .find_map(|update| match update {
                ViewSetUpdate::Snapshot {
                    snapshot: ViewSnapshot::Peers { peers, .. },
                    ..
                } => peers
                    .iter()
                    .find(|peer| peer.lifecycle == crate::PeerLifecycle::Connected)
                    .map(|peer| peer.connection_id.clone()),
                _ => None,
            });
        if active_peer.is_none() {
            active_peer = tokio::time::timeout(std::time::Duration::from_secs(1), async {
                loop {
                    let batch = view_set
                        .next_updates(&cursor, 1_000)
                        .await
                        .expect("connected peer update");
                    cursor = batch.cursor.clone();
                    if let Some(connection_id) =
                        batch.updates.iter().find_map(|update| match update {
                            ViewSetUpdate::Snapshot {
                                snapshot: ViewSnapshot::Peers { peers, .. },
                                ..
                            } => peers
                                .iter()
                                .find(|peer| peer.lifecycle == crate::PeerLifecycle::Connected)
                                .map(|peer| peer.connection_id.clone()),
                            ViewSetUpdate::Patch {
                                patch: ViewPatch::Peers { upsert, .. },
                                ..
                            } => upsert
                                .iter()
                                .find(|peer| peer.lifecycle == crate::PeerLifecycle::Connected)
                                .map(|peer| peer.connection_id.clone()),
                            _ => None,
                        })
                    {
                        break Some(connection_id);
                    }
                }
            })
            .await
            .expect("content peer never became visible");
        }
        let active_peer = active_peer.expect("connected content peer row");

        let paused = service
            .dispatch(RequestEnvelope {
                version: CONTROL_VERSION,
                request_id: "pause-content".to_owned(),
                expected_revision: None,
                command: Command::Pause {
                    torrent_id: torrent_id.clone(),
                },
            })
            .await
            .expect("pause content torrent");
        let ResponseOutcome::Success { snapshot } = paused.outcome else {
            panic!("pause should succeed");
        };
        assert_eq!(snapshot.torrents[0].state, TorrentState::Paused);
        tokio::time::timeout(std::time::Duration::from_secs(1), peer_task)
            .await
            .expect("content peer did not close before pause receipt")
            .expect("content peer task");

        tokio::time::timeout(std::time::Duration::from_secs(1), async {
            let mut removed = false;
            let mut summary_is_terminal = false;
            while !removed || !summary_is_terminal {
                let batch = view_set
                    .next_updates(&cursor, 1_000)
                    .await
                    .expect("terminal peer updates");
                cursor = batch.cursor.clone();
                for update in batch.updates {
                    match update {
                        ViewSetUpdate::Patch {
                            patch: ViewPatch::Peers { removed: ids, .. },
                            ..
                        } => removed |= ids.contains(&active_peer),
                        ViewSetUpdate::Patch {
                            patch:
                                ViewPatch::Torrent {
                                    torrent: Some(torrent),
                                },
                            ..
                        }
                        | ViewSetUpdate::Snapshot {
                            snapshot:
                                ViewSnapshot::Torrent {
                                    torrent: Some(torrent),
                                },
                            ..
                        } => {
                            summary_is_terminal |= torrent.active_peer_connections == 0
                                && torrent.payload_download_rate_bytes == "0";
                        }
                        _ => {}
                    }
                }
            }
        })
        .await
        .expect("pause did not publish terminal peer views");

        service.shutdown().await.expect("shutdown");
        drop(service);
        fs::remove_dir_all(root).expect("remove test root");
    }

    #[tokio::test]
    async fn owner_cleanup_failure_is_not_accepted_as_joined_pause() {
        let root = test_root("pause-cleanup-failure");
        let configuration = config(&root);
        let torrent_id = "000102030405060708090a0b0c0d0e0f10111213";
        let mut store = SessionStore::open(
            &configuration.profile_root,
            &configuration.profile_id,
            &configuration.storage_roots,
        )
        .expect("open setup store");
        store
            .handle_durable(&add_request("add-cleanup-failure", torrent_id))
            .expect("add cleanup-failure torrent");
        store
            .handle_durable(&RequestEnvelope {
                version: CONTROL_VERSION,
                request_id: "pause-cleanup-failure".to_owned(),
                expected_revision: None,
                command: Command::Pause {
                    torrent_id: torrent_id.to_owned(),
                },
            })
            .expect("persist paused intent");
        drop(store);

        let mut service = ApplicationService::open(configuration)
            .await
            .expect("open paused service");
        let result = handle_task_outcome(
            &service.store,
            &service.storage_roots,
            &service.views,
            torrent_id,
            Err(DownloadError::PeerCleanup {
                failure: "download cancelled".to_owned(),
                cleanup: "one peer connection remains".to_owned(),
            }),
        );
        assert!(
            result
                .expect_err("cleanup failure must propagate")
                .contains("one peer connection remains")
        );

        service.shutdown().await.expect("shutdown");
        drop(service);
        fs::remove_dir_all(root).expect("remove test root");
    }

    #[tokio::test]
    async fn delete_managed_removal_joins_and_deletes_only_owned_path_artifacts() {
        let root = test_root("remove-path");
        let config = config(&root);
        let raw_info =
            b"d6:lengthi4e4:name4:test12:piece lengthi4e6:pieces20:aaaaaaaaaaaaaaaaaaaae";
        let info_hash: [u8; 20] = Sha1::digest(raw_info).into();
        let torrent_id = crate::control::encode_info_hash(info_hash);
        let mut store = SessionStore::open(
            &config.profile_root,
            &config.profile_id,
            &config.storage_roots,
        )
        .expect("open setup store");
        store
            .handle_durable(&add_request("add-remove-path", &torrent_id))
            .expect("add torrent");
        store
            .record_metadata(&torrent_id, raw_info)
            .expect("record metadata");
        drop(store);

        let payload = root.join("payload");
        let output = payload.join(&torrent_id);
        let staging = payload.join(format!(".{torrent_id}.rstorrent-staging"));
        let part = payload.join(format!(".{torrent_id}.rstorrent-parts"));
        let sibling = payload.join("keep-me");
        fs::create_dir_all(&output).expect("create output");
        fs::write(output.join("payload.bin"), b"payload").expect("write output");
        fs::create_dir_all(&staging).expect("create staging");
        fs::write(staging.join("partial.bin"), b"partial").expect("write staging");
        fs::write(&part, b"parts").expect("write part file");
        fs::write(&sibling, b"sibling").expect("write sibling");

        let mut service = ApplicationService::open(config)
            .await
            .expect("open service");
        let request = RequestEnvelope {
            version: CONTROL_VERSION,
            request_id: "remove-with-data".to_owned(),
            expected_revision: None,
            command: Command::RemoveTorrent {
                torrent_id: torrent_id.clone(),
                data: RemovalDataPolicy::DeleteManaged,
            },
        };
        service.dispatch(request.clone()).await.expect("remove");
        assert!(!output.exists());
        assert!(!staging.exists());
        assert!(!part.exists());
        assert_eq!(fs::read(&sibling).expect("preserved sibling"), b"sibling");
        assert!(
            service
                .store_mut()
                .expect("store")
                .snapshot()
                .expect("snapshot")
                .torrents
                .is_empty()
        );
        service.dispatch(request).await.expect("idempotent replay");

        service.shutdown().await.expect("shutdown");
        drop(service);
        fs::remove_dir_all(root).expect("remove test root");
    }

    #[tokio::test]
    async fn keep_data_removal_preserves_managed_path_artifacts() {
        let root = test_root("remove-keep");
        let torrent_id = "000102030405060708090a0b0c0d0e0f10111213";
        let payload = root.join("payload");
        let output = payload.join(torrent_id);
        let staging = payload.join(format!(".{torrent_id}.rstorrent-staging"));
        let part = payload.join(format!(".{torrent_id}.rstorrent-parts"));
        fs::create_dir_all(&output).expect("create output");
        fs::create_dir_all(&staging).expect("create staging");
        fs::write(output.join("payload.bin"), b"payload").expect("write output");
        fs::write(staging.join("partial.bin"), b"partial").expect("write staging");
        fs::write(&part, b"parts").expect("write part file");

        let mut service = ApplicationService::open(config(&root))
            .await
            .expect("open service");
        service
            .dispatch(add_request("add-remove-keep", torrent_id))
            .await
            .expect("add");
        service
            .dispatch(RequestEnvelope {
                version: CONTROL_VERSION,
                request_id: "remove-keep".to_owned(),
                expected_revision: None,
                command: Command::RemoveTorrent {
                    torrent_id: torrent_id.to_owned(),
                    data: RemovalDataPolicy::Keep,
                },
            })
            .await
            .expect("remove while keeping data");
        assert_eq!(
            fs::read(output.join("payload.bin")).expect("output"),
            b"payload"
        );
        assert_eq!(
            fs::read(staging.join("partial.bin")).expect("staging"),
            b"partial"
        );
        assert_eq!(fs::read(&part).expect("part"), b"parts");

        service.shutdown().await.expect("shutdown");
        drop(service);
        fs::remove_dir_all(root).expect("remove test root");
    }

    #[tokio::test]
    async fn failed_path_cleanup_stays_visible_and_retries_with_a_new_generation() {
        let root = test_root("remove-failure");
        let config = config(&root);
        let raw_info =
            b"d6:lengthi4e4:name4:test12:piece lengthi4e6:pieces20:aaaaaaaaaaaaaaaaaaaae";
        let info_hash: [u8; 20] = Sha1::digest(raw_info).into();
        let torrent_id = crate::control::encode_info_hash(info_hash);
        let mut store = SessionStore::open(
            &config.profile_root,
            &config.profile_id,
            &config.storage_roots,
        )
        .expect("open setup store");
        store
            .handle_durable(&add_request("add-remove-failure", &torrent_id))
            .expect("add torrent");
        store
            .record_metadata(&torrent_id, raw_info)
            .expect("record metadata");
        drop(store);
        let part = root
            .join("payload")
            .join(format!(".{torrent_id}.rstorrent-parts"));
        fs::create_dir_all(&part).expect("create invalid part directory");

        let mut service = ApplicationService::open(config)
            .await
            .expect("open service");
        service
            .dispatch(RequestEnvelope {
                version: CONTROL_VERSION,
                request_id: "remove-fails".to_owned(),
                expected_revision: None,
                command: Command::RemoveTorrent {
                    torrent_id: torrent_id.clone(),
                    data: RemovalDataPolicy::DeleteManaged,
                },
            })
            .await
            .expect("accept failed cleanup intent");
        let failed = service
            .store_mut()
            .expect("store")
            .snapshot()
            .expect("failed snapshot")
            .torrents
            .remove(0);
        assert_eq!(failed.removal_state, Some(RemovalState::Failed));
        assert!(failed.error.is_some());

        fs::remove_dir(&part).expect("repair invalid part path");
        fs::write(&part, b"part").expect("create valid part file");
        service
            .dispatch(RequestEnvelope {
                version: CONTROL_VERSION,
                request_id: "remove-retry".to_owned(),
                expected_revision: None,
                command: Command::RemoveTorrent {
                    torrent_id: torrent_id.clone(),
                    data: RemovalDataPolicy::DeleteManaged,
                },
            })
            .await
            .expect("retry cleanup");
        assert!(!part.exists());
        assert!(
            service
                .store_mut()
                .expect("store")
                .snapshot()
                .expect("snapshot")
                .torrents
                .is_empty()
        );

        service.shutdown().await.expect("shutdown");
        drop(service);
        fs::remove_dir_all(root).expect("remove test root");
    }

    #[tokio::test]
    async fn startup_resumes_a_durable_pending_path_removal() {
        let root = test_root("remove-restart");
        let config = config(&root);
        let raw_info =
            b"d6:lengthi4e4:name4:test12:piece lengthi4e6:pieces20:aaaaaaaaaaaaaaaaaaaae";
        let info_hash: [u8; 20] = Sha1::digest(raw_info).into();
        let torrent_id = crate::control::encode_info_hash(info_hash);
        let mut store = SessionStore::open(
            &config.profile_root,
            &config.profile_id,
            &config.storage_roots,
        )
        .expect("open setup store");
        store
            .handle_durable(&add_request("add-remove-restart", &torrent_id))
            .expect("add torrent");
        store
            .record_metadata(&torrent_id, raw_info)
            .expect("record metadata");
        store
            .handle_durable(&RequestEnvelope {
                version: CONTROL_VERSION,
                request_id: "remove-before-restart".to_owned(),
                expected_revision: None,
                command: Command::RemoveTorrent {
                    torrent_id: torrent_id.clone(),
                    data: RemovalDataPolicy::DeleteManaged,
                },
            })
            .expect("persist pending removal");
        drop(store);
        let output = root.join("payload").join(&torrent_id);
        fs::create_dir_all(&output).expect("create interrupted output");
        fs::write(output.join("payload.bin"), b"payload").expect("write output");

        let mut service = ApplicationService::open(config)
            .await
            .expect("resume application");
        assert!(!output.exists());
        assert!(
            service
                .store_mut()
                .expect("store")
                .snapshot()
                .expect("snapshot")
                .torrents
                .is_empty()
        );
        service.shutdown().await.expect("shutdown");
        drop(service);
        fs::remove_dir_all(root).expect("remove test root");
    }

    #[tokio::test]
    async fn platform_removal_requires_matching_generation_confirmation() {
        let root = test_root("remove-platform");
        let mut config = config(&root);
        config.storage_roots = vec![ConfiguredStorageRoot::platform("downloads")];
        let raw_info =
            b"d6:lengthi4e4:name4:test12:piece lengthi4e6:pieces20:aaaaaaaaaaaaaaaaaaaae";
        let info_hash: [u8; 20] = Sha1::digest(raw_info).into();
        let torrent_id = crate::control::encode_info_hash(info_hash);
        let mut store = SessionStore::open(
            &config.profile_root,
            &config.profile_id,
            &config.storage_roots,
        )
        .expect("open setup store");
        store
            .handle_durable(&add_request("add-remove-platform", &torrent_id))
            .expect("add torrent");
        store
            .record_metadata(&torrent_id, raw_info)
            .expect("record metadata");
        drop(store);

        let mut service = ApplicationService::open(config.clone())
            .await
            .expect("open service");
        service
            .dispatch(RequestEnvelope {
                version: CONTROL_VERSION,
                request_id: "remove-platform".to_owned(),
                expected_revision: None,
                command: Command::RemoveTorrent {
                    torrent_id: torrent_id.clone(),
                    data: RemovalDataPolicy::DeleteManaged,
                },
            })
            .await
            .expect("request platform removal");
        let initial_plan = service
            .platform_removal_plan(&torrent_id)
            .await
            .expect("platform plan");
        assert_eq!(initial_plan.name, "test");
        assert_eq!(initial_plan.storage_root, "downloads");
        assert!(
            service
                .confirm_platform_removal(&torrent_id, "stale-operation")
                .await
                .is_err()
        );
        assert_eq!(
            service
                .store_mut()
                .expect("store")
                .snapshot()
                .expect("snapshot")
                .torrents[0]
                .removal_state,
            Some(RemovalState::AwaitingPlatform)
        );
        service
            .fail_platform_removal(
                &torrent_id,
                &initial_plan.operation_id,
                "provider permission was revoked",
            )
            .await
            .expect("record platform failure");
        let failed = service
            .store_mut()
            .expect("store")
            .snapshot()
            .expect("failed snapshot")
            .torrents
            .remove(0);
        assert_eq!(failed.removal_state, Some(RemovalState::Failed));
        assert_eq!(
            failed.error.as_deref(),
            Some("provider permission was revoked")
        );
        service
            .dispatch(RequestEnvelope {
                version: CONTROL_VERSION,
                request_id: "retry-platform-removal".to_owned(),
                expected_revision: None,
                command: Command::RemoveTorrent {
                    torrent_id: torrent_id.clone(),
                    data: RemovalDataPolicy::DeleteManaged,
                },
            })
            .await
            .expect("retry platform removal");
        let plan = service
            .platform_removal_plan(&torrent_id)
            .await
            .expect("retry platform plan");
        assert_ne!(plan.operation_id, initial_plan.operation_id);
        service
            .shutdown()
            .await
            .expect("shutdown before confirmation");
        drop(service);

        let mut service = ApplicationService::open(config)
            .await
            .expect("reopen awaiting platform removal");
        let resumed_plan = service
            .platform_removal_plan(&torrent_id)
            .await
            .expect("resume platform plan");
        assert_eq!(resumed_plan, plan);
        service
            .confirm_platform_removal(&torrent_id, &plan.operation_id)
            .await
            .expect("confirm current operation");
        assert!(
            service
                .store_mut()
                .expect("store")
                .snapshot()
                .expect("snapshot")
                .torrents
                .is_empty()
        );

        let metadata_pending = "000102030405060708090a0b0c0d0e0f10111213";
        service
            .dispatch(add_request(
                "add-platform-without-metadata",
                metadata_pending,
            ))
            .await
            .expect("add metadata-pending platform torrent");
        service
            .dispatch(RequestEnvelope {
                version: CONTROL_VERSION,
                request_id: "remove-platform-without-metadata".to_owned(),
                expected_revision: None,
                command: Command::RemoveTorrent {
                    torrent_id: metadata_pending.to_owned(),
                    data: RemovalDataPolicy::DeleteManaged,
                },
            })
            .await
            .expect("remove metadata-pending torrent");
        assert!(
            service
                .store_mut()
                .expect("store")
                .snapshot()
                .expect("snapshot")
                .torrents
                .is_empty()
        );

        service.shutdown().await.expect("shutdown");
        drop(service);
        fs::remove_dir_all(root).expect("remove test root");
    }

    #[tokio::test]
    async fn tracker_retry_waiting_is_observed_without_another_command() {
        let root = test_root("tracker-retry-waiting");
        let torrent_id = "000102030405060708090a0b0c0d0e0f10111213";
        let mut service = ApplicationService::open(config(&root))
            .await
            .expect("open service");
        let summary = service
            .subscribe(SubscriptionSpec {
                selector: ViewSelector::TorrentList,
                projection: ViewProjection::Summary,
                delivery: DeliveryPolicy {
                    min_interval_millis: 0,
                    max_queue_bytes: 256 * 1024,
                },
                diagnostics: None,
            })
            .expect("summary");
        summary.next_update().await.expect("summary snapshot");
        let diagnostics = service
            .subscribe(SubscriptionSpec {
                selector: ViewSelector::TorrentList,
                projection: ViewProjection::Diagnostics,
                delivery: DeliveryPolicy {
                    min_interval_millis: 0,
                    max_queue_bytes: 256 * 1024,
                },
                diagnostics: Some(DiagnosticFilter {
                    profile: DiagnosticProfile::Normal,
                    minimum_severity: DiagnosticSeverity::Info,
                    categories: Vec::new(),
                }),
            })
            .expect("diagnostics");
        diagnostics
            .next_update()
            .await
            .expect("diagnostic snapshot");

        service
            .dispatch(RequestEnvelope {
                version: CONTROL_VERSION,
                request_id: "blocked-add".to_owned(),
                expected_revision: None,
                command: Command::AddMagnet {
                    magnet: format!(
                        "magnet:?xt=urn:btih:{torrent_id}&tr=udp%3A%2F%2F192.0.2.1%3A6969%2Fannounce"
                    ),
                    storage_root: "downloads".to_owned(),
                    skip_files: Vec::new(),
                },
            })
            .await
            .expect("add");

        let waiting = tokio::time::timeout(std::time::Duration::from_secs(2), async {
            loop {
                let update = summary.next_update().await.expect("summary update");
                let is_waiting = match update.payload {
                    ViewUpdatePayload::Snapshot {
                        snapshot: ViewSnapshot::TorrentList { torrents, .. },
                    } => torrents.first().is_some_and(|torrent| {
                        torrent.progress.disposition == ProgressDisposition::Waiting
                            && torrent.progress.reason == ProgressReason::WaitingForDiscovery
                    }),
                    ViewUpdatePayload::Patch {
                        patch: ViewPatch::TorrentList { upsert, .. },
                    } => upsert.first().is_some_and(|torrent| {
                        torrent.progress.disposition == ProgressDisposition::Waiting
                            && torrent.progress.reason == ProgressReason::WaitingForDiscovery
                    }),
                    _ => false,
                };
                if is_waiting {
                    break;
                }
            }
        })
        .await;
        assert!(
            waiting.is_ok(),
            "tracker owner did not publish scheduled discovery waiting"
        );

        let diagnostic = tokio::time::timeout(std::time::Duration::from_secs(1), async {
            loop {
                let update = diagnostics.next_update().await.expect("diagnostic update");
                if serde_json::to_string(&update)
                    .expect("serialize diagnostic")
                    .contains("tracker_retry_scheduled")
                {
                    break;
                }
            }
        })
        .await;
        assert!(
            diagnostic.is_ok(),
            "missing scheduled tracker retry diagnostic"
        );
        let resume = service
            .load_resume_conservative(torrent_id)
            .expect("resume state");
        assert!(resume.desired_running);
        assert_eq!(resume.state, TorrentState::AwaitingMetadata);

        service.shutdown().await.expect("shutdown");
        drop(service);
        fs::remove_dir_all(root).expect("remove test root");
    }

    #[tokio::test]
    async fn offline_policy_reports_network_blockage_without_torrent_error() {
        let root = test_root("network-offline");
        let torrent_id = "000102030405060708090a0b0c0d0e0f10111213";
        let mut application_config = config(&root);
        application_config.network.policy = NetworkPolicy::Offline;
        let mut service = ApplicationService::open(application_config)
            .await
            .expect("open offline service");
        let summary = service
            .subscribe(SubscriptionSpec {
                selector: ViewSelector::TorrentList,
                projection: ViewProjection::Summary,
                delivery: DeliveryPolicy {
                    min_interval_millis: 0,
                    max_queue_bytes: 256 * 1024,
                },
                diagnostics: None,
            })
            .expect("summary");
        summary.next_update().await.expect("initial summary");

        service
            .dispatch(add_request("offline-add", torrent_id))
            .await
            .expect("add while offline");

        let torrent = tokio::time::timeout(std::time::Duration::from_secs(1), async {
            loop {
                let update = summary.next_update().await.expect("summary update");
                let candidate = match update.payload {
                    ViewUpdatePayload::Snapshot {
                        snapshot: ViewSnapshot::TorrentList { mut torrents, .. },
                    } => torrents.pop(),
                    ViewUpdatePayload::Patch {
                        patch: ViewPatch::TorrentList { mut upsert, .. },
                    } => upsert.pop(),
                    _ => None,
                };
                if let Some(torrent) = candidate
                    && torrent.progress.reason == ProgressReason::NetworkDisabled
                {
                    break torrent;
                }
            }
        })
        .await
        .expect("network-disabled progress");

        assert_eq!(torrent.state, TorrentState::AwaitingMetadata);
        assert!(torrent.error.is_none());
        service.shutdown().await.expect("shutdown");
        drop(service);
        fs::remove_dir_all(root).expect("remove offline test root");
    }

    #[tokio::test]
    async fn startup_projects_verified_metadata_name() {
        let root = test_root("metadata-name");
        let configuration = config(&root);
        let raw_info =
            b"d6:lengthi4e4:name5:named12:piece lengthi4e6:pieces20:aaaaaaaaaaaaaaaaaaaae";
        let info_hash: [u8; 20] = Sha1::digest(raw_info).into();
        let torrent_id = super::encode_info_hash(info_hash);
        let configured_root = configuration.storage_roots[0].clone();
        let mut store = SessionStore::open(
            &configuration.profile_root,
            &configuration.profile_id,
            &[configured_root],
        )
        .expect("open store");
        store
            .handle_durable(&add_request("add", &torrent_id))
            .expect("add");
        store
            .record_metadata(&torrent_id, raw_info)
            .expect("record metadata");
        drop(store);

        let mut service = ApplicationService::open(configuration)
            .await
            .expect("open service");
        let summary = service
            .subscribe(SubscriptionSpec {
                selector: ViewSelector::TorrentList,
                projection: ViewProjection::Summary,
                delivery: DeliveryPolicy {
                    min_interval_millis: 0,
                    max_queue_bytes: 256 * 1024,
                },
                diagnostics: None,
            })
            .expect("summary");
        let update = summary.next_update().await.expect("summary snapshot");
        let ViewUpdatePayload::Snapshot {
            snapshot: ViewSnapshot::TorrentList { torrents, .. },
        } = update.payload
        else {
            panic!("expected torrent-list snapshot");
        };
        assert_eq!(torrents[0].display_name.as_deref(), Some("named"));

        service.shutdown().await.expect("shutdown");
        drop(service);
        fs::remove_dir_all(root).expect("remove metadata-name test root");
    }

    #[tokio::test]
    async fn corrupt_metadata_enters_repair_before_storage() {
        let root = test_root("metadata-corruption");
        let configuration = config(&root);
        let raw_info =
            b"d6:lengthi4e4:name4:test12:piece lengthi4e6:pieces20:aaaaaaaaaaaaaaaaaaaae";
        let info_hash: [u8; 20] = Sha1::digest(raw_info).into();
        let torrent_id = super::encode_info_hash(info_hash);
        let configured_root = configuration.storage_roots[0].clone();
        let mut store = SessionStore::open(
            &configuration.profile_root,
            &configuration.profile_id,
            &[configured_root],
        )
        .expect("open store");
        store
            .handle_durable(&add_request("add", &torrent_id))
            .expect("add");
        store
            .record_metadata(&torrent_id, raw_info)
            .expect("record metadata");
        let database = store.database_path().to_owned();
        drop(store);

        let connection = Connection::open(database).expect("open raw database");
        connection
            .execute(
                "UPDATE torrents SET raw_info = x'00' WHERE info_hash = ?1",
                [info_hash.as_slice()],
            )
            .expect("corrupt raw metadata");
        drop(connection);

        let mut service = ApplicationService::open(configuration)
            .await
            .expect("open service with corrupt metadata");
        let response = service
            .dispatch(RequestEnvelope {
                version: CONTROL_VERSION,
                request_id: "snapshot".to_owned(),
                expected_revision: None,
                command: Command::Snapshot,
            })
            .await
            .expect("snapshot");
        let ResponseOutcome::Success { snapshot } = response.outcome else {
            panic!("snapshot should succeed");
        };
        assert_eq!(snapshot.torrents[0].state, TorrentState::NeedsRepair);
        assert_eq!(
            fs::read_dir(root.join("payload"))
                .expect("read empty payload root")
                .count(),
            0
        );
        service.shutdown().await.expect("shutdown");
        drop(service);
        fs::remove_dir_all(root).expect("remove test root");
    }

    #[tokio::test]
    async fn malformed_have_state_is_cleared_conservatively() {
        let root = test_root("have-corruption");
        let configuration = config(&root);
        let raw_info =
            b"d6:lengthi4e4:name4:test12:piece lengthi4e6:pieces20:aaaaaaaaaaaaaaaaaaaae";
        let info_hash: [u8; 20] = Sha1::digest(raw_info).into();
        let torrent_id = super::encode_info_hash(info_hash);
        let configured_root = configuration.storage_roots[0].clone();
        let mut store = SessionStore::open(
            &configuration.profile_root,
            &configuration.profile_id,
            &[configured_root],
        )
        .expect("open store");
        store
            .handle_durable(&add_request("add", &torrent_id))
            .expect("add");
        store
            .record_metadata(&torrent_id, raw_info)
            .expect("record metadata");
        let database = store.database_path().to_owned();
        drop(store);

        let connection = Connection::open(database).expect("open raw database");
        let mut have: Vec<u8> = connection
            .query_row(
                "SELECT have_state FROM torrents WHERE info_hash = ?1",
                [info_hash.as_slice()],
                |row| row.get(0),
            )
            .expect("read have state");
        *have.last_mut().expect("bitfield byte") = 1;
        connection
            .execute(
                "UPDATE torrents SET have_state = ?2 WHERE info_hash = ?1",
                rusqlite::params![info_hash.as_slice(), have],
            )
            .expect("corrupt have padding");
        drop(connection);

        let mut service = ApplicationService::open(configuration)
            .await
            .expect("open service with malformed have state");
        let response = service
            .dispatch(RequestEnvelope {
                version: CONTROL_VERSION,
                request_id: "snapshot".to_owned(),
                expected_revision: None,
                command: Command::Snapshot,
            })
            .await
            .expect("snapshot");
        let ResponseOutcome::Success { snapshot } = response.outcome else {
            panic!("snapshot should succeed");
        };
        assert_eq!(snapshot.torrents[0].verified_piece_count, 0);
        assert_ne!(snapshot.torrents[0].state, TorrentState::NeedsRepair);
        service.shutdown().await.expect("shutdown");
        drop(service);
        fs::remove_dir_all(root).expect("remove test root");
    }

    #[tokio::test]
    async fn incomplete_storage_artifacts_enter_repair_without_overwrite() {
        let root = test_root("storage-repair");
        let configuration = config(&root);
        let raw_info = b"d5:filesld6:lengthi4e4:pathl4:testeee4:name4:root12:piece lengthi4e6:pieces20:aaaaaaaaaaaaaaaaaaaae";
        let info_hash: [u8; 20] = Sha1::digest(raw_info).into();
        let torrent_id = super::encode_info_hash(info_hash);
        let configured_root = configuration.storage_roots[0].clone();
        let mut store = SessionStore::open(
            &configuration.profile_root,
            &configuration.profile_id,
            std::slice::from_ref(&configured_root),
        )
        .expect("open store");
        store
            .handle_durable(&add_request("add", &torrent_id))
            .expect("add");
        store
            .record_metadata(&torrent_id, raw_info)
            .expect("record metadata");
        drop(store);

        let crate::StorageRootLocation::Path(payload_root) = &configured_root.location else {
            unreachable!("test root is path-backed")
        };
        let incomplete_output = payload_root.join(&torrent_id);
        fs::create_dir_all(&incomplete_output).expect("create incomplete output");
        fs::write(incomplete_output.join("preserve"), b"user artifact")
            .expect("write preserved artifact");

        let mut service = ApplicationService::open(configuration)
            .await
            .expect("open service with incomplete storage");
        let mut state = None;
        for sequence in 0..100 {
            tokio::task::yield_now().await;
            let response = service
                .dispatch(RequestEnvelope {
                    version: CONTROL_VERSION,
                    request_id: format!("snapshot-{sequence}"),
                    expected_revision: None,
                    command: Command::Snapshot,
                })
                .await
                .expect("snapshot");
            let ResponseOutcome::Success { snapshot } = response.outcome else {
                panic!("snapshot should succeed");
            };
            state = Some(snapshot.torrents[0].state);
            if state == Some(TorrentState::NeedsRepair) {
                break;
            }
        }
        assert_eq!(state, Some(TorrentState::NeedsRepair));
        assert_eq!(
            fs::read(incomplete_output.join("preserve"))
                .expect("read preserved incomplete artifact"),
            b"user artifact"
        );
        service.shutdown().await.expect("shutdown");
        drop(service);
        fs::remove_dir_all(root).expect("remove test root");
    }
}
