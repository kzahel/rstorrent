use core::sync::atomic::{AtomicUsize, Ordering};
use std::alloc::{GlobalAlloc, Layout, System};
use std::env;
use std::fs::{self, File};
use std::io::{self, BufWriter, Write};
use std::time::Instant;

use rstorrent_protocol::metainfo::{
    EXPLICIT_IMPORT_METAINFO_LIMITS, Metainfo, MetainfoLimits, ParsedInfoKind, ParsedOuterMetainfo,
    V2Metainfo,
};
use serde::Serialize;

const MIB: usize = 1024 * 1024;
const EXPLICIT_MAX_BYTES: usize = 64 * MIB;
const EXPLICIT_MAX_TOKENS: usize = 3_000_000;
const PEER_MAX_BYTES: usize = 30 * MIB;
const PEER_MAX_TOKENS: usize = 2_500_000;
const MAX_PIECES: usize = 2_097_152;

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

fn main() {
    let arguments: Vec<String> = env::args().collect();
    let result = match arguments.get(1).map(String::as_str) {
        Some("generate") => generate_command(&arguments[2..]),
        Some("profile") => profile_command(&arguments[2..]),
        Some("bep52") => bep52_command(&arguments[2..]),
        _ => Err(usage()),
    };
    if let Err(error) = result {
        eprintln!("{error}");
        std::process::exit(2);
    }
}

fn usage() -> String {
    concat!(
        "usage:\n",
        "  rstorrent-metainfo-compare generate FIXTURE OUTPUT VALUE [SECOND]\n",
        "  rstorrent-metainfo-compare profile explicit|peer INPUT\n",
        "  rstorrent-metainfo-compare bep52 INPUT\n",
        "fixtures: size-outer, structure-outer, structure-info, many-files-outer, ",
        "many-files-info, many-trackers-outer, many-pieces-outer, many-pieces-info, ",
        "piece-length-outer, piece-length-info, tracker-url-outer, ",
        "long-path-outer, deep-outer, invalid-utf8-path-outer\n",
    )
    .to_owned()
}

#[derive(Serialize)]
struct NormalizedBep52 {
    implementation: &'static str,
    format: &'static str,
    exact_info_bytes: usize,
    v1_info_hash: Option<String>,
    v2_info_hash: Option<String>,
    piece_length: u32,
    payload_bytes: u64,
    logical_bytes: u64,
    logical_pieces: usize,
    files: Vec<NormalizedV2File>,
    v1_files: Vec<NormalizedV1File>,
    piece_layers: Vec<NormalizedPieceLayer>,
    retained_layer_hashes: usize,
    parse_wall_us: u128,
    transient_peak_bytes: usize,
}

#[derive(Serialize)]
struct NormalizedV2File {
    raw_path_hex: Vec<String>,
    length: u64,
    logical_offset: u64,
    start_piece: u32,
    piece_count: u32,
    pieces_root: Option<String>,
}

#[derive(Serialize)]
struct NormalizedV1File {
    path: Vec<String>,
    length: u64,
    offset: u64,
    padding: bool,
}

#[derive(Serialize)]
struct NormalizedPieceLayer {
    pieces_root: String,
    hashes: Vec<String>,
}

