//! Explicit runtime-free parsing for v1, v2, and hybrid metainfo.

use std::collections::BTreeMap;
use std::ops::Range;

use sha1::{Digest, Sha1};
use sha2::Sha256;

use super::direct::{
    self, Parser, check_dictionary_key, project_component, project_raw_path,
    resolve_projected_path_collisions,
};
use super::{Metainfo, MetainfoError, MetainfoLimits, MetainfoMode, MetainfoTracker};
use crate::identity::{InfoHashes, V1InfoHash, V2InfoHash};
use crate::merkle::{
    MAX_BEP52_PIECE_LENGTH, MIN_BEP52_PIECE_LENGTH, Sha256Hash, file_root_from_piece_hashes,
};
use crate::v2_layout::V2TorrentLayout;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MetainfoFormat {
    V1,
    V2,
    Hybrid,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct V2File {
    pub raw_path: Vec<Vec<u8>>,
    pub path: Vec<String>,
    pub length: u64,
    pub pieces_root: Option<Sha256Hash>,
    pub hidden: bool,
    pub executable: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct V2Metainfo {
    pub raw_name: Vec<u8>,
    pub name: String,
    pub private: bool,
    pub piece_length: u32,
    pub total_length: u64,
    pub files: Vec<V2File>,
    pub layout: V2TorrentLayout,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HybridTailPadding {
    Absent,
    Present { length: u64 },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HybridMetainfo {
    pub v1: Metainfo,
    pub v2: V2Metainfo,
    pub tail_padding: HybridTailPadding,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ParsedInfoKind {
    V1(Metainfo),
    V2(V2Metainfo),
    Hybrid(HybridMetainfo),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParsedInfo<'a> {
    exact_info_bytes: &'a [u8],
    info_hashes: InfoHashes,
    kind: ParsedInfoKind,
}

impl<'a> ParsedInfo<'a> {
    pub fn from_bytes_with_limits(
        bytes: &'a [u8],
        limits: MetainfoLimits,
    ) -> Result<Self, MetainfoError> {
        parse_info(bytes, limits)
    }

    pub const fn exact_info_bytes(&self) -> &'a [u8] {
        self.exact_info_bytes
    }

    pub const fn info_hashes(&self) -> InfoHashes {
        self.info_hashes
    }

    pub const fn kind(&self) -> &ParsedInfoKind {
        &self.kind
    }

    pub const fn format(&self) -> MetainfoFormat {
        match self.kind {
            ParsedInfoKind::V1(_) => MetainfoFormat::V1,
            ParsedInfoKind::V2(_) => MetainfoFormat::V2,
            ParsedInfoKind::Hybrid(_) => MetainfoFormat::Hybrid,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PieceLayerEntry {
    pub pieces_root: Sha256Hash,
    pub hashes: Range<usize>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CompletePieceLayers {
    hashes: Vec<Sha256Hash>,
    entries: Vec<PieceLayerEntry>,
}

impl CompletePieceLayers {
    pub fn hashes(&self) -> &[Sha256Hash] {
        &self.hashes
    }

    pub fn entries(&self) -> &[PieceLayerEntry] {
        &self.entries
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ParsedOuterMetainfo<'a> {
    V1 {
        info: ParsedInfo<'a>,
        trackers: Vec<MetainfoTracker>,
    },
    V2 {
        info: ParsedInfo<'a>,
        piece_layers: CompletePieceLayers,
        trackers: Vec<MetainfoTracker>,
    },
    Hybrid {
        info: ParsedInfo<'a>,
        piece_layers: CompletePieceLayers,
        trackers: Vec<MetainfoTracker>,
    },
}

impl<'a> ParsedOuterMetainfo<'a> {
    pub fn from_bytes_with_limits(
        bytes: &'a [u8],
        limits: MetainfoLimits,
    ) -> Result<Self, MetainfoError> {
        let outer = direct::scan_outer(bytes, limits)?;
        let info = parse_info(&bytes[outer.info_span], limits)?;
        match info.format() {
            MetainfoFormat::V1 => Ok(Self::V1 {
                info,
                trackers: outer.trackers,
            }),
            MetainfoFormat::V2 | MetainfoFormat::Hybrid => {
                let layer_span = outer
                    .piece_layers_span
                    .ok_or(MetainfoError::MissingPieceLayers)?;
                let layers = parse_piece_layers(&bytes[layer_span], &info, limits)?;
                if info.format() == MetainfoFormat::V2 {
                    Ok(Self::V2 {
                        info,
                        piece_layers: layers,
                        trackers: outer.trackers,
                    })
                } else {
                    Ok(Self::Hybrid {
                        info,
                        piece_layers: layers,
                        trackers: outer.trackers,
                    })
                }
            }
        }
    }

    pub const fn info(&self) -> &ParsedInfo<'a> {
        match self {
            Self::V1 { info, .. } | Self::V2 { info, .. } | Self::Hybrid { info, .. } => info,
        }
    }

    pub fn trackers(&self) -> &[MetainfoTracker] {
        match self {
            Self::V1 { trackers, .. }
            | Self::V2 { trackers, .. }
            | Self::Hybrid { trackers, .. } => trackers,
        }
    }

    pub const fn piece_layers(&self) -> Option<&CompletePieceLayers> {
        match self {
            Self::V1 { .. } => None,
            Self::V2 { piece_layers, .. } | Self::Hybrid { piece_layers, .. } => Some(piece_layers),
        }
    }
}

fn parse_info<'a>(
    bytes: &'a [u8],
    limits: MetainfoLimits,
) -> Result<ParsedInfo<'a>, MetainfoError> {
    direct::enforce_info_length(bytes.len(), limits)?;
    let mut scanner = Parser::new(bytes, limits.max_info_bytes, limits)?;
    let scan = direct::scan_info_dictionary(&mut scanner, 0)?;
    scanner.finish()?;

    if scan.invalid_meta_version {
        return Err(MetainfoError::InvalidField("info.meta version"));
    }
    match scan.meta_version {
        Some(version) if version > 2 => {
            return Err(MetainfoError::UnsupportedVersion { version });
        }
        Some(2) => {}
        Some(_) => return Err(MetainfoError::InvalidField("info.meta version")),
        None if scan.has_file_tree => {
            return Err(MetainfoError::InvalidField("info.meta version"));
        }
        None => {
            let v1 = direct::parse_info(bytes, limits)?;
            return Ok(ParsedInfo {
                exact_info_bytes: bytes,
                info_hashes: InfoHashes::v1(V1InfoHash::new(v1.info_hash)),
                kind: ParsedInfoKind::V1(v1),
            });
        }
    }

    let top = parse_v2_top(bytes, limits)?;
    let v2 = parse_v2(top, limits)?;
    let v2_hash = V2InfoHash::new(Sha256::digest(bytes).into());
    if !top.has_v1_fields {
        return Ok(ParsedInfo {
            exact_info_bytes: bytes,
            info_hashes: InfoHashes::v2(v2_hash),
            kind: ParsedInfoKind::V2(v2),
        });
    }

    let v1 = direct::parse_v1_semantics(bytes, limits)?;
    let raw_paths = parse_v1_raw_paths(bytes, &v1, limits)?;
    let tail_padding = validate_hybrid(&v1, &raw_paths, &v2)?;
    let v1_hash = V1InfoHash::new(Sha1::digest(bytes).into());
    Ok(ParsedInfo {
        exact_info_bytes: bytes,
        info_hashes: InfoHashes::hybrid(v1_hash, v2_hash),
        kind: ParsedInfoKind::Hybrid(HybridMetainfo {
            v1,
            v2,
            tail_padding,
        }),
    })
}

#[derive(Clone, Copy)]
struct V2Top<'a> {
    file_tree: &'a [u8],
    name: &'a [u8],
    piece_length: u32,
    private: bool,
    has_v1_fields: bool,
}

fn parse_v2_top<'a>(bytes: &'a [u8], limits: MetainfoLimits) -> Result<V2Top<'a>, MetainfoError> {
    let mut parser = Parser::new(bytes, limits.max_info_bytes, limits)?;
    if parser.peek()? != b'd' {
        return Err(MetainfoError::InvalidField("info dictionary"));
    }
    parser.enter_container(b'd', 0)?;
    let mut previous_key = None;
    let mut entries = 0_usize;
    let mut file_tree = None;
    let mut name = None;
    let mut piece_length = None;
    let mut private = false;
    let mut has_v1_fields = false;
    while parser.peek()? != b'e' {
        parser.check_collection(entries, parser.position)?;
        let key_position = parser.position;
        let key = parser.parse_bytes(1)?;
        check_dictionary_key(previous_key, key, key_position)?;
        previous_key = Some(key);
        entries += 1;
        match key {
            b"file tree" => {
                let start = parser.position;
                parser.skip_value(1)?;
                file_tree = Some(&bytes[start..parser.position]);
            }
            b"files" | b"length" | b"pieces" => {
                has_v1_fields = true;
                parser.skip_value(1)?;
            }
            b"meta version" => {
                if parser.parse_integer(1)? != 2 {
                    return Err(MetainfoError::InvalidField("info.meta version"));
                }
            }
            b"name" => name = Some(parse_bytes(&mut parser, 1, "info.name")?),
            b"piece length" => {
                let value = parse_nonnegative(&mut parser, 1, "info.piece length")?;
                piece_length = Some(
                    u32::try_from(value)
                        .map_err(|_| MetainfoError::InvalidField("info.piece length"))?,
                );
            }
            b"private" => {
                private = match parser
                    .parse_integer(1)
                    .map_err(|_| MetainfoError::InvalidField("info.private"))?
                {
                    0 => false,
                    1 => true,
                    _ => return Err(MetainfoError::InvalidField("info.private")),
                };
            }
            _ => parser.skip_value(1)?,
        }
    }
    parser.leave_container()?;
    parser.finish()?;

    let piece_length = piece_length.ok_or(MetainfoError::MissingField("info.piece length"))?;
    if !(MIN_BEP52_PIECE_LENGTH..=MAX_BEP52_PIECE_LENGTH).contains(&piece_length)
        || !piece_length.is_power_of_two()
    {
        return Err(MetainfoError::InvalidField("info.piece length"));
    }
    Ok(V2Top {
        file_tree: file_tree.ok_or(MetainfoError::MissingField("info.file tree"))?,
        name: name.ok_or(MetainfoError::MissingField("info.name"))?,
        piece_length,
        private,
        has_v1_fields,
    })
}

fn parse_v2(top: V2Top<'_>, limits: MetainfoLimits) -> Result<V2Metainfo, MetainfoError> {
    if top.name.len() > limits.max_path_component_bytes {
        return Err(MetainfoError::UnsafePath {
            file: None,
            reason: "component is too long",
        });
    }
    let mut files = parse_file_tree(top.file_tree, limits)?;
    let mut projected: Vec<Vec<String>> = files
        .iter_mut()
        .map(|file| std::mem::take(&mut file.path))
        .collect();
    resolve_projected_path_collisions(&mut projected);
    for (file, path) in files.iter_mut().zip(projected) {
        file.path = path;
    }
    let lengths: Vec<u64> = files.iter().map(|file| file.length).collect();
    let layout =
        V2TorrentLayout::new_with_piece_limit(top.piece_length, &lengths, limits.max_pieces)?;
    Ok(V2Metainfo {
        raw_name: top.name.to_vec(),
        name: project_component(top.name),
        private: top.private,
        piece_length: top.piece_length,
        total_length: layout.payload_length(),
        files,
        layout,
    })
}

struct TreeFrame<'a> {
    previous_key: Option<&'a [u8]>,
    entries: usize,
    saw_leaf: bool,
    is_root: bool,
}

fn parse_file_tree(bytes: &[u8], limits: MetainfoLimits) -> Result<Vec<V2File>, MetainfoError> {
    let mut parser = Parser::new(bytes, limits.max_info_bytes, limits)?;
    if parser.peek()? != b'd' {
        return Err(MetainfoError::InvalidField("info.file tree"));
    }
    parser.enter_container(b'd', 0)?;
    let mut frames = vec![TreeFrame {
        previous_key: None,
        entries: 0,
        saw_leaf: false,
        is_root: true,
    }];
    let mut raw_path: Vec<Vec<u8>> = Vec::new();
    let mut files = Vec::new();
    let mut retained_components = 0_usize;

    while !frames.is_empty() {
        if parser.peek()? == b'e' {
            parser.leave_container()?;
            let frame = frames.pop().expect("nonempty traversal stack");
            if !frame.is_root && frame.entries == 0 {
                return Err(MetainfoError::InvalidField("info.file tree empty branch"));
            }
            if !frame.is_root {
                raw_path.pop();
            }
            continue;
        }

        let frame_index = frames.len() - 1;
        let depth = frames.len();
        let key_position = parser.position;
        parser.check_collection(frames[frame_index].entries, key_position)?;
        let key = parser.parse_bytes(depth)?;
        check_dictionary_key(frames[frame_index].previous_key, key, key_position)?;
        frames[frame_index].previous_key = Some(key);
        frames[frame_index].entries += 1;

        if key.is_empty() {
            if frames[frame_index].is_root || frames[frame_index].entries != 1 {
                return Err(MetainfoError::InvalidField("info.file tree leaf position"));
            }
            frames[frame_index].saw_leaf = true;
            if files.len() == limits.max_files {
                return Err(MetainfoError::TooManyFiles {
                    actual: files.len() + 1,
                    maximum: limits.max_files,
                });
            }
            retained_components = retained_components
                .checked_add(raw_path.len())
                .ok_or(MetainfoError::TotalLengthOverflow)?;
            if retained_components > limits.max_path_components {
                return Err(MetainfoError::UnsafePath {
                    file: Some(files.len()),
                    reason: "paths have too many total components",
                });
            }
            let leaf = parse_file_tree_leaf(&mut parser, depth, files.len())?;
            if parser.peek()? != b'e' {
                return Err(MetainfoError::InvalidField(
                    "info.file tree branch/leaf conflict",
                ));
            }
            let path = project_raw_path(raw_path.iter().map(Vec::as_slice), files.len(), limits)?;
            files.push(V2File {
                raw_path: raw_path.clone(),
                path,
                length: leaf.length,
                pieces_root: leaf.pieces_root,
                hidden: leaf.hidden,
                executable: leaf.executable,
            });
            continue;
        }

        if frames[frame_index].saw_leaf {
            return Err(MetainfoError::InvalidField(
                "info.file tree branch/leaf conflict",
            ));
        }
        if key.len() > limits.max_path_component_bytes {
            return Err(MetainfoError::UnsafePath {
                file: Some(files.len()),
                reason: "component is too long",
            });
        }
        raw_path.push(key.to_vec());
        if parser.peek()? != b'd' {
            return Err(MetainfoError::InvalidField("info.file tree node"));
        }
        parser.enter_container(b'd', depth)?;
        frames.push(TreeFrame {
            previous_key: None,
            entries: 0,
            saw_leaf: false,
            is_root: false,
        });
    }
    parser.finish()?;
    if files.is_empty() {
        return Err(MetainfoError::InvalidField("info.file tree"));
    }
    Ok(files)
}

struct V2Leaf {
    length: u64,
    pieces_root: Option<Sha256Hash>,
    hidden: bool,
    executable: bool,
}

fn parse_file_tree_leaf(
    parser: &mut Parser<'_>,
    depth: usize,
    file: usize,
) -> Result<V2Leaf, MetainfoError> {
    if parser.peek()? != b'd' {
        return Err(MetainfoError::InvalidField("info.file tree leaf"));
    }
    parser.enter_container(b'd', depth)?;
    let mut previous_key = None;
    let mut entries = 0_usize;
    let mut length = None;
    let mut pieces_root = None;
    let mut hidden = false;
    let mut executable = false;
    while parser.peek()? != b'e' {
        parser.check_collection(entries, parser.position)?;
        let key_position = parser.position;
        let key = parser.parse_bytes(depth + 1)?;
        check_dictionary_key(previous_key, key, key_position)?;
        previous_key = Some(key);
        entries += 1;
        match key {
            b"attr" => {
                let attr = parse_bytes(parser, depth + 1, "info.file tree.attr")?;
                if attr.contains(&b'l') || attr.contains(&b'p') {
                    return Err(MetainfoError::Unsupported("v2 symlink or padding file"));
                }
                hidden = attr.contains(&b'h');
                executable = attr.contains(&b'x');
            }
            b"length" => {
                length = Some(parse_nonnegative(
                    parser,
                    depth + 1,
                    "info.file tree.length",
                )?)
            }
            b"pieces root" => {
                let root = parse_bytes(parser, depth + 1, "info.file tree.pieces root")?;
                pieces_root = Some(root.try_into().map_err(|_| {
                    MetainfoError::InvalidField("info.file tree.pieces root length")
                })?);
            }
            _ => parser.skip_value(depth + 1)?,
        }
    }
    parser.leave_container()?;
    let length = length.ok_or(MetainfoError::MissingField("info.file tree.length"))?;
    match (length, pieces_root) {
        (0, Some(_)) => Err(MetainfoError::PieceLayer {
            file: Some(file),
            reason: "empty file has a pieces root",
        }),
        (0, None) => Ok(V2Leaf {
            length,
            pieces_root,
            hidden,
            executable,
        }),
        (_, None) => Err(MetainfoError::PieceLayer {
            file: Some(file),
            reason: "nonempty file is missing pieces root",
        }),
        (_, Some(_)) => Ok(V2Leaf {
            length,
            pieces_root,
            hidden,
            executable,
        }),
    }
}

fn parse_bytes<'a>(
    parser: &mut Parser<'a>,
    depth: usize,
    field: &'static str,
) -> Result<&'a [u8], MetainfoError> {
    if !parser.peek()?.is_ascii_digit() {
        return Err(MetainfoError::InvalidField(field));
    }
    parser.parse_bytes(depth).map_err(Into::into)
}

