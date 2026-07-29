//! Coarse Android control plane for an in-process RSTorrent engine.

use std::net::{Ipv4Addr, SocketAddr};
use std::path::PathBuf;
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use rstorrent_engine::{
    DownloadConfig, DownloadControl, DownloadError, DownloadProgress, DownloadReport,
    download_verified_piece_with_control,
};

const INTERFACE_VERSION: &str = "rstorrent-android/0.1.0;uniffi/0.31.0";
const MIN_PAYLOAD_BYTES: u64 = 16 * 1024;
const MAX_PAYLOAD_BYTES: u64 = 4 * 1024 * 1024;
const MAX_TIMEOUT_SECONDS: u64 = 5 * 60;
const MAX_JOIN_MILLIS: u64 = 5 * 60 * 1_000;
const MAX_FILE_SELECTIONS: usize = 1_024;
const MAX_STORAGE_WRITE_DELAY_MILLIS: u64 = 5_000;

uniffi::setup_scaffolding!();

#[derive(Clone, Debug, uniffi::Record)]
pub struct EngineConfig {
    pub metainfo_path: String,
    pub output_path: String,
    pub peer_port: u16,
    pub timeout_seconds: u64,
    pub max_buffered_payload_bytes: u64,
    pub storage_write_delay_millis: u64,
    pub skip_files: Vec<u32>,
    pub materialize_files: Vec<u32>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, uniffi::Enum)]
pub enum StartDisposition {
    Started,
    Busy,
    NeedsJoin,
    Rejected,
}

#[derive(Clone, Debug, uniffi::Record)]
pub struct StartResult {
    pub disposition: StartDisposition,
    pub generation: u64,
    pub message: Option<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, uniffi::Enum)]
pub enum SessionState {
    Idle,
    Running,
    Cancelling,
    Succeeded,
    Failed,
    Cancelled,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, uniffi::Enum)]
pub enum TerminalOutcome {
    Succeeded,
    Failed,
    Cancelled,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, uniffi::Enum)]
pub enum FailureKind {
    Peer,
    Timeout,
    Configuration,
    Protocol,
    PreexistingArtifact,
    Storage,
    Cleanup,
    Runtime,
}

#[derive(Clone, Debug, uniffi::Record)]
pub struct EngineReport {
    pub info_hash_hex: String,
    pub final_piece_hash_hex: String,
    pub bytes_written: u64,
    pub block_count: u64,
    pub payload_limit: u64,
    pub payload_high_water: u64,
    pub verification_buffer: u64,
    pub piece_count: u64,
    pub verified_piece_count: u64,
    pub skipped_piece_count: u64,
    pub selected_file_bytes: u64,
    pub skipped_file_bytes: u64,
    pub padding_bytes: u64,
    pub selected_written_bytes: u64,
    pub part_written_bytes: u64,
    pub materialized_bytes: u64,
    pub part_slots_before_materialization: u64,
    pub part_slots_after_materialization: u64,
    pub part_reopened: bool,
    pub part_path: Option<String>,
}

#[derive(Clone, Debug, uniffi::Record)]
pub struct TerminalResult {
    pub outcome: TerminalOutcome,
    pub failure_kind: Option<FailureKind>,
    pub failure_message: Option<String>,
    pub report: Option<EngineReport>,
    pub elapsed_millis: u64,
}

#[derive(Clone, Debug, uniffi::Record)]
pub struct SessionSnapshot {
    pub interface_version: String,
    pub state: SessionState,
    pub generation: u64,
    pub task_alive: bool,
    pub cancellation_requested: bool,
    pub buffered_payload_bytes: u64,
    pub payload_high_water: u64,
    pub requested_bytes: u64,
    pub received_bytes: u64,
    pub stored_bytes: u64,
    pub terminal: Option<TerminalResult>,
}

#[derive(Clone, Debug, uniffi::Record)]
pub struct JoinResult {
    pub joined: bool,
    pub terminal: Option<TerminalResult>,
}

#[derive(Debug, uniffi::Object)]
pub struct EngineSession {
    shared: Arc<Shared>,
}

#[derive(Debug)]
struct Shared {
    inner: Mutex<Inner>,
    changed: Condvar,
}

