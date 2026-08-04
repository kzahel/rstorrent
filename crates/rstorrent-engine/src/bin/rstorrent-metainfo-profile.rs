use core::sync::atomic::{AtomicUsize, Ordering};
use std::alloc::{GlobalAlloc, Layout, System};
use std::time::Instant;

use rstorrent_protocol::metainfo::{EXPLICIT_IMPORT_METAINFO_LIMITS, Metainfo};
use sha1::{Digest, Sha1};

struct MeasuringAllocator;

static LIVE_BYTES: AtomicUsize = AtomicUsize::new(0);
static PEAK_BYTES: AtomicUsize = AtomicUsize::new(0);

fn record_allocation(length: usize) {
    let live = LIVE_BYTES.fetch_add(length, Ordering::Relaxed) + length;
    let mut peak = PEAK_BYTES.load(Ordering::Relaxed);
    while live > peak {
        match PEAK_BYTES.compare_exchange_weak(peak, live, Ordering::Relaxed, Ordering::Relaxed) {
            Ok(_) => break,
            Err(actual) => peak = actual,
        }
    }
}

unsafe impl GlobalAlloc for MeasuringAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let pointer = unsafe { System.alloc(layout) };
        if !pointer.is_null() {
            record_allocation(layout.size());
        }
        pointer
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        let pointer = unsafe { System.alloc_zeroed(layout) };
        if !pointer.is_null() {
            record_allocation(layout.size());
        }
        pointer
    }

    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        LIVE_BYTES.fetch_sub(layout.size(), Ordering::Relaxed);
        unsafe { System.dealloc(pointer, layout) };
    }

    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        let new_pointer = unsafe { System.realloc(pointer, layout, new_size) };
        if !new_pointer.is_null() {
            if new_size >= layout.size() {
                record_allocation(new_size - layout.size());
            } else {
                LIVE_BYTES.fetch_sub(layout.size() - new_size, Ordering::Relaxed);
            }
        }
        new_pointer
    }
}

#[global_allocator]
static GLOBAL_ALLOCATOR: MeasuringAllocator = MeasuringAllocator;

const STRUCTURE_INNER_LISTS: usize = 244;
const STRUCTURE_ITEMS_PER_LIST: usize = 4096;
const STRUCTURE_DECODED_ITEMS: usize = 1
    + 1
    + 1
    + STRUCTURE_INNER_LISTS
    + STRUCTURE_INNER_LISTS * STRUCTURE_ITEMS_PER_LIST
    + 1
    + 1
    + 8;

fn main() {
    let info = minimal_info();
    let expected_hash: [u8; 20] = Sha1::digest(&info).into();

    let size_fixture = size_heavy_fixture(&info, EXPLICIT_IMPORT_METAINFO_LIMITS.max_outer_bytes);
    profile_fixture("size-heavy", &size_fixture, 13, expected_hash);

    let structure_fixture = structure_heavy_fixture(&info);
    profile_fixture(
        "structure-heavy",
        &structure_fixture,
        STRUCTURE_DECODED_ITEMS,
        expected_hash,
    );
}

fn profile_fixture(name: &str, bytes: &[u8], decoded_items: usize, expected_hash: [u8; 20]) {
    let baseline = LIVE_BYTES.load(Ordering::Relaxed);
    PEAK_BYTES.store(baseline, Ordering::Relaxed);
    let started = Instant::now();
    let metainfo = Metainfo::from_bytes_with_limits(bytes, EXPLICIT_IMPORT_METAINFO_LIMITS)
        .expect("generated fixture must parse");
    let elapsed = started.elapsed();
    let transient_peak = PEAK_BYTES.load(Ordering::Relaxed).saturating_sub(baseline);
    assert_eq!(metainfo.info_hash, expected_hash);
    assert!(transient_peak <= 128 * 1024 * 1024);

    println!(
        "fixture={name} input_bytes={} info_bytes={} decoded_items={decoded_items} wall_us={} transient_peak_bytes={} info_hash={}",
        bytes.len(),
        minimal_info().len(),
        elapsed.as_micros(),
        transient_peak,
        hex(expected_hash),
    );
}

fn minimal_info() -> Vec<u8> {
    let mut info = b"d6:lengthi1e4:name1:x12:piece lengthi1e6:pieces20:".to_vec();
    info.extend_from_slice(&[0; 20]);
    info.push(b'e');
    info
}

fn size_heavy_fixture(info: &[u8], target_length: usize) -> Vec<u8> {
    let fixed_length = b"d7:comment".len() + b":4:info".len() + info.len() + 1;
    let mut payload_length = target_length - fixed_length - target_length.to_string().len();
    loop {
        let actual = fixed_length + payload_length.to_string().len() + payload_length;
        if actual == target_length {
            break;
        }
        payload_length = payload_length
            .checked_add_signed(target_length as isize - actual as isize)
            .expect("target fixture length");
    }

    let mut bytes = Vec::with_capacity(target_length);
    bytes.extend_from_slice(format!("d7:comment{payload_length}:").as_bytes());
    bytes.resize(bytes.len() + payload_length, b'x');
    bytes.extend_from_slice(b"4:info");
    bytes.extend_from_slice(info);
    bytes.push(b'e');
    assert_eq!(bytes.len(), target_length);
    bytes
}

fn structure_heavy_fixture(info: &[u8]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(3 * STRUCTURE_INNER_LISTS * STRUCTURE_ITEMS_PER_LIST);
    bytes.extend_from_slice(b"d1:al");
    for _ in 0..STRUCTURE_INNER_LISTS {
        bytes.push(b'l');
        for _ in 0..STRUCTURE_ITEMS_PER_LIST {
            bytes.extend_from_slice(b"i0e");
        }
        bytes.push(b'e');
    }
    bytes.extend_from_slice(b"e4:info");
    bytes.extend_from_slice(info);
    bytes.push(b'e');
    bytes
}

fn hex(bytes: [u8; 20]) -> String {
    let mut output = String::with_capacity(40);
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    for byte in bytes {
        output.push(char::from(DIGITS[usize::from(byte >> 4)]));
        output.push(char::from(DIGITS[usize::from(byte & 0x0f)]));
    }
    output
}