fn parse_nonnegative(
    parser: &mut Parser<'_>,
    depth: usize,
    field: &'static str,
) -> Result<u64, MetainfoError> {
    let value = parser
        .parse_integer(depth)
        .map_err(|_| MetainfoError::InvalidField(field))?;
    u64::try_from(value).map_err(|_| MetainfoError::InvalidField(field))
}

fn parse_v1_raw_paths(
    bytes: &[u8],
    v1: &Metainfo,
    limits: MetainfoLimits,
) -> Result<Vec<Option<Vec<Vec<u8>>>>, MetainfoError> {
    let mut parser = Parser::new(bytes, limits.max_info_bytes, limits)?;
    parser.enter_container(b'd', 0)?;
    let mut previous_key = None;
    let mut entries = 0_usize;
    let mut name = None;
    let mut paths = None;
    while parser.peek()? != b'e' {
        parser.check_collection(entries, parser.position)?;
        let position = parser.position;
        let key = parser.parse_bytes(1)?;
        check_dictionary_key(previous_key, key, position)?;
        previous_key = Some(key);
        entries += 1;
        match key {
            b"files" => paths = Some(parse_v1_file_paths(&mut parser, 1)?),
            b"name" => name = Some(parse_bytes(&mut parser, 1, "info.name")?.to_vec()),
            _ => parser.skip_value(1)?,
        }
    }
    parser.leave_container()?;
    parser.finish()?;
    let raw = match v1.mode {
        MetainfoMode::SingleFile => vec![Some(vec![
            name.ok_or(MetainfoError::MissingField("info.name"))?,
        ])],
        MetainfoMode::MultiFile => paths.ok_or(MetainfoError::MissingField("info.files"))?,
    };
    if raw.len() != v1.files.len() {
        return Err(MetainfoError::HybridMismatch {
            file: None,
            category: "v1 file count",
        });
    }
    Ok(raw)
}