#[derive(Debug)]
struct Inner {
    state: SessionState,
    generation: u64,
    started: Option<Instant>,
    control: Option<DownloadControl>,
    progress: DownloadProgress,
    cancellation_requested: bool,
    worker: Option<JoinHandle<()>>,
    terminal: Option<TerminalResult>,
}

impl EngineSession {
    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.shared
            .inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn join_until(&self, timeout: Duration) -> JoinResult {
        let deadline = Instant::now() + timeout;
        let mut inner = self.lock();
        loop {
            if inner.terminal.is_some() {
                let terminal = inner.terminal.clone();
                let worker = inner.worker.take();
                drop(inner);
                if let Some(worker) = worker
                    && worker.join().is_err()
                {
                    let failure = runtime_failure("engine worker panicked");
                    let mut inner = self.lock();
                    inner.state = SessionState::Failed;
                    inner.terminal = Some(failure.clone());
                    return JoinResult {
                        joined: true,
                        terminal: Some(failure),
                    };
                }
                return JoinResult {
                    joined: true,
                    terminal,
                };
            }
            if inner.worker.is_none() {
                return JoinResult {
                    joined: true,
                    terminal: None,
                };
            }

            let now = Instant::now();
            if now >= deadline {
                return JoinResult {
                    joined: false,
                    terminal: None,
                };
            }
            let remaining = deadline.saturating_duration_since(now);
            let (next, wait) = self
                .shared
                .changed
                .wait_timeout(inner, remaining)
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            inner = next;
            if wait.timed_out() && inner.terminal.is_none() {
                return JoinResult {
                    joined: false,
                    terminal: None,
                };
            }
        }
    }
}

#[uniffi::export]
impl EngineSession {
    #[uniffi::constructor]
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            shared: Arc::new(Shared {
                inner: Mutex::new(Inner {
                    state: SessionState::Idle,
                    generation: 0,
                    started: None,
                    control: None,
                    progress: DownloadProgress::default(),
                    cancellation_requested: false,
                    worker: None,
                    terminal: None,
                }),
                changed: Condvar::new(),
            }),
        })
    }

    pub fn start(&self, config: EngineConfig) -> StartResult {
        let (config, storage_write_delay) = match validate_config(config) {
            Ok(config) => config,
            Err(message) => {
                let inner = self.lock();
                return StartResult {
                    disposition: StartDisposition::Rejected,
                    generation: inner.generation,
                    message: Some(message),
                };
            }
        };

        let mut inner = self.lock();
        if matches!(
            inner.state,
            SessionState::Running | SessionState::Cancelling
        ) {
            return StartResult {
                disposition: StartDisposition::Busy,
                generation: inner.generation,
                message: Some("an engine task is already active".to_owned()),
            };
        }
        if inner.worker.is_some() {
            return StartResult {
                disposition: StartDisposition::NeedsJoin,
                generation: inner.generation,
                message: Some("the previous engine task must be joined".to_owned()),
            };
        }

        inner.generation += 1;
        let generation = inner.generation;
        let started = Instant::now();
        let control = DownloadControl::new();
        control.set_storage_write_delay(storage_write_delay);
        inner.state = SessionState::Running;
        inner.started = Some(started);
        inner.control = Some(control.clone());
        inner.progress = DownloadProgress::default();
        inner.cancellation_requested = false;
        inner.terminal = None;

        let shared = Arc::clone(&self.shared);
        let worker = std::thread::Builder::new()
            .name(format!("rstorrent-engine-{generation}"))
            .spawn(move || run_worker(shared, config, control, started));
        match worker {
            Ok(worker) => {
                inner.worker = Some(worker);
                StartResult {
                    disposition: StartDisposition::Started,
                    generation,
                    message: None,
                }
            }
            Err(error) => {
                inner.state = SessionState::Idle;
                inner.started = None;
                inner.control = None;
                StartResult {
                    disposition: StartDisposition::Rejected,
                    generation,
                    message: Some(format!("failed to start engine worker: {error}")),
                }
            }
        }
    }

    pub fn snapshot(&self) -> SessionSnapshot {
        let inner = self.lock();
        let progress = inner
            .control
            .as_ref()
            .map(DownloadControl::snapshot)
            .unwrap_or(inner.progress);
        SessionSnapshot {
            interface_version: INTERFACE_VERSION.to_owned(),
            state: inner.state,
            generation: inner.generation,
            task_alive: inner
                .worker
                .as_ref()
                .is_some_and(|worker| !worker.is_finished()),
            cancellation_requested: inner.cancellation_requested,
            buffered_payload_bytes: progress.buffered_payload_bytes as u64,
            payload_high_water: progress.payload_high_water as u64,
            requested_bytes: progress.requested_bytes as u64,
            received_bytes: progress.received_bytes as u64,
            stored_bytes: progress.stored_bytes as u64,
            terminal: inner.terminal.clone(),
        }
    }

    pub fn wait_for_terminal(&self, timeout_millis: u64) -> JoinResult {
        self.join_until(join_timeout(timeout_millis))
    }

    pub fn cancel_and_join(&self, timeout_millis: u64) -> JoinResult {
        let control = {
            let mut inner = self.lock();
            if inner.state == SessionState::Running {
                inner.state = SessionState::Cancelling;
            }
            if inner.control.is_some() {
                inner.cancellation_requested = true;
            }
            inner.control.clone()
        };
        if let Some(control) = control {
            control.cancel();
        }
        self.join_until(join_timeout(timeout_millis))
    }
}

