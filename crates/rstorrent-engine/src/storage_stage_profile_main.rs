mod positional_io;

use std::collections::HashMap;
use std::env;
use std::error::Error;
use std::fs::{File, OpenOptions};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::thread;
use std::time::{Duration, Instant};

use serde::Serialize;
use sha1::{Digest, Sha1};

use positional_io::{read_exact_at, write_all_at};

const KIB: u64 = 1024;
const MIB: u64 = 1024 * KIB;
const GIB: u64 = 1024 * MIB;
const MIN_SIZE: u64 = 16 * KIB;
const MAX_SIZE: u64 = 10 * GIB;
const MIN_PIECE_SIZE: u64 = 16 * KIB;
const MAX_PIECE_SIZE: u64 = 256 * MIB;
const MIN_WRITE_CHUNK_SIZE: u64 = 16 * KIB;
const MAX_WRITE_CHUNK_SIZE: u64 = 256 * KIB;
const HASH_READ_SIZE: usize = 16 * 1024;
const MAX_CONCURRENCY: usize = 8;

#[derive(Debug)]
struct Config {
    path: PathBuf,
    size_bytes: u64,
    piece_size: usize,
    write_chunk_size: usize,
    write_concurrency: usize,
    hash_concurrency: usize,
    write_order: WriteOrder,
}

#[derive(Clone, Copy, Debug)]
enum WriteOrder {
    Sequential,
    Permuted,
}

impl WriteOrder {
    fn as_str(self) -> &'static str {
        match self {
            Self::Sequential => "sequential",
            Self::Permuted => "permuted",
        }
    }
}

#[derive(Debug, Serialize)]
struct OperationStats {
    operations: u64,
    service_seconds: f64,
    active_high_water: usize,
}

#[derive(Debug, Serialize)]
struct StageResult {
    stage: &'static str,
    wall_seconds: f64,
    throughput_mib_s: f64,
    sync_seconds: Option<f64>,
    queue_high_water: Option<usize>,
    write: Option<OperationStats>,
    hash: Option<OperationStats>,
}

#[derive(Debug, Serialize)]
struct ProfileReport {
    schema_version: u32,
    scenario: &'static str,
    size_bytes: u64,
    piece_size: usize,
    write_chunk_size: usize,
    write_concurrency: usize,
    hash_concurrency: usize,
    write_order: &'static str,
    piece_count: u64,
    combined_queue_capacity: usize,
    expected_piece_sha1: String,
    materialized_allocated_bytes: u64,
    combined_allocated_bytes: u64,
    cleanup_succeeded: bool,
    stages: Vec<StageResult>,
}

#[derive(Debug, Default)]
struct TimingCounters {
    operations: AtomicU64,
    service_nanos: AtomicU64,
    active: AtomicUsize,
    active_high_water: AtomicUsize,
}

impl TimingCounters {
    fn measure<T>(&self, operation: impl FnOnce() -> io::Result<T>) -> io::Result<T> {
        let active = self.active.fetch_add(1, Ordering::AcqRel) + 1;
        self.active_high_water.fetch_max(active, Ordering::AcqRel);
        let started = Instant::now();
        let result = operation();
        atomic_saturating_add(&self.service_nanos, duration_nanos(started.elapsed()));
        self.operations.fetch_add(1, Ordering::AcqRel);
        self.active.fetch_sub(1, Ordering::AcqRel);
        result
    }

    fn snapshot(&self) -> OperationStats {
        OperationStats {
            operations: self.operations.load(Ordering::Acquire),
            service_seconds: nanos_seconds(self.service_nanos.load(Ordering::Acquire)),
            active_high_water: self.active_high_water.load(Ordering::Acquire),
        }
    }
}

#[derive(Debug)]
struct OwnedProfilePaths {
    materialized: PathBuf,
    combined: PathBuf,
}

impl OwnedProfilePaths {
    fn new(materialized: PathBuf) -> io::Result<Self> {
        let file_name = materialized
            .file_name()
            .ok_or_else(|| io::Error::other("profile path has no file name"))?;
        let mut combined_name = file_name.to_os_string();
        combined_name.push(".combined");
        let combined = materialized.with_file_name(combined_name);
        if materialized.exists() {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                format!("profile path already exists: {}", materialized.display()),
            ));
        }
        if combined.exists() {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                format!(
                    "combined profile path already exists: {}",
                    combined.display()
                ),
            ));
        }
        Ok(Self {
            materialized,
            combined,
        })
    }

    fn cleanup(&self) -> io::Result<()> {
        remove_if_present(&self.materialized)?;
        remove_if_present(&self.combined)
    }
}