fn parse_v1_file_paths(
    parser: &mut Parser<'_>,
    depth: usize,
) -> Result<Vec<Option<Vec<Vec<u8>>>>, MetainfoError> {
    if parser.peek()? != b'l' {
        return Err(MetainfoError::InvalidField("info.files"));
    }
    parser.enter_container(b'l', depth)?;
    let mut paths = Vec::new();
    while parser.peek()? != b'e' {
        parser.enter_container(b'd', depth + 1)?;
        let mut previous_key = None;
        let mut entries = 0_usize;
        let mut path = None;
        while parser.peek()? != b'e' {
            parser.check_collection(entries, parser.position)?;
            let position = parser.position;
            let key = parser.parse_bytes(depth + 2)?;
            check_dictionary_key(previous_key, key, position)?;
            previous_key = Some(key);
            entries += 1;
            if key == b"path" {
                parser.enter_container(b'l', depth + 2)?;
                let mut components = Vec::new();
                while parser.peek()? != b'e' {
                    components.push(
                        parse_bytes(parser, depth + 3, "info.files.path component")?.to_vec(),
                    );
                }
                parser.leave_container()?;
                path = Some(components);
            } else {
                parser.skip_value(depth + 2)?;
            }
        }
        parser.leave_container()?;
        paths.push(path);
    }
    parser.leave_container()?;
    Ok(paths)
}