impl Drop for EngineSession {
    fn drop(&mut self) {
        if let Some(control) = self.lock().control.clone() {
            control.cancel();
        }
    }
}

#[uniffi::export]
pub fn interface_version() -> String {
    INTERFACE_VERSION.to_owned()
}

fn validate_config(config: EngineConfig) -> Result<(DownloadConfig, Duration), String> {
    if config.metainfo_path.is_empty() || config.output_path.is_empty() {
        return Err("metainfo and output paths must be nonempty".to_owned());
    }
    if config.peer_port == 0 {
        return Err("peer port must be nonzero".to_owned());
    }
    if !(1..=MAX_TIMEOUT_SECONDS).contains(&config.timeout_seconds) {
        return Err(format!(
            "timeout must be between 1 and {MAX_TIMEOUT_SECONDS} seconds"
        ));
    }
    if !(MIN_PAYLOAD_BYTES..=MAX_PAYLOAD_BYTES).contains(&config.max_buffered_payload_bytes) {
        return Err(format!(
            "payload allowance must be between {MIN_PAYLOAD_BYTES} and \
             {MAX_PAYLOAD_BYTES} bytes"
        ));
    }
    if config.skip_files.len() > MAX_FILE_SELECTIONS
        || config.materialize_files.len() > MAX_FILE_SELECTIONS
    {
        return Err(format!(
            "file selection lists may contain at most {MAX_FILE_SELECTIONS} entries"
        ));
    }
    if config.storage_write_delay_millis > MAX_STORAGE_WRITE_DELAY_MILLIS {
        return Err(format!(
            "storage write delay may be at most \
             {MAX_STORAGE_WRITE_DELAY_MILLIS} milliseconds"
        ));
    }

    Ok((
        DownloadConfig {
            metainfo_path: PathBuf::from(config.metainfo_path),
            peer: SocketAddr::from((Ipv4Addr::LOCALHOST, config.peer_port)),
            output_path: PathBuf::from(config.output_path),
            timeout: Duration::from_secs(config.timeout_seconds),
            max_buffered_payload_bytes: config.max_buffered_payload_bytes as usize,
            skip_files: config
                .skip_files
                .into_iter()
                .map(|index| index as usize)
                .collect(),
            materialize_files: config
                .materialize_files
                .into_iter()
                .map(|index| index as usize)
                .collect(),
        },
        Duration::from_millis(config.storage_write_delay_millis),
    ))
}