fn bep52_command(arguments: &[String]) -> Result<(), String> {
    let [input] = arguments else {
        return Err(usage());
    };
    let bytes = fs::read(input).map_err(|error| format!("read {input}: {error}"))?;
    let baseline = LIVE_BYTES.load(Ordering::Relaxed);
    PEAK_BYTES.store(baseline, Ordering::Relaxed);
    let started = Instant::now();
    let parsed =
        ParsedOuterMetainfo::from_bytes_with_limits(&bytes, EXPLICIT_IMPORT_METAINFO_LIMITS)
            .map_err(|error| format!("parse {input}: {error:?}"))?;
    let parse_wall_us = started.elapsed().as_micros();
    let transient_peak_bytes = PEAK_BYTES.load(Ordering::Relaxed).saturating_sub(baseline);
    let info = parsed.info();
    let (format, v2, v1_files) = match info.kind() {
        ParsedInfoKind::V1(v1) => ("v1", None, normalized_v1_files(v1)),
        ParsedInfoKind::V2(v2) => ("v2", Some(v2), Vec::new()),
        ParsedInfoKind::Hybrid(hybrid) => {
            ("hybrid", Some(&hybrid.v2), normalized_v1_files(&hybrid.v1))
        }
    };
    let (piece_length, payload_bytes, logical_bytes, logical_pieces, files) = v2.map_or_else(
        || {
            let ParsedInfoKind::V1(v1) = info.kind() else {
                unreachable!("non-v2 parsed kind is v1")
            };
            (
                v1.piece_length,
                v1.total_length,
                v1.total_length,
                v1.piece_count(),
                Vec::new(),
            )
        },
        normalized_v2_files,
    );
    let layers = parsed.piece_layers();
    let piece_layers = layers
        .into_iter()
        .flat_map(|layers| {
            layers.entries().iter().map(|entry| NormalizedPieceLayer {
                pieces_root: hex(entry.pieces_root),
                hashes: layers.hashes()[entry.hashes.clone()]
                    .iter()
                    .copied()
                    .map(hex)
                    .collect(),
            })
        })
        .collect();
    let output = NormalizedBep52 {
        implementation: "rstorrent",
        format,
        exact_info_bytes: info.exact_info_bytes().len(),
        v1_info_hash: info.info_hashes().v1_hash().map(|hash| hash.to_string()),
        v2_info_hash: info.info_hashes().v2_hash().map(|hash| hash.to_string()),
        piece_length,
        payload_bytes,
        logical_bytes,
        logical_pieces,
        files,
        v1_files,
        retained_layer_hashes: layers.map_or(0, |layers| layers.hashes().len()),
        parse_wall_us,
        transient_peak_bytes,
        piece_layers,
    };
    println!(
        "{}",
        serde_json::to_string(&output).map_err(|error| error.to_string())?
    );
    Ok(())
}

fn normalized_v2_files(v2: &V2Metainfo) -> (u32, u64, u64, usize, Vec<NormalizedV2File>) {
    let files = v2
        .files
        .iter()
        .zip(v2.layout.files())
        .map(|(file, geometry)| NormalizedV2File {
            raw_path_hex: file.raw_path.iter().map(hex).collect(),
            length: file.length,
            logical_offset: geometry.logical_offset(),
            start_piece: geometry.start_piece(),
            piece_count: geometry.piece_count(),
            pieces_root: file.pieces_root.map(hex),
        })
        .collect();
    (
        v2.piece_length,
        v2.total_length,
        v2.layout.logical_length(),
        v2.layout.piece_count(),
        files,
    )
}

fn normalized_v1_files(v1: &Metainfo) -> Vec<NormalizedV1File> {
    v1.files
        .iter()
        .map(|file| NormalizedV1File {
            path: file.path.clone(),
            length: file.length,
            offset: file.offset,
            padding: file.padding,
        })
        .collect()
}

fn hex(bytes: impl AsRef<[u8]>) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let bytes = bytes.as_ref();
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(char::from(DIGITS[usize::from(byte >> 4)]));
        output.push(char::from(DIGITS[usize::from(byte & 0x0f)]));
    }
    output
}

fn generate_command(arguments: &[String]) -> Result<(), String> {
    let [fixture, output, value, rest @ ..] = arguments else {
        return Err(usage());
    };
    if rest.len() > 1 {
        return Err(usage());
    }
    let value = parse_usize(value, "VALUE")?;
    let second = rest
        .first()
        .map(|value| parse_usize(value, "SECOND"))
        .transpose()?;
    let file = File::create(output).map_err(|error| format!("create {output}: {error}"))?;
    let mut writer = BufWriter::new(file);
    match fixture.as_str() {
        "size-outer" => write_size_outer(&mut writer, value),
        "structure-outer" => write_structure_outer(&mut writer, value),
        "structure-info" => write_structure_info(&mut writer, value),
        "many-files-outer" => write_many_files_outer(&mut writer, value),
        "many-files-info" => write_many_files_info(&mut writer, value),
        "many-trackers-outer" => write_many_trackers_outer(&mut writer, value),
        "many-pieces-outer" => write_many_pieces_outer(&mut writer, value),
        "many-pieces-info" => write_many_pieces_info(&mut writer, value),
        "piece-length-outer" => write_piece_length_outer(&mut writer, value),
        "piece-length-info" => write_piece_length_info(&mut writer, value),
        "tracker-url-outer" => write_tracker_url_outer(&mut writer, value),
        "long-path-outer" => write_long_path_outer(&mut writer, value, second.unwrap_or(1)),
        "deep-outer" => write_deep_outer(&mut writer, value),
        "invalid-utf8-path-outer" => write_invalid_utf8_path_outer(&mut writer),
        _ => return Err(format!("unknown fixture {fixture}")),
    }
    .map_err(|error| format!("write {output}: {error}"))?;
    writer
        .flush()
        .map_err(|error| format!("flush {output}: {error}"))?;
    Ok(())
}