impl Drop for OwnedProfilePaths {
    fn drop(&mut self) {
        let _ = self.cleanup();
    }
}

fn main() {
    if let Err(error) = run() {
        eprintln!("storage stage profile failed: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn Error>> {
    let config = parse_arguments()?;
    let paths = OwnedProfilePaths::new(config.path.clone())?;
    let write_payload = Arc::new(deterministic_payload(config.write_chunk_size));
    let memory_piece = Arc::new(repeated_piece(&write_payload, config.piece_size));
    let expected_piece_sha1: [u8; 20] = Sha1::digest(&*memory_piece).into();

    let materialized = create_profile_file(&paths.materialized, config.size_bytes)?;
    let write_timing = TimingCounters::default();
    let write_started = Instant::now();
    run_writes(
        &materialized,
        config.size_bytes,
        &write_payload,
        config.write_concurrency,
        config.write_order,
        &write_timing,
    )?;
    let write_wall = write_started.elapsed();
    let sync_started = Instant::now();
    materialized.sync_data()?;
    let write_sync = sync_started.elapsed();
    let materialized_allocated_bytes = allocated_bytes(&materialized)?;

    let file_hash_timing = TimingCounters::default();
    let file_hash_started = Instant::now();
    run_file_hashes(
        &materialized,
        config.size_bytes,
        config.piece_size,
        config.hash_concurrency,
        expected_piece_sha1,
        &file_hash_timing,
    )?;
    let file_hash_wall = file_hash_started.elapsed();

    let memory_hash_timing = TimingCounters::default();
    let memory_hash_started = Instant::now();
    run_memory_hashes(
        config.size_bytes,
        &memory_piece,
        config.hash_concurrency,
        expected_piece_sha1,
        &memory_hash_timing,
    )?;
    let memory_hash_wall = memory_hash_started.elapsed();

    let combined = create_profile_file(&paths.combined, config.size_bytes)?;
    let combined_write_timing = TimingCounters::default();
    let combined_hash_timing = TimingCounters::default();
    let combined_queue_high_water = AtomicUsize::new(0);
    let combined_started = Instant::now();
    run_combined(
        &combined,
        config.size_bytes,
        config.piece_size,
        &write_payload,
        config.write_concurrency,
        config.hash_concurrency,
        config.write_order,
        expected_piece_sha1,
        &combined_write_timing,
        &combined_hash_timing,
        &combined_queue_high_water,
    )?;
    let combined_wall = combined_started.elapsed();
    let combined_sync_started = Instant::now();
    combined.sync_data()?;
    let combined_sync = combined_sync_started.elapsed();
    let combined_allocated_bytes = allocated_bytes(&combined)?;

    drop(materialized);
    drop(combined);
    paths.cleanup()?;
    let cleanup_succeeded = !paths.materialized.exists() && !paths.combined.exists();
    if !cleanup_succeeded {
        return Err("owned profile files remain after cleanup".into());
    }

    let report = ProfileReport {
        schema_version: 1,
        scenario: "raw-positional-write-and-sha1",
        size_bytes: config.size_bytes,
        piece_size: config.piece_size,
        write_chunk_size: config.write_chunk_size,
        write_concurrency: config.write_concurrency,
        hash_concurrency: config.hash_concurrency,
        write_order: config.write_order.as_str(),
        piece_count: config.size_bytes / config.piece_size as u64,
        combined_queue_capacity: config
            .write_concurrency
            .saturating_add(config.hash_concurrency),
        expected_piece_sha1: hex_digest(expected_piece_sha1),
        materialized_allocated_bytes,
        combined_allocated_bytes,
        cleanup_succeeded,
        stages: vec![
            stage_result("write", config.size_bytes, write_wall)
                .with_sync(write_sync)
                .with_write(write_timing.snapshot()),
            stage_result("file_hash_warm", config.size_bytes, file_hash_wall)
                .with_hash(file_hash_timing.snapshot()),
            stage_result("memory_hash", config.size_bytes, memory_hash_wall)
                .with_hash(memory_hash_timing.snapshot()),
            stage_result("combined", config.size_bytes, combined_wall)
                .with_sync(combined_sync)
                .with_queue(combined_queue_high_water.load(Ordering::Acquire))
                .with_write(combined_write_timing.snapshot())
                .with_hash(combined_hash_timing.snapshot()),
        ],
    };
    println!("{}", serde_json::to_string_pretty(&report)?);
    Ok(())
}

impl StageResult {
    fn with_sync(mut self, duration: Duration) -> Self {
        self.sync_seconds = Some(duration.as_secs_f64());
        self
    }

    fn with_queue(mut self, high_water: usize) -> Self {
        self.queue_high_water = Some(high_water);
        self
    }

    fn with_write(mut self, stats: OperationStats) -> Self {
        self.write = Some(stats);
        self
    }

    fn with_hash(mut self, stats: OperationStats) -> Self {
        self.hash = Some(stats);
        self
    }
}

fn stage_result(stage: &'static str, size_bytes: u64, wall: Duration) -> StageResult {
    StageResult {
        stage,
        wall_seconds: wall.as_secs_f64(),
        throughput_mib_s: size_bytes as f64 / MIB as f64 / wall.as_secs_f64(),
        sync_seconds: None,
        queue_high_water: None,
        write: None,
        hash: None,
    }
}

fn parse_arguments() -> Result<Config, Box<dyn Error>> {
    let mut arguments = env::args_os();
    let program = arguments.next().unwrap_or_default();
    let mut values = HashMap::new();
    while let Some(flag) = arguments.next() {
        let flag = flag
            .into_string()
            .map_err(|_| "argument names must be valid Unicode")?;
        if flag == "--help" || flag == "-h" {
            print_usage(Path::new(&program));
            std::process::exit(0);
        }
        if !flag.starts_with("--") {
            return Err(format!("unexpected positional argument {flag:?}").into());
        }
        let value = arguments
            .next()
            .ok_or_else(|| format!("missing value for {flag}"))?;
        if values.insert(flag.clone(), value).is_some() {
            return Err(format!("duplicate argument {flag}").into());
        }
    }

    let path = PathBuf::from(required_value(&mut values, "--path")?);
    let size_bytes = parse_mib(&mut values, "--size-mib", MIN_SIZE, MAX_SIZE)?;
    let piece_size = parse_kib(
        &mut values,
        "--piece-size-kib",
        MIN_PIECE_SIZE,
        MAX_PIECE_SIZE,
    )?;
    let write_chunk_size = parse_kib(
        &mut values,
        "--write-chunk-kib",
        MIN_WRITE_CHUNK_SIZE,
        MAX_WRITE_CHUNK_SIZE,
    )?;
    let write_concurrency = parse_concurrency(&mut values, "--write-concurrency")?;
    let hash_concurrency = parse_concurrency(&mut values, "--hash-concurrency")?;
    let write_order = parse_write_order(&mut values)?;
    if !values.is_empty() {
        let mut unknown: Vec<_> = values.keys().cloned().collect();
        unknown.sort();
        return Err(format!("unknown arguments: {}", unknown.join(", ")).into());
    }
    if !piece_size.is_power_of_two() || !write_chunk_size.is_power_of_two() {
        return Err("piece and write-chunk sizes must be powers of two".into());
    }
    if size_bytes % piece_size != 0 {
        return Err("size must be an exact multiple of the piece size".into());
    }
    if piece_size % write_chunk_size != 0 {
        return Err("piece size must be an exact multiple of write-chunk size".into());
    }
    if size_bytes % write_chunk_size != 0 {
        return Err("size must be an exact multiple of write-chunk size".into());
    }
    Ok(Config {
        path,
        size_bytes,
        piece_size: usize::try_from(piece_size)?,
        write_chunk_size: usize::try_from(write_chunk_size)?,
        write_concurrency,
        hash_concurrency,
        write_order,
    })
}

fn required_value(
    values: &mut HashMap<String, std::ffi::OsString>,
    name: &str,
) -> Result<std::ffi::OsString, Box<dyn Error>> {
    values
        .remove(name)
        .ok_or_else(|| format!("missing required argument {name}").into())
}

fn parse_mib(
    values: &mut HashMap<String, std::ffi::OsString>,
    name: &str,
    minimum: u64,
    maximum: u64,
) -> Result<u64, Box<dyn Error>> {
    parse_scaled(values, name, MIB, minimum, maximum)
}

fn parse_kib(
    values: &mut HashMap<String, std::ffi::OsString>,
    name: &str,
    minimum: u64,
    maximum: u64,
) -> Result<u64, Box<dyn Error>> {
    parse_scaled(values, name, KIB, minimum, maximum)
}

fn parse_scaled(
    values: &mut HashMap<String, std::ffi::OsString>,
    name: &str,
    scale: u64,
    minimum: u64,
    maximum: u64,
) -> Result<u64, Box<dyn Error>> {
    let raw = required_value(values, name)?;
    let parsed: u64 = raw
        .to_str()
        .ok_or_else(|| format!("{name} must be valid Unicode"))?
        .parse()
        .map_err(|_| format!("{name} must be a positive integer"))?;
    let scaled = parsed
        .checked_mul(scale)
        .ok_or_else(|| format!("{name} overflows bytes"))?;
    if !(minimum..=maximum).contains(&scaled) {
        return Err(format!("{name} must be between {minimum} and {maximum} bytes").into());
    }
    Ok(scaled)
}

fn parse_concurrency(
    values: &mut HashMap<String, std::ffi::OsString>,
    name: &str,
) -> Result<usize, Box<dyn Error>> {
    let raw = required_value(values, name)?;
    let parsed: usize = raw
        .to_str()
        .ok_or_else(|| format!("{name} must be valid Unicode"))?
        .parse()
        .map_err(|_| format!("{name} must be a positive integer"))?;
    if !(1..=MAX_CONCURRENCY).contains(&parsed) {
        return Err(format!("{name} must be between 1 and {MAX_CONCURRENCY}").into());
    }
    Ok(parsed)
}

fn parse_write_order(
    values: &mut HashMap<String, std::ffi::OsString>,
) -> Result<WriteOrder, Box<dyn Error>> {
    let raw = required_value(values, "--write-order")?;
    match raw.to_str() {
        Some("sequential") => Ok(WriteOrder::Sequential),
        Some("permuted") => Ok(WriteOrder::Permuted),
        _ => Err("--write-order must be sequential or permuted".into()),
    }
}

fn print_usage(program: &Path) {
    println!(
        concat!(
            "usage: {} --path PATH --size-mib MIB --piece-size-kib KIB \\",
            "\n  --write-chunk-kib KIB --write-concurrency N ",
            "--hash-concurrency N \\",
            "\n  --write-order sequential|permuted"
        ),
        program.display()
    );
}

fn deterministic_payload(length: usize) -> Vec<u8> {
    (0..length)
        .map(|offset| {
            ((offset.wrapping_mul(73)) ^ (offset >> 3) ^ (offset.wrapping_mul(offset) >> 11) ^ 0xa5)
                as u8
        })
        .collect()
}

fn repeated_piece(chunk: &[u8], piece_size: usize) -> Vec<u8> {
    let mut piece = Vec::with_capacity(piece_size);
    while piece.len() < piece_size {
        piece.extend_from_slice(chunk);
    }
    piece
}

fn create_profile_file(path: &Path, length: u64) -> io::Result<File> {
    let file = OpenOptions::new()
        .create_new(true)
        .read(true)
        .write(true)
        .open(path)?;
    file.set_len(length)?;
    Ok(file)
}

fn run_writes(
    file: &File,
    size_bytes: u64,
    payload: &[u8],
    concurrency: usize,
    write_order: WriteOrder,
    timing: &TimingCounters,
) -> io::Result<()> {
    let chunk_count = size_bytes / payload.len() as u64;
    let multiplier = permutation_multiplier(chunk_count);
    let next_chunk = AtomicU64::new(0);
    thread::scope(|scope| {
        let mut workers = Vec::with_capacity(concurrency);
        for _ in 0..concurrency {
            workers.push(scope.spawn(|| -> io::Result<()> {
                loop {
                    let sequence = next_chunk.fetch_add(1, Ordering::AcqRel);
                    if sequence >= chunk_count {
                        break;
                    }
                    let chunk = ordered_index(sequence, chunk_count, write_order, multiplier);
                    let offset = chunk * payload.len() as u64;
                    timing.measure(|| write_all_at(file, payload, offset))?;
                }
                Ok(())
            }));
        }
        join_workers(workers)
    })
}

fn run_file_hashes(
    file: &File,
    size_bytes: u64,
    piece_size: usize,
    concurrency: usize,
    expected: [u8; 20],
    timing: &TimingCounters,
) -> io::Result<()> {
    let piece_count = size_bytes / piece_size as u64;
    let next_piece = AtomicU64::new(0);
    thread::scope(|scope| {
        let mut workers = Vec::with_capacity(concurrency);
        for _ in 0..concurrency {
            workers.push(scope.spawn(|| -> io::Result<()> {
                loop {
                    let piece = next_piece.fetch_add(1, Ordering::AcqRel);
                    if piece >= piece_count {
                        break;
                    }
                    let digest = timing.measure(|| hash_file_piece(file, piece, piece_size))?;
                    if digest != expected {
                        return Err(io::Error::other(format!(
                            "file hash mismatch at piece {piece}"
                        )));
                    }
                }
                Ok(())
            }));
        }
        join_workers(workers)
    })
}

fn run_memory_hashes(
    size_bytes: u64,
    piece: &[u8],
    concurrency: usize,
    expected: [u8; 20],
    timing: &TimingCounters,
) -> io::Result<()> {
    let piece_count = size_bytes / piece.len() as u64;
    let next_piece = AtomicU64::new(0);
    thread::scope(|scope| {
        let mut workers = Vec::with_capacity(concurrency);
        for _ in 0..concurrency {
            workers.push(scope.spawn(|| -> io::Result<()> {
                loop {
                    let piece_index = next_piece.fetch_add(1, Ordering::AcqRel);
                    if piece_index >= piece_count {
                        break;
                    }
                    let digest: [u8; 20] = timing.measure(|| Ok(Sha1::digest(piece).into()))?;
                    if digest != expected {
                        return Err(io::Error::other(format!(
                            "memory hash mismatch at piece {piece_index}"
                        )));
                    }
                }
                Ok(())
            }));
        }
        join_workers(workers)
    })
}

#[allow(clippy::too_many_arguments)]
fn run_combined(
    file: &File,
    size_bytes: u64,
    piece_size: usize,
    payload: &[u8],
    write_concurrency: usize,
    hash_concurrency: usize,
    write_order: WriteOrder,
    expected: [u8; 20],
    write_timing: &TimingCounters,
    hash_timing: &TimingCounters,
    queue_high_water: &AtomicUsize,
) -> io::Result<()> {
    let piece_count = size_bytes / piece_size as u64;
    let chunk_count = size_bytes / payload.len() as u64;
    let chunks_per_piece = piece_size / payload.len();
    let multiplier = permutation_multiplier(chunk_count);
    let next_chunk = AtomicU64::new(0);
    let remaining_chunks: Vec<_> = (0..piece_count)
        .map(|_| AtomicUsize::new(chunks_per_piece))
        .collect();
    let queued = AtomicUsize::new(0);
    let ready_capacity = write_concurrency.saturating_add(hash_concurrency);
    let (ready_sender, ready_receiver) = mpsc::sync_channel::<u64>(ready_capacity);
    let ready_receiver = Arc::new(Mutex::new(ready_receiver));
    thread::scope(|scope| {
        let mut hash_workers = Vec::with_capacity(hash_concurrency);
        for _ in 0..hash_concurrency {
            let queued = &queued;
            let ready_receiver = ready_receiver.clone();
            hash_workers.push(scope.spawn(move || -> io::Result<()> {
                loop {
                    let received = ready_receiver
                        .lock()
                        .unwrap_or_else(|poisoned| poisoned.into_inner())
                        .recv();
                    let piece = match received {
                        Ok(piece) => piece,
                        Err(_) => break,
                    };
                    queued.fetch_sub(1, Ordering::AcqRel);
                    let digest =
                        hash_timing.measure(|| hash_file_piece(file, piece, piece_size))?;
                    if digest != expected {
                        return Err(io::Error::other(format!(
                            "combined hash mismatch at piece {piece}"
                        )));
                    }
                }
                Ok(())
            }));
        }
        drop(ready_receiver);

        let mut write_workers = Vec::with_capacity(write_concurrency);
        for _ in 0..write_concurrency {
            let sender = ready_sender.clone();
            let next_chunk = &next_chunk;
            let queued = &queued;
            let remaining_chunks = &remaining_chunks;
            write_workers.push(scope.spawn(move || -> io::Result<()> {
                loop {
                    let sequence = next_chunk.fetch_add(1, Ordering::AcqRel);
                    if sequence >= chunk_count {
                        break;
                    }
                    let chunk = ordered_index(sequence, chunk_count, write_order, multiplier);
                    let offset = chunk * payload.len() as u64;
                    write_timing.measure(|| write_all_at(file, payload, offset))?;
                    let piece = usize::try_from(offset / piece_size as u64)
                        .map_err(|_| io::Error::other("piece index overflow"))?;
                    if remaining_chunks[piece].fetch_sub(1, Ordering::AcqRel) != 1 {
                        continue;
                    }
                    let depth = queued.fetch_add(1, Ordering::AcqRel) + 1;
                    queue_high_water.fetch_max(depth, Ordering::AcqRel);
                    if sender.send(piece as u64).is_err() {
                        queued.fetch_sub(1, Ordering::AcqRel);
                        return Err(io::Error::other("combined hash workers stopped early"));
                    }
                }
                Ok(())
            }));
        }
        drop(ready_sender);

        let write_result = join_workers(write_workers);
        let hash_result = join_workers(hash_workers);
        write_result.and(hash_result)
    })
}

fn ordered_index(sequence: u64, count: u64, order: WriteOrder, multiplier: u64) -> u64 {
    match order {
        WriteOrder::Sequential => sequence,
        WriteOrder::Permuted => (sequence * multiplier + count / 3) % count,
    }
}

fn permutation_multiplier(count: u64) -> u64 {
    let mut candidate = count / 2 + 1;
    while greatest_common_divisor(candidate, count) != 1 {
        candidate += 1;
    }
    candidate
}

fn greatest_common_divisor(mut left: u64, mut right: u64) -> u64 {
    while right != 0 {
        let remainder = left % right;
        left = right;
        right = remainder;
    }
    left
}

fn hash_file_piece(file: &File, piece: u64, piece_size: usize) -> io::Result<[u8; 20]> {
    let mut hasher = Sha1::new();
    let mut buffer = [0_u8; HASH_READ_SIZE];
    let mut remaining = piece_size;
    let mut offset = piece
        .checked_mul(piece_size as u64)
        .ok_or_else(|| io::Error::other("piece offset overflow"))?;
    while remaining > 0 {
        let length = remaining.min(buffer.len());
        read_exact_at(file, &mut buffer[..length], offset)?;
        hasher.update(&buffer[..length]);
        remaining -= length;
        offset += length as u64;
    }
    Ok(hasher.finalize().into())
}

fn join_workers<T>(workers: Vec<thread::ScopedJoinHandle<'_, io::Result<T>>>) -> io::Result<()> {
    let mut first_error = None;
    for worker in workers {
        let result = worker
            .join()
            .map_err(|_| io::Error::other("storage profile worker panicked"))?;
        if let Err(error) = result {
            first_error.get_or_insert(error);
        }
    }
    match first_error {
        Some(error) => Err(error),
        None => Ok(()),
    }
}

fn remove_if_present(path: &Path) -> io::Result<()> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

#[cfg(unix)]
fn allocated_bytes(file: &File) -> io::Result<u64> {
    use std::os::unix::fs::MetadataExt;

    Ok(file.metadata()?.blocks().saturating_mul(512))
}

#[cfg(not(unix))]
fn allocated_bytes(file: &File) -> io::Result<u64> {
    Ok(file.metadata()?.len())
}

fn duration_nanos(duration: Duration) -> u64 {
    u64::try_from(duration.as_nanos()).unwrap_or(u64::MAX)
}

fn nanos_seconds(nanos: u64) -> f64 {
    nanos as f64 / 1_000_000_000.0
}

fn atomic_saturating_add(target: &AtomicU64, value: u64) {
    let _ = target.fetch_update(Ordering::AcqRel, Ordering::Acquire, |current| {
        Some(current.saturating_add(value))
    });
}

fn hex_digest(digest: [u8; 20]) -> String {
    let mut rendered = String::with_capacity(40);
    for byte in digest {
        use std::fmt::Write;
        let _ = write!(rendered, "{byte:02x}");
    }
    rendered
}

#[cfg(test)]
mod tests {
    use super::{WriteOrder, ordered_index, permutation_multiplier};

    #[test]
    fn permuted_order_visits_every_chunk_once() {
        let count = 40_960;
        let multiplier = permutation_multiplier(count);
        let mut visited = vec![false; count as usize];
        for sequence in 0..count {
            let chunk = ordered_index(sequence, count, WriteOrder::Permuted, multiplier);
            assert!(!visited[chunk as usize]);
            visited[chunk as usize] = true;
        }
        assert!(visited.into_iter().all(|value| value));
    }

    #[test]
    fn sequential_order_preserves_chunk_indexes() {
        let count = 64;
        let multiplier = permutation_multiplier(count);
        for sequence in 0..count {
            assert_eq!(
                ordered_index(sequence, count, WriteOrder::Sequential, multiplier),
                sequence
            );
        }
    }
}