fn run_worker(
    shared: Arc<Shared>,
    config: DownloadConfig,
    control: DownloadControl,
    started: Instant,
) {
    let result = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|error| WorkerFailure::Runtime(format!("failed to build Tokio runtime: {error}")))
        .and_then(|runtime| {
            runtime
                .block_on(download_verified_piece_with_control(
                    config,
                    control.clone(),
                ))
                .map_err(WorkerFailure::Engine)
        });

    let terminal = match result {
        Ok(report) => success_result(report, started.elapsed()),
        Err(WorkerFailure::Engine(DownloadError::Cancelled)) => TerminalResult {
            outcome: TerminalOutcome::Cancelled,
            failure_kind: None,
            failure_message: None,
            report: None,
            elapsed_millis: millis(started.elapsed()),
        },
        Err(WorkerFailure::Engine(error)) => TerminalResult {
            outcome: TerminalOutcome::Failed,
            failure_kind: Some(classify_failure(&error)),
            failure_message: Some(error.to_string()),
            report: None,
            elapsed_millis: millis(started.elapsed()),
        },
        Err(WorkerFailure::Runtime(message)) => TerminalResult {
            outcome: TerminalOutcome::Failed,
            failure_kind: Some(FailureKind::Runtime),
            failure_message: Some(message),
            report: None,
            elapsed_millis: millis(started.elapsed()),
        },
    };
    let mut inner = shared
        .inner
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    inner.state = match terminal.outcome {
        TerminalOutcome::Succeeded => SessionState::Succeeded,
        TerminalOutcome::Failed => SessionState::Failed,
        TerminalOutcome::Cancelled => SessionState::Cancelled,
    };
    inner.progress = control.snapshot();
    inner.control = None;
    inner.terminal = Some(terminal);
    drop(inner);
    shared.changed.notify_all();
}

#[derive(Debug)]
enum WorkerFailure {
    Engine(DownloadError),
    Runtime(String),
}

fn classify_failure(error: &DownloadError) -> FailureKind {
    if error.is_existing_artifact() {
        return FailureKind::PreexistingArtifact;
    }
    match error {
        DownloadError::PeerClosed | DownloadError::Handshake(_) | DownloadError::Frame(_) => {
            FailureKind::Peer
        }
        DownloadError::Io { operation, .. } if operation.contains("peer") => FailureKind::Peer,
        DownloadError::TimedOut { .. } => FailureKind::Timeout,
        DownloadError::NonLoopbackPeer(_)
        | DownloadError::InvalidTimeout
        | DownloadError::MetainfoTooLarge { .. }
        | DownloadError::Metainfo(_)
        | DownloadError::Layout(_) => FailureKind::Configuration,
        DownloadError::Piece(_) => FailureKind::Protocol,
        DownloadError::Storage(_)
        | DownloadError::SelectiveStorage(_)
        | DownloadError::Io { .. } => FailureKind::Storage,
        DownloadError::CleanupAfterFailure { .. } => FailureKind::Cleanup,
        DownloadError::Cancelled => FailureKind::Runtime,
    }
}

fn success_result(report: DownloadReport, elapsed: Duration) -> TerminalResult {
    TerminalResult {
        outcome: TerminalOutcome::Succeeded,
        failure_kind: None,
        failure_message: None,
        report: Some(EngineReport {
            info_hash_hex: hex(&report.info_hash),
            final_piece_hash_hex: hex(&report.piece_hash),
            bytes_written: report.bytes_written as u64,
            block_count: report.block_count as u64,
            payload_limit: report.payload_limit as u64,
            payload_high_water: report.payload_high_water as u64,
            verification_buffer: report.verification_buffer as u64,
            piece_count: report.piece_count as u64,
            verified_piece_count: report.verified_piece_count as u64,
            skipped_piece_count: report.skipped_piece_count as u64,
            selected_file_bytes: report.selected_file_bytes,
            skipped_file_bytes: report.skipped_file_bytes,
            padding_bytes: report.padding_bytes,
            selected_written_bytes: report.selected_written_bytes as u64,
            part_written_bytes: report.part_written_bytes as u64,
            materialized_bytes: report.materialized_bytes,
            part_slots_before_materialization: report.part_slots_before_materialization as u64,
            part_slots_after_materialization: report.part_slots_after_materialization as u64,
            part_reopened: report.part_reopened,
            part_path: report.part_path.map(|path| path.display().to_string()),
        }),
        elapsed_millis: millis(elapsed),
    }
}