fn profile_command(arguments: &[String]) -> Result<(), String> {
    let [profile, input] = arguments else {
        return Err(usage());
    };
    let (limits, info_only) = match profile.as_str() {
        "explicit" => (
            comparison_limits(EXPLICIT_MAX_BYTES, EXPLICIT_MAX_TOKENS, 100),
            false,
        ),
        "peer" => (
            comparison_limits(PEER_MAX_BYTES, PEER_MAX_TOKENS, 200),
            true,
        ),
        _ => return Err(usage()),
    };
    let bytes = fs::read(input).map_err(|error| format!("read {input}: {error}"))?;
    let lexical_tokens = count_lexical_tokens(&bytes).unwrap_or(0);
    let baseline = LIVE_BYTES.load(Ordering::Relaxed);
    PEAK_BYTES.store(baseline, Ordering::Relaxed);
    let started = Instant::now();
    let parsed = if info_only {
        Metainfo::from_info_bytes_with_limits(&bytes, limits)
    } else {
        Metainfo::from_bytes_with_limits(&bytes, limits)
    };
    let wall_us = started.elapsed().as_micros();
    let transient_peak = PEAK_BYTES.load(Ordering::Relaxed).saturating_sub(baseline);
    match parsed {
        Ok(metainfo) => {
            let info_bytes = if info_only {
                bytes.len()
            } else {
                Metainfo::info_bytes_with_limits(&bytes, limits)
                    .map_err(|error| error.to_string())?
                    .len()
            };
            let path_bytes: usize = metainfo
                .files
                .iter()
                .map(|file| file.path.iter().map(String::len).sum::<usize>())
                .sum();
            println!(
                "implementation=rstorrent profile={profile} accepted=true input_bytes={} info_bytes={info_bytes} lexical_tokens={lexical_tokens} files={} pieces={} path_bytes={path_bytes} wall_us={wall_us} transient_peak_bytes={transient_peak}",
                bytes.len(),
                metainfo.files.len(),
                metainfo.piece_count(),
            );
            Ok(())
        }
        Err(error) => {
            println!(
                "implementation=rstorrent profile={profile} accepted=false input_bytes={} lexical_tokens={lexical_tokens} wall_us={wall_us} transient_peak_bytes={transient_peak} error={error:?}",
                bytes.len(),
            );
            std::process::exit(1);
        }
    }
}

const fn comparison_limits(max_bytes: usize, max_items: usize, depth: usize) -> MetainfoLimits {
    MetainfoLimits {
        max_outer_bytes: max_bytes,
        max_info_bytes: max_bytes,
        max_string_bytes: max_bytes,
        max_decoded_items: max_items,
        max_depth: depth,
        max_collection_entries: max_items,
        max_files: 400_000,
        max_pieces: MAX_PIECES,
        max_path_components: max_items,
        max_path_component_bytes: max_bytes,
        max_path_bytes: max_bytes,
    }
}

fn parse_usize(value: &str, name: &str) -> Result<usize, String> {
    value
        .parse()
        .map_err(|_| format!("{name} must be a nonnegative integer"))
}

fn minimal_info() -> Vec<u8> {
    let mut info = b"d6:lengthi1e4:name1:x12:piece lengthi1e6:pieces20:".to_vec();
    info.extend_from_slice(&[0; 20]);
    info.push(b'e');
    info
}

