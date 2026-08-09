use std::hint::black_box;
use std::thread;
use std::time::{Duration, Instant};

use rstorrent_protocol::mse::{DhPrivateExponent, Rc4, compute_public_key, compute_shared_secret};

const MIB: usize = 1024 * 1024;
const TOTAL_BYTES: usize = 64 * MIB;
const CHUNK_BYTES: usize = 16 * 1024;
const CONCURRENT_STREAMS: usize = 4;
const DH_SAMPLES: usize = 100;

fn main() {
    profile_rc4_contiguous();
    profile_rc4_chunked();
    profile_rc4_concurrent();
    profile_dh();
}

fn profile_rc4_contiguous() {
    let mut bytes = vec![0x5a_u8; TOTAL_BYTES];
    let mut cipher = Rc4::new(&[0x31; 20]).expect("fixed non-empty key");
    let start = Instant::now();
    cipher.apply(&mut bytes);
    let elapsed = start.elapsed();
    let checksum = checksum(black_box(&bytes));
    print_rate("rc4-contiguous", TOTAL_BYTES, elapsed, checksum);
}

fn profile_rc4_chunked() {
    let mut chunk = vec![0xa5_u8; CHUNK_BYTES];
    let mut cipher = Rc4::new(&[0x42; 20]).expect("fixed non-empty key");
    let start = Instant::now();
    for _ in 0..TOTAL_BYTES / CHUNK_BYTES {
        cipher.apply(&mut chunk);
    }
    let aggregate_checksum = checksum(black_box(&chunk));
    print_rate(
        "rc4-16k-chunks",
        TOTAL_BYTES,
        start.elapsed(),
        aggregate_checksum,
    );
}

fn profile_rc4_concurrent() {
    let per_stream = TOTAL_BYTES / CONCURRENT_STREAMS;
    let start = Instant::now();
    let checksum = thread::scope(|scope| {
        let mut handles = Vec::with_capacity(CONCURRENT_STREAMS);
        for stream in 0..CONCURRENT_STREAMS {
            handles.push(scope.spawn(move || {
                let mut chunk = vec![stream as u8; CHUNK_BYTES];
                let mut cipher = Rc4::new(&[0x50 + stream as u8; 20]).expect("fixed non-empty key");
                for _ in 0..per_stream / CHUNK_BYTES {
                    cipher.apply(&mut chunk);
                }
                checksum(black_box(&chunk))
            }));
        }
        handles
            .into_iter()
            .map(|handle| handle.join().expect("RC4 profile thread"))
            .fold(0_u64, |aggregate, value| aggregate ^ value)
    });
    print_rate(
        "rc4-four-stream-16k",
        TOTAL_BYTES,
        start.elapsed(),
        checksum,
    );
}

fn profile_dh() {
    let local = DhPrivateExponent::from_entropy([0x23; 20]);
    let remote = DhPrivateExponent::from_entropy([0x91; 20]);
    let remote_public = compute_public_key(&remote);

    let mut public_samples = Vec::with_capacity(DH_SAMPLES);
    let mut shared_samples = Vec::with_capacity(DH_SAMPLES);
    for _ in 0..DH_SAMPLES {
        let start = Instant::now();
        black_box(compute_public_key(&local));
        public_samples.push(start.elapsed());

        let start = Instant::now();
        black_box(
            compute_shared_secret(&local, remote_public.as_bytes())
                .expect("deterministic valid remote key"),
        );
        shared_samples.push(start.elapsed());
    }
    print_durations("dh-public", &mut public_samples);
    print_durations("dh-shared", &mut shared_samples);
}

fn checksum(bytes: &[u8]) -> u64 {
    bytes
        .iter()
        .fold(0_u64, |value, byte| value.rotate_left(5) ^ u64::from(*byte))
}

fn print_rate(name: &str, bytes: usize, elapsed: Duration, checksum: u64) {
    let gib_per_second = bytes as f64 / elapsed.as_secs_f64() / (1024.0 * 1024.0 * 1024.0);
    println!(
        "{name}: {gib_per_second:.3} GiB/s bytes={bytes} elapsed_ms={:.3} checksum={checksum:016x}",
        elapsed.as_secs_f64() * 1000.0,
    );
}

fn print_durations(name: &str, samples: &mut [Duration]) {
    samples.sort_unstable();
    let median = samples[samples.len() / 2];
    let p95 = samples[(samples.len() * 95 / 100).min(samples.len() - 1)];
    println!(
        "{name}: samples={} median_ms={:.3} p95_ms={:.3}",
        samples.len(),
        median.as_secs_f64() * 1000.0,
        p95.as_secs_f64() * 1000.0,
    );
}