fn runtime_failure(message: &str) -> TerminalResult {
    TerminalResult {
        outcome: TerminalOutcome::Failed,
        failure_kind: Some(FailureKind::Runtime),
        failure_message: Some(message.to_owned()),
        report: None,
        elapsed_millis: 0,
    }
}

fn millis(duration: Duration) -> u64 {
    duration.as_millis().try_into().unwrap_or(u64::MAX)
}

fn join_timeout(timeout_millis: u64) -> Duration {
    Duration::from_millis(timeout_millis.min(MAX_JOIN_MILLIS))
}

fn hex(bytes: &[u8]) -> String {
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        use std::fmt::Write;
        write!(output, "{byte:02x}").expect("writing to a String cannot fail");
    }
    output
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::io::Read;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::mpsc;

    use super::*;

    static TEST_SEQUENCE: AtomicU64 = AtomicU64::new(0);

    fn config(metainfo_path: String, output_path: String, peer_port: u16) -> EngineConfig {
        EngineConfig {
            metainfo_path,
            output_path,
            peer_port,
            timeout_seconds: 30,
            max_buffered_payload_bytes: 32 * 1024,
            storage_write_delay_millis: 0,
            skip_files: Vec::new(),
            materialize_files: Vec::new(),
        }
    }

    #[test]
    fn rejects_unbounded_configuration_before_starting() {
        let session = EngineSession::new();
        let mut invalid = config(
            "/does/not/matter".to_owned(),
            "/does/not/matter".to_owned(),
            1,
        );
        invalid.max_buffered_payload_bytes = MAX_PAYLOAD_BYTES + 1;
        let result = session.start(invalid);
        assert_eq!(result.disposition, StartDisposition::Rejected);
        assert_eq!(session.snapshot().state, SessionState::Idle);
        assert!(!session.snapshot().task_alive);
    }

    #[test]
    fn rejects_duplicate_start_and_joins_cancellation() {
        let sequence = TEST_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!(
            "rstorrent-android-test-{}-{sequence}",
            std::process::id()
        ));
        fs::create_dir(&root).expect("create test root");
        let metainfo_path = root.join("fixture.torrent");
        let output_path = root.join("output.bin");
        let mut metainfo =
            b"d4:infod6:lengthi1e4:name1:x12:piece lengthi16384e6:pieces20:".to_vec();
        metainfo.extend_from_slice(&[1; 20]);
        metainfo.extend_from_slice(b"ee");
        fs::write(&metainfo_path, metainfo).expect("write metainfo");

        let listener =
            std::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).expect("bind controlled peer");
        let peer_port = listener.local_addr().expect("peer address").port();
        let (accepted_sender, accepted_receiver) = mpsc::channel();
        let peer = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept engine");
            accepted_sender.send(()).expect("signal accept");
            let mut bytes = Vec::new();
            stream.read_to_end(&mut bytes).expect("read until cancel");
        });

        let config = config(
            metainfo_path.display().to_string(),
            output_path.display().to_string(),
            peer_port,
        );
        let session = EngineSession::new();
        let first = session.start(config.clone());
        assert_eq!(first.disposition, StartDisposition::Started);
        accepted_receiver
            .recv_timeout(Duration::from_secs(2))
            .expect("engine connected");
        let second = session.start(config);
        assert_eq!(second.disposition, StartDisposition::Busy);

        let joined = session.cancel_and_join(5_000);
        assert!(joined.joined);
        assert_eq!(
            joined.terminal.expect("terminal result").outcome,
            TerminalOutcome::Cancelled
        );
        let snapshot = session.snapshot();
        assert_eq!(snapshot.state, SessionState::Cancelled);
        assert!(!snapshot.task_alive);
        assert_eq!(snapshot.buffered_payload_bytes, 0);
        assert_eq!(snapshot.requested_bytes, 0);
        assert_eq!(snapshot.received_bytes, 0);
        assert_eq!(snapshot.stored_bytes, 0);

        let repeated = session.cancel_and_join(1);
        assert!(repeated.joined);
        assert_eq!(
            repeated.terminal.expect("terminal result").outcome,
            TerminalOutcome::Cancelled
        );
        peer.join().expect("peer thread");
        assert!(!output_path.exists());
        fs::remove_dir_all(root).expect("remove test root");
    }
}