fn write_size_outer<W: Write>(writer: &mut W, target_length: usize) -> io::Result<()> {
    let info = minimal_info();
    let fixed_length = b"d7:comment".len() + b":4:info".len() + info.len() + 1;
    let mut payload_length = target_length
        .checked_sub(fixed_length + target_length.to_string().len())
        .expect("target length is large enough");
    loop {
        let actual = fixed_length + payload_length.to_string().len() + payload_length;
        if actual == target_length {
            break;
        }
        payload_length = payload_length
            .checked_add_signed(target_length as isize - actual as isize)
            .expect("target fixture length");
    }
    write!(writer, "d7:comment{payload_length}:")?;
    write_repeated(writer, b'x', payload_length)?;
    writer.write_all(b"4:info")?;
    writer.write_all(&info)?;
    writer.write_all(b"e")
}

fn write_structure_outer<W: Write>(writer: &mut W, integers: usize) -> io::Result<()> {
    writer.write_all(b"d1:al")?;
    for _ in 0..integers {
        writer.write_all(b"i0e")?;
    }
    writer.write_all(b"e4:info")?;
    writer.write_all(&minimal_info())?;
    writer.write_all(b"e")
}

fn write_structure_info<W: Write>(writer: &mut W, integers: usize) -> io::Result<()> {
    writer.write_all(b"d1:al")?;
    for _ in 0..integers {
        writer.write_all(b"i0e")?;
    }
    writer.write_all(b"e6:lengthi1e4:name1:x12:piece lengthi1e6:pieces20:")?;
    writer.write_all(&[0; 20])?;
    writer.write_all(b"e")
}

fn write_many_files_outer<W: Write>(writer: &mut W, files: usize) -> io::Result<()> {
    writer.write_all(b"d4:info")?;
    write_many_files_info(writer, files)?;
    writer.write_all(b"e")
}

fn write_many_files_info<W: Write>(writer: &mut W, files: usize) -> io::Result<()> {
    let width = files.saturating_sub(1).to_string().len().max(1);
    writer.write_all(b"d5:filesl")?;
    for index in 0..files {
        let path = format!("f{index:0width$}");
        write!(writer, "d6:lengthi1e4:pathl{}:{}ee", path.len(), path)?;
    }
    write!(
        writer,
        "e4:name4:root12:piece lengthi{}e6:pieces20:",
        files.max(1)
    )?;
    writer.write_all(&[0; 20])?;
    writer.write_all(b"e")
}

fn write_many_trackers_outer<W: Write>(writer: &mut W, trackers: usize) -> io::Result<()> {
    writer.write_all(b"d13:announce-listl")?;
    for index in 0..trackers {
        let url = format!("udp://127.0.0.1:1/{index}");
        write!(writer, "l{}:{}e", url.len(), url)?;
    }
    writer.write_all(b"e4:info")?;
    writer.write_all(&minimal_info())?;
    writer.write_all(b"e")
}

fn write_many_pieces_outer<W: Write>(writer: &mut W, pieces: usize) -> io::Result<()> {
    writer.write_all(b"d4:info")?;
    write_many_pieces_info(writer, pieces)?;
    writer.write_all(b"e")
}

fn write_many_pieces_info<W: Write>(writer: &mut W, pieces: usize) -> io::Result<()> {
    write!(
        writer,
        "d6:lengthi{pieces}e4:name1:x12:piece lengthi1e6:pieces{}:",
        pieces * 20
    )?;
    write_repeated(writer, 0, pieces * 20)?;
    writer.write_all(b"e")
}

fn write_piece_length_outer<W: Write>(writer: &mut W, piece_length: usize) -> io::Result<()> {
    writer.write_all(b"d4:info")?;
    write_piece_length_info(writer, piece_length)?;
    writer.write_all(b"e")
}

fn write_piece_length_info<W: Write>(writer: &mut W, piece_length: usize) -> io::Result<()> {
    write!(
        writer,
        "d6:lengthi{piece_length}e4:name1:x12:piece lengthi{piece_length}e6:pieces20:"
    )?;
    writer.write_all(&[0; 20])?;
    writer.write_all(b"e")
}