fn validate_hybrid(
    v1: &Metainfo,
    raw_paths: &[Option<Vec<Vec<u8>>>],
    v2: &V2Metainfo,
) -> Result<HybridTailPadding, MetainfoError> {
    if v1.piece_length != v2.piece_length {
        return Err(MetainfoError::HybridMismatch {
            file: None,
            category: "piece length",
        });
    }
    let mut v1_index = 0_usize;
    for (file_index, (v2_file, geometry)) in v2.files.iter().zip(v2.layout.files()).enumerate() {
        if geometry.alignment_gap_before() != 0 {
            let padding = v1
                .files
                .get(v1_index)
                .ok_or(MetainfoError::HybridMismatch {
                    file: Some(file_index),
                    category: "missing internal padding",
                })?;
            if !padding.padding
                || padding.offset != geometry.logical_offset() - geometry.alignment_gap_before()
                || padding.length != geometry.alignment_gap_before()
            {
                return Err(MetainfoError::HybridMismatch {
                    file: Some(file_index),
                    category: "internal padding",
                });
            }
            v1_index += 1;
        }
        let v1_file = v1
            .files
            .get(v1_index)
            .ok_or(MetainfoError::HybridMismatch {
                file: Some(file_index),
                category: "missing payload file",
            })?;
        if v1_file.padding {
            return Err(MetainfoError::HybridMismatch {
                file: Some(file_index),
                category: "extra padding",
            });
        }
        if v1_file.length != v2_file.length {
            return Err(MetainfoError::HybridMismatch {
                file: Some(file_index),
                category: "file length",
            });
        }
        if v1_file.offset != geometry.logical_offset() {
            return Err(MetainfoError::HybridMismatch {
                file: Some(file_index),
                category: "file offset",
            });
        }
        if raw_paths.get(v1_index).and_then(Option::as_ref) != Some(&v2_file.raw_path) {
            return Err(MetainfoError::HybridMismatch {
                file: Some(file_index),
                category: "raw path",
            });
        }
        v1_index += 1;
    }

    match v1.files.len().saturating_sub(v1_index) {
        0 => Ok(HybridTailPadding::Absent),
        1 => {
            let padding = &v1.files[v1_index];
            let piece_length = u64::from(v2.piece_length);
            let remainder = v2.layout.logical_length() % piece_length;
            let expected = if remainder == 0 {
                0
            } else {
                piece_length - remainder
            };
            if expected == 0
                || !padding.padding
                || padding.offset != v2.layout.logical_length()
                || padding.length != expected
            {
                return Err(MetainfoError::HybridMismatch {
                    file: None,
                    category: "final tail padding",
                });
            }
            Ok(HybridTailPadding::Present { length: expected })
        }
        _ => Err(MetainfoError::HybridMismatch {
            file: None,
            category: "extra trailing files",
        }),
    }
}

fn parse_piece_layers(
    bytes: &[u8],
    info: &ParsedInfo<'_>,
    limits: MetainfoLimits,
) -> Result<CompletePieceLayers, MetainfoError> {
    let v2 = match info.kind() {
        ParsedInfoKind::V1(_) => {
            return Err(MetainfoError::PieceLayer {
                file: None,
                reason: "v1 metainfo has no v2 piece layers",
            });
        }
        ParsedInfoKind::V2(v2) => v2,
        ParsedInfoKind::Hybrid(hybrid) => &hybrid.v2,
    };
    let mut required = BTreeMap::<Sha256Hash, (usize, usize)>::new();
    for (file_index, (file, geometry)) in v2.files.iter().zip(v2.layout.files()).enumerate() {
        if geometry.piece_count() <= 1 {
            continue;
        }
        let root = file.pieces_root.expect("nonempty v2 file has a root");
        let expected = usize::try_from(geometry.piece_count())
            .map_err(|_| MetainfoError::TotalLengthOverflow)?;
        if let Some((previous, _)) = required.insert(root, (expected, file_index))
            && previous != expected
        {
            return Err(MetainfoError::PieceLayer {
                file: Some(file_index),
                reason: "shared root requires conflicting hash counts",
            });
        }
    }

    let mut parser = Parser::new(bytes, limits.max_outer_bytes, limits)?;
    if parser.peek()? != b'd' {
        return Err(MetainfoError::InvalidField("piece layers"));
    }
    parser.enter_container(b'd', 0)?;
    let mut previous_key = None;
    let mut source_entries = 0_usize;
    let mut hashes = Vec::new();
    let mut entries = Vec::new();
    while parser.peek()? != b'e' {
        parser.check_collection(source_entries, parser.position)?;
        let key_position = parser.position;
        let key = parser.parse_bytes(1)?;
        check_dictionary_key(previous_key, key, key_position)?;
        previous_key = Some(key);
        source_entries += 1;
        let root: Sha256Hash = key
            .try_into()
            .map_err(|_| MetainfoError::InvalidField("piece layers root length"))?;
        let (expected, file_index) =
            required
                .get(&root)
                .copied()
                .ok_or(MetainfoError::PieceLayer {
                    file: None,
                    reason: "entry does not name a required multi-piece file root",
                })?;
        let encoded = parse_bytes(&mut parser, 1, "piece layers hashes")?;
        if encoded.len() % 32 != 0 {
            return Err(MetainfoError::PieceLayer {
                file: Some(file_index),
                reason: "hash byte string is not divisible by 32",
            });
        }
        let actual = encoded.len() / 32;
        if actual != expected {
            return Err(MetainfoError::PieceLayer {
                file: Some(file_index),
                reason: "hash count does not match file piece count",
            });
        }
        let next =
            hashes
                .len()
                .checked_add(actual)
                .ok_or(MetainfoError::TooManyPieceLayerHashes {
                    actual: usize::MAX,
                    maximum: limits.max_pieces,
                })?;
        if next > limits.max_pieces {
            return Err(MetainfoError::TooManyPieceLayerHashes {
                actual: next,
                maximum: limits.max_pieces,
            });
        }
        let start = hashes.len();
        hashes.extend(encoded.chunks_exact(32).map(|hash| {
            <[u8; 32]>::try_from(hash).expect("piece-layer chunk is exactly 32 bytes")
        }));
        if file_root_from_piece_hashes(hashes[start..next].iter().copied(), v2.piece_length)?
            != root
        {
            return Err(MetainfoError::PieceLayer {
                file: Some(file_index),
                reason: "piece hashes do not reconstruct the file root",
            });
        }
        entries.push(PieceLayerEntry {
            pieces_root: root,
            hashes: start..next,
        });
    }
    parser.leave_container()?;
    parser.finish()?;
    if entries.len() != required.len() {
        return Err(MetainfoError::PieceLayer {
            file: None,
            reason: "required piece layer is missing",
        });
    }
    Ok(CompletePieceLayers { hashes, entries })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::merkle::file_root_from_piece_hashes;
    use crate::metainfo::{BEP9_METAINFO_LIMITS, EXPLICIT_IMPORT_METAINFO_LIMITS};

    fn bytes(output: &mut Vec<u8>, value: &[u8]) {
        output.extend_from_slice(value.len().to_string().as_bytes());
        output.push(b':');
        output.extend_from_slice(value);
    }

    fn leaf(output: &mut Vec<u8>, name: &[u8], length: u64, root: Option<Sha256Hash>) {
        bytes(output, name);
        output.push(b'd');
        bytes(output, b"");
        output.push(b'd');
        bytes(output, b"length");
        output.extend_from_slice(format!("i{length}e").as_bytes());
        if let Some(root) = root {
            bytes(output, b"pieces root");
            bytes(output, &root);
        }
        output.extend_from_slice(b"ee");
    }

    fn file_tree(files: &[(&[u8], u64, Option<Sha256Hash>)]) -> Vec<u8> {
        let mut output = vec![b'd'];
        for &(name, length, root) in files {
            leaf(&mut output, name, length, root);
        }
        output.push(b'e');
        output
    }

    fn v2_info(tree: &[u8], piece_length: u32) -> Vec<u8> {
        let mut output = b"d9:file tree".to_vec();
        output.extend_from_slice(tree);
        output.extend_from_slice(
            format!("12:meta versioni2e4:name4:root12:piece lengthi{piece_length}ee").as_bytes(),
        );
        output
    }

    fn hybrid_info(tree: &[u8], v1_files: &[u8], piece_length: u32, piece_count: usize) -> Vec<u8> {
        let mut output = b"d9:file tree".to_vec();
        output.extend_from_slice(tree);
        output.extend_from_slice(b"5:filesl");
        output.extend_from_slice(v1_files);
        output.extend_from_slice(
            format!(
                "e12:meta versioni2e4:name4:root12:piece lengthi{piece_length}e6:pieces{}:",
                piece_count * 20
            )
            .as_bytes(),
        );
        output.resize(output.len() + piece_count * 20, 7);
        output.push(b'e');
        output
    }

    fn outer(info: &[u8], layers: Option<&[u8]>) -> Vec<u8> {
        let mut output = b"d4:info".to_vec();
        output.extend_from_slice(info);
        if let Some(layers) = layers {
            output.extend_from_slice(b"12:piece layers");
            output.extend_from_slice(layers);
        }
        output.push(b'e');
        output
    }

    fn layer_dictionary(entries: &[(Sha256Hash, Vec<Sha256Hash>)]) -> Vec<u8> {
        let mut output = vec![b'd'];
        for (root, hashes) in entries {
            bytes(&mut output, root);
            let flattened: Vec<u8> = hashes.iter().flatten().copied().collect();
            bytes(&mut output, &flattened);
        }
        output.push(b'e');
        output
    }

    #[test]
    fn parses_pure_v2_and_hashes_the_exact_non_utf8_info_bytes() {
        let root = [0_u8; 32];
        let tree = file_tree(&[(&[0xff], 1, Some(root))]);
        let mut info = v2_info(&tree, 16 * 1024);
        let name = info
            .windows(b"4:name4:root".len())
            .position(|window| window == b"4:name4:root")
            .expect("name field");
        info.splice(
            name..name + b"4:name4:root".len(),
            b"4:name1:\xff".iter().copied(),
        );
        let expected: [u8; 32] = Sha256::digest(&info).into();

        let parsed = ParsedInfo::from_bytes_with_limits(&info, BEP9_METAINFO_LIMITS)
            .expect("valid pure v2 info");
        assert_eq!(parsed.format(), MetainfoFormat::V2);
        assert_eq!(parsed.exact_info_bytes(), info);
        assert_eq!(
            parsed.info_hashes().v2_hash().unwrap().into_bytes(),
            expected
        );
        assert_eq!(parsed.info_hashes().v1_hash(), None);
        let ParsedInfoKind::V2(v2) = parsed.kind() else {
            panic!("v2 variant");
        };
        assert_eq!(v2.files[0].raw_path, [vec![0xff]]);
        assert_eq!(v2.files[0].pieces_root, Some(root));
        assert_eq!(v2.layout.piece_count(), 1);
        assert!(v2.name.contains('~'));
    }

    #[test]
    fn product_parser_still_rejects_valid_v2_and_future_version_wins() {
        let info = v2_info(&file_tree(&[(b"a", 1, Some([1; 32]))]), 16 * 1024);
        assert_eq!(
            Metainfo::from_info_bytes_with_limits(&info, BEP9_METAINFO_LIMITS),
            Err(MetainfoError::Unsupported("v2 or hybrid info dictionary"))
        );

        let future = b"d9:file tree1:x12:meta versioni3e4:namei0ee";
        assert_eq!(
            ParsedInfo::from_bytes_with_limits(future, BEP9_METAINFO_LIMITS),
            Err(MetainfoError::UnsupportedVersion { version: 3 })
        );
    }

    #[test]
    fn file_tree_preserves_empty_files_and_rejects_hostile_shapes() {
        let info = v2_info(
            &file_tree(&[(b"a", 0, None), (b"b", 1, Some([2; 32]))]),
            16 * 1024,
        );
        let parsed = ParsedInfo::from_bytes_with_limits(&info, BEP9_METAINFO_LIMITS)
            .expect("empty file around payload");
        let ParsedInfoKind::V2(v2) = parsed.kind() else {
            panic!("v2 variant");
        };
        assert_eq!(v2.files.len(), 2);
        assert_eq!(v2.layout.file_piece_range(0), Ok(0..0));
        assert_eq!(v2.layout.file_piece_range(1), Ok(0..1));

        let malformed = [
            b"de".as_slice(),
            b"d0:d6:lengthi1e11:pieces root32:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaee".as_slice(),
            b"d1:ade".as_slice(),
            b"d1:ad0:d6:lengthi1eee".as_slice(),
            b"d1:ad0:d6:lengthi0e11:pieces root32:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaeee".as_slice(),
            b"d1:ad0:d4:attr1:p6:lengthi1e11:pieces root32:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaeee"
                .as_slice(),
        ];
        for tree in malformed {
            let info = v2_info(tree, 16 * 1024);
            assert!(
                ParsedInfo::from_bytes_with_limits(&info, BEP9_METAINFO_LIMITS).is_err(),
                "tree should fail: {tree:?}"
            );
        }
    }

    #[test]
    fn rejects_branch_leaf_conflicts_and_non_power_of_two_piece_lengths() {
        let mut tree = b"d1:ad0:d6:lengthi1e11:pieces root32:".to_vec();
        tree.extend_from_slice(&[1; 32]);
        tree.extend_from_slice(b"e1:bd0:d6:lengthi1e11:pieces root32:");
        tree.extend_from_slice(&[2; 32]);
        tree.extend_from_slice(b"eeee");
        assert!(
            ParsedInfo::from_bytes_with_limits(&v2_info(&tree, 16 * 1024), BEP9_METAINFO_LIMITS)
                .is_err()
        );
        assert_eq!(
            ParsedInfo::from_bytes_with_limits(
                &v2_info(&file_tree(&[(b"a", 1, Some([1; 32]))]), 24 * 1024),
                BEP9_METAINFO_LIMITS,
            ),
            Err(MetainfoError::InvalidField("info.piece length"))
        );
    }

    #[test]
    fn validates_hybrid_raw_paths_offsets_and_bounded_tail_policy() {
        let tree = file_tree(&[(b"a", 1, Some([1; 32])), (b"b", 1, Some([2; 32]))]);
        let files = concat!(
            "d6:lengthi1e4:pathl1:aee",
            "d4:attr1:p6:lengthi16383ee",
            "d6:lengthi1e4:pathl1:bee"
        );
        let info = hybrid_info(&tree, files.as_bytes(), 16 * 1024, 2);
        let parsed = ParsedInfo::from_bytes_with_limits(&info, BEP9_METAINFO_LIMITS)
            .expect("historical missing final tail pad");
        assert_eq!(parsed.format(), MetainfoFormat::Hybrid);
        assert!(parsed.info_hashes().is_hybrid());
        assert_eq!(
            parsed.info_hashes().v1_hash().unwrap().into_bytes(),
            <[u8; 20]>::from(Sha1::digest(&info))
        );
        assert_eq!(
            parsed.info_hashes().v2_hash().unwrap().into_bytes(),
            <[u8; 32]>::from(Sha256::digest(&info))
        );
        let ParsedInfoKind::Hybrid(hybrid) = parsed.kind() else {
            panic!("hybrid variant");
        };
        assert_eq!(hybrid.tail_padding, HybridTailPadding::Absent);

        let with_tail = concat!(
            "d6:lengthi1e4:pathl1:aee",
            "d4:attr1:p6:lengthi16383ee",
            "d6:lengthi1e4:pathl1:bee",
            "d4:attr1:p6:lengthi16383ee"
        );
        let info = hybrid_info(&tree, with_tail.as_bytes(), 16 * 1024, 2);
        let parsed = ParsedInfo::from_bytes_with_limits(&info, BEP9_METAINFO_LIMITS)
            .expect("canonical final tail pad");
        let ParsedInfoKind::Hybrid(hybrid) = parsed.kind() else {
            panic!("hybrid variant");
        };
        assert_eq!(
            hybrid.tail_padding,
            HybridTailPadding::Present { length: 16_383 }
        );

        let wrong_padding = concat!(
            "d6:lengthi1e4:pathl1:aee",
            "d4:attr1:p6:lengthi16382ee",
            "d6:lengthi1e4:pathl1:bee"
        );
        assert!(
            ParsedInfo::from_bytes_with_limits(
                &hybrid_info(&tree, wrong_padding.as_bytes(), 16 * 1024, 2),
                BEP9_METAINFO_LIMITS,
            )
            .is_err()
        );

        let wrong_path = concat!(
            "d6:lengthi1e4:pathl1:aee",
            "d4:attr1:p6:lengthi16383ee",
            "d6:lengthi1e4:pathl1:cee"
        );
        assert!(matches!(
            ParsedInfo::from_bytes_with_limits(
                &hybrid_info(&tree, wrong_path.as_bytes(), 16 * 1024, 2),
                BEP9_METAINFO_LIMITS,
            ),
            Err(MetainfoError::HybridMismatch {
                category: "raw path",
                ..
            })
        ));
    }

    #[test]
    fn complete_outer_requires_and_reconstructs_exact_piece_layers() {
        let piece_hashes = vec![[3_u8; 32], [4_u8; 32]];
        let root = file_root_from_piece_hashes(piece_hashes.iter().copied(), 16 * 1024)
            .expect("file root");
        let info = v2_info(&file_tree(&[(b"a", 16 * 1024 + 1, Some(root))]), 16 * 1024);
        assert_eq!(
            ParsedOuterMetainfo::from_bytes_with_limits(
                &outer(&info, None),
                EXPLICIT_IMPORT_METAINFO_LIMITS,
            ),
            Err(MetainfoError::MissingPieceLayers)
        );

        let layers = layer_dictionary(&[(root, piece_hashes.clone())]);
        let source = outer(&info, Some(&layers));
        let parsed =
            ParsedOuterMetainfo::from_bytes_with_limits(&source, EXPLICIT_IMPORT_METAINFO_LIMITS)
                .expect("complete piece layers");
        assert_eq!(parsed.piece_layers().unwrap().hashes(), piece_hashes);
        assert_eq!(parsed.piece_layers().unwrap().entries()[0].hashes, 0..2);

        let corrupt = layer_dictionary(&[(root, vec![[3; 32], [5; 32]])]);
        assert!(matches!(
            ParsedOuterMetainfo::from_bytes_with_limits(
                &outer(&info, Some(&corrupt)),
                EXPLICIT_IMPORT_METAINFO_LIMITS,
            ),
            Err(MetainfoError::PieceLayer { .. })
        ));

        let short = layer_dictionary(&[(root, vec![[3; 32]])]);
        assert!(matches!(
            ParsedOuterMetainfo::from_bytes_with_limits(
                &outer(&info, Some(&short)),
                EXPLICIT_IMPORT_METAINFO_LIMITS,
            ),
            Err(MetainfoError::PieceLayer { .. })
        ));
    }

    #[test]
    fn complete_outer_requires_an_explicit_empty_layer_dictionary() {
        let info = v2_info(&file_tree(&[(b"a", 1, Some([1; 32]))]), 16 * 1024);
        let source = outer(&info, Some(b"de"));
        let parsed =
            ParsedOuterMetainfo::from_bytes_with_limits(&source, EXPLICIT_IMPORT_METAINFO_LIMITS)
                .expect("explicit empty layers");
        assert!(parsed.piece_layers().unwrap().hashes().is_empty());

        let unexpected = layer_dictionary(&[([1; 32], vec![[2; 32]])]);
        assert!(
            ParsedOuterMetainfo::from_bytes_with_limits(
                &outer(&info, Some(&unexpected)),
                EXPLICIT_IMPORT_METAINFO_LIMITS,
            )
            .is_err()
        );
    }

    #[test]
    fn iterative_tree_projection_handles_depth_and_sanitized_collisions() {
        let mut tree = vec![b'd'];
        for index in 0..48_u8 {
            bytes(&mut tree, &[b'a' + index % 20]);
            tree.push(b'd');
        }
        bytes(&mut tree, b"");
        tree.extend_from_slice(b"d6:lengthi1e11:pieces root32:");
        tree.extend_from_slice(&[9; 32]);
        tree.extend(std::iter::repeat_n(b'e', 50));
        let info = v2_info(&tree, 16 * 1024);
        let parsed = ParsedInfo::from_bytes_with_limits(&info, BEP9_METAINFO_LIMITS)
            .expect("iterative deep tree");
        let ParsedInfoKind::V2(v2) = parsed.kind() else {
            panic!("v2 variant");
        };
        assert_eq!(v2.files[0].raw_path.len(), 48);

        let collision_tree = file_tree(&[(b"a/b", 1, Some([1; 32])), (b"a\\b", 1, Some([2; 32]))]);
        let collision_info = v2_info(&collision_tree, 16 * 1024);
        let parsed = ParsedInfo::from_bytes_with_limits(&collision_info, BEP9_METAINFO_LIMITS)
            .expect("sanitized collision projection");
        let ParsedInfoKind::V2(v2) = parsed.kind() else {
            panic!("v2 variant");
        };
        assert_ne!(v2.files[0].path, v2.files[1].path);
        assert_eq!(v2.files[0].raw_path, [b"a/b".to_vec()]);
        assert_eq!(v2.files[1].raw_path, [b"a\\b".to_vec()]);
    }

    #[test]
    fn file_tree_and_total_component_limits_fail_before_retention_grows() {
        let tree = file_tree(&[(b"a", 1, Some([1; 32])), (b"b", 1, Some([2; 32]))]);
        let info = v2_info(&tree, 16 * 1024);
        let mut file_limited = BEP9_METAINFO_LIMITS;
        file_limited.max_files = 1;
        assert_eq!(
            ParsedInfo::from_bytes_with_limits(&info, file_limited),
            Err(MetainfoError::TooManyFiles {
                actual: 2,
                maximum: 1,
            })
        );

        let mut component_limited = BEP9_METAINFO_LIMITS;
        component_limited.max_path_components = 1;
        assert!(matches!(
            ParsedInfo::from_bytes_with_limits(&info, component_limited),
            Err(MetainfoError::UnsafePath {
                reason: "paths have too many total components",
                ..
            })
        ));
    }
}