fn write_tracker_url_outer<W: Write>(writer: &mut W, url_length: usize) -> io::Result<()> {
    const PREFIX: &[u8] = b"udp://127.0.0.1:1/";
    assert!(url_length >= PREFIX.len());
    write!(writer, "d8:announce{url_length}:")?;
    writer.write_all(PREFIX)?;
    write_repeated(writer, b'a', url_length - PREFIX.len())?;
    writer.write_all(b"4:info")?;
    writer.write_all(&minimal_info())?;
    writer.write_all(b"e")
}

fn write_long_path_outer<W: Write>(
    writer: &mut W,
    components: usize,
    component_length: usize,
) -> io::Result<()> {
    writer.write_all(b"d4:infod5:filesld6:lengthi1e4:pathl")?;
    for index in 0..components {
        let fill = b'a' + u8::try_from(index % 26).expect("bounded letter");
        write!(writer, "{component_length}:")?;
        write_repeated(writer, fill, component_length)?;
    }
    writer.write_all(b"eee4:name4:root12:piece lengthi1e6:pieces20:")?;
    writer.write_all(&[0; 20])?;
    writer.write_all(b"ee")
}

fn write_deep_outer<W: Write>(writer: &mut W, nested_lists: usize) -> io::Result<()> {
    writer.write_all(b"d1:a")?;
    write_repeated(writer, b'l', nested_lists)?;
    writer.write_all(b"i0e")?;
    write_repeated(writer, b'e', nested_lists)?;
    writer.write_all(b"4:info")?;
    writer.write_all(&minimal_info())?;
    writer.write_all(b"e")
}

fn write_invalid_utf8_path_outer<W: Write>(writer: &mut W) -> io::Result<()> {
    writer.write_all(b"d4:infod5:filesld6:lengthi1e4:pathl1:")?;
    writer.write_all(&[0xff])?;
    writer.write_all(b"eee4:name4:root12:piece lengthi1e6:pieces20:")?;
    writer.write_all(&[0; 20])?;
    writer.write_all(b"ee")
}

fn write_repeated<W: Write>(writer: &mut W, byte: u8, length: usize) -> io::Result<()> {
    let buffer = [byte; 16 * 1024];
    let mut remaining = length;
    while remaining != 0 {
        let chunk = remaining.min(buffer.len());
        writer.write_all(&buffer[..chunk])?;
        remaining -= chunk;
    }
    Ok(())
}

fn count_lexical_tokens(bytes: &[u8]) -> Option<usize> {
    fn value(bytes: &[u8], position: &mut usize, tokens: &mut usize) -> Option<()> {
        let token = *bytes.get(*position)?;
        *tokens = tokens.checked_add(1)?;
        match token {
            b'i' => {
                *position += 1;
                while *bytes.get(*position)? != b'e' {
                    *position += 1;
                }
                *position += 1;
            }
            b'l' => {
                *position += 1;
                while *bytes.get(*position)? != b'e' {
                    value(bytes, position, tokens)?;
                }
                *tokens = tokens.checked_add(1)?;
                *position += 1;
            }
            b'd' => {
                *position += 1;
                while *bytes.get(*position)? != b'e' {
                    value(bytes, position, tokens)?;
                    value(bytes, position, tokens)?;
                }
                *tokens = tokens.checked_add(1)?;
                *position += 1;
            }
            b'0'..=b'9' => {
                let mut length = 0_usize;
                while *bytes.get(*position)? != b':' {
                    let digit = bytes.get(*position)?.checked_sub(b'0')?;
                    if digit > 9 {
                        return None;
                    }
                    length = length.checked_mul(10)?.checked_add(usize::from(digit))?;
                    *position += 1;
                }
                *position = position.checked_add(1 + length)?;
                if *position > bytes.len() {
                    return None;
                }
            }
            _ => return None,
        }
        Some(())
    }

    let mut position = 0;
    let mut tokens = 0;
    value(bytes, &mut position, &mut tokens)?;
    (position == bytes.len()).then_some(tokens)
}
