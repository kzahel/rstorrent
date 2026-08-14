//! Owned, runtime-free torrent content and integrity facts.

use std::ops::Range;

use crate::identity::{InfoHashes, SwarmKey, V1InfoHash};
use crate::merkle::{MERKLE_BLOCK_SIZE, MerkleTreeShape, Sha256Hash, piece_layer};
use crate::metainfo::{
    Metainfo, MetainfoError, MetainfoFile, MetainfoFormat, MetainfoLimits, MetainfoTracker,
    ParsedInfo, ParsedInfoKind, ParsedOuterMetainfo, V2File, V2Metainfo,
};
use crate::storage_layout::{LayoutError, TorrentLayout};
use crate::v2_hashes::{HashRequest, MAX_HASH_REQUEST_COUNT, V2FileHashGeometry, V2HashCatalog};
use crate::v2_layout::{V2LayoutError, V2PieceGeometry, V2TorrentLayout};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum V2ExpectedRootSource {
    FileRoot,
    PieceLayer,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExpectedPieceIntegrity {
    V1Sha1([u8; 20]),
    V2Merkle {
        expected_root: Sha256Hash,
        target_height: u8,
        source: V2ExpectedRootSource,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct V1TorrentContent {
    pub metainfo: Metainfo,
    pub layout: TorrentLayout,
    pub trackers: Vec<MetainfoTracker>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct V2ContentDescriptor {
    pub info_hashes: InfoHashes,
    pub raw_info: Vec<u8>,
    pub metainfo: V2Metainfo,
    pub trackers: Vec<MetainfoTracker>,
}

pub type V2TorrentContent = V2ContentDescriptor;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TorrentContent {
    V1(V1TorrentContent),
    V2(V2TorrentContent),
}

impl From<Metainfo> for TorrentContent {
    fn from(metainfo: Metainfo) -> Self {
        Self::from_v1_metainfo(metainfo)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TorrentContentProjection {
    pub content: TorrentContent,
    pub integrity: TorrentIntegrity,
    pub info_span: Range<usize>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TorrentContentWithIntegrity {
    pub content: TorrentContent,
    pub integrity: TorrentIntegrity,
}

impl From<TorrentContentProjection> for TorrentContentWithIntegrity {
    fn from(value: TorrentContentProjection) -> Self {
        Self {
            content: value.content,
            integrity: value.integrity,
        }
    }
}

impl From<Metainfo> for TorrentContentWithIntegrity {
    fn from(value: Metainfo) -> Self {
        Self {
            content: TorrentContent::from_v1_metainfo(value),
            integrity: TorrentIntegrity::V1,
        }
    }
}

impl From<TorrentContent> for TorrentContentWithIntegrity {
    fn from(content: TorrentContent) -> Self {
        let integrity = match &content {
            TorrentContent::V1(_) => TorrentIntegrity::V1,
            TorrentContent::V2(content) => TorrentIntegrity::V2(
                V2HashCatalog::new(content.metainfo.layout.piece_count())
                    .expect("parsed v2 piece count fits the catalog bound"),
            ),
        };
        Self { content, integrity }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TorrentIntegrity {
    V1,
    V2(V2HashCatalog),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum V2ExpectedPieceQuery {
    Known(ExpectedPieceIntegrity),
    Missing {
        geometry: V2FileHashGeometry,
        request: HashRequest,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ContentFileRef<'a> {
    V1(&'a MetainfoFile),
    V2(&'a V2File),
}

impl ContentFileRef<'_> {
    pub fn path(&self) -> &[String] {
        match self {
            Self::V1(file) => &file.path,
            Self::V2(file) => &file.path,
        }
    }

    pub const fn length(&self) -> u64 {
        match self {
            Self::V1(file) => file.length,
            Self::V2(file) => file.length,
        }
    }

    pub const fn padding(&self) -> bool {
        matches!(self, Self::V1(file) if file.padding)
    }
}

impl TorrentContentProjection {
    /// Parse complete outer metainfo and own every fact required by a runtime.
    /// Hybrid input remains outside the complete-source pure-v2 slice.
    pub fn from_bytes_with_limits(
        bytes: &[u8],
        limits: MetainfoLimits,
    ) -> Result<Self, MetainfoError> {
        let parsed = ParsedOuterMetainfo::from_bytes_with_limits(bytes, limits)?;
        let info_span = parsed.info_span();
        let (content, integrity) = TorrentContent::from_parsed_outer(&parsed)?;
        Ok(Self {
            content,
            integrity,
            info_span,
        })
    }
}

impl TorrentContent {
    pub fn from_v2_info_bytes_with_limits(
        bytes: &[u8],
        limits: MetainfoLimits,
    ) -> Result<TorrentContentWithIntegrity, MetainfoError> {
        let parsed = ParsedInfo::from_bytes_with_limits(bytes, limits)?;
        let ParsedInfoKind::V2(metainfo) = parsed.kind() else {
            return Err(MetainfoError::Unsupported("pure-v2 info required"));
        };
        let catalog =
            V2HashCatalog::new(metainfo.layout.piece_count()).map_err(map_hash_catalog_error)?;
        Ok(TorrentContentWithIntegrity {
            content: Self::V2(V2ContentDescriptor {
                info_hashes: parsed.info_hashes(),
                raw_info: bytes.to_vec(),
                metainfo: metainfo.clone(),
                trackers: Vec::new(),
            }),
            integrity: TorrentIntegrity::V2(catalog),
        })
    }

    pub fn from_parsed_outer(
        parsed: &ParsedOuterMetainfo<'_>,
    ) -> Result<(Self, TorrentIntegrity), MetainfoError> {
        let trackers = parsed.trackers().to_vec();
        match parsed.info().kind() {
            ParsedInfoKind::V1(metainfo) => Ok((
                Self::V1(V1TorrentContent {
                    layout: TorrentLayout::from_metainfo(metainfo),
                    metainfo: metainfo.clone(),
                    trackers,
                }),
                TorrentIntegrity::V1,
            )),
            ParsedInfoKind::V2(metainfo) => {
                let layers = parsed
                    .piece_layers()
                    .ok_or(MetainfoError::MissingPieceLayers)?;
                let mut catalog = V2HashCatalog::new(metainfo.layout.piece_count())
                    .map_err(map_hash_catalog_error)?;
                for (file, geometry) in metainfo.files.iter().zip(metainfo.layout.files()) {
                    if geometry.piece_count() <= 1 {
                        continue;
                    }
                    let root = file.pieces_root.ok_or(MetainfoError::MissingPieceLayers)?;
                    let entry = layers
                        .entries()
                        .iter()
                        .find(|entry| entry.pieces_root == root)
                        .ok_or(MetainfoError::MissingPieceLayers)?;
                    let hashes = layers
                        .hashes()
                        .get(entry.hashes.clone())
                        .ok_or(MetainfoError::MissingPieceLayers)?;
                    let hash_geometry = V2FileHashGeometry::new(
                        root,
                        file.length,
                        metainfo.piece_length,
                        geometry.start_piece(),
                        geometry.piece_count(),
                    )
                    .map_err(map_hash_catalog_error)?;
                    catalog
                        .seed_complete_piece_layer(hash_geometry, hashes)
                        .map_err(map_hash_catalog_error)?;
                }
                Ok((
                    Self::V2(V2ContentDescriptor {
                        info_hashes: parsed.info().info_hashes(),
                        raw_info: parsed.info().exact_info_bytes().to_vec(),
                        metainfo: metainfo.clone(),
                        trackers,
                    }),
                    TorrentIntegrity::V2(catalog),
                ))
            }
            ParsedInfoKind::Hybrid(_) => Err(MetainfoError::Unsupported("hybrid runtime content")),
        }
    }

    pub fn from_v1_metainfo(metainfo: Metainfo) -> Self {
        Self::V1(V1TorrentContent {
            layout: TorrentLayout::from_metainfo(&metainfo),
            metainfo,
            trackers: Vec::new(),
        })
    }

    pub const fn format(&self) -> MetainfoFormat {
        match self {
            Self::V1(_) => MetainfoFormat::V1,
            Self::V2(_) => MetainfoFormat::V2,
        }
    }

    pub fn info_hashes(&self) -> InfoHashes {
        match self {
            Self::V1(content) => InfoHashes::v1(V1InfoHash::new(content.metainfo.info_hash)),
            Self::V2(content) => content.info_hashes,
        }
    }

    pub fn swarm_key(&self) -> SwarmKey {
        match self {
            Self::V1(content) => SwarmKey::V1(V1InfoHash::new(content.metainfo.info_hash)),
            Self::V2(content) => content
                .info_hashes
                .v2_hash()
                .expect("pure-v2 content has a v2 identity")
                .swarm_key(),
        }
    }

    pub fn name(&self) -> &str {
        match self {
            Self::V1(content) => &content.metainfo.name,
            Self::V2(content) => &content.metainfo.name,
        }
    }

    pub const fn private(&self) -> bool {
        match self {
            Self::V1(content) => content.metainfo.private,
            Self::V2(content) => content.metainfo.private,
        }
    }

    pub const fn piece_length(&self) -> u32 {
        match self {
            Self::V1(content) => content.metainfo.piece_length,
            Self::V2(content) => content.metainfo.piece_length,
        }
    }

    pub fn piece_count(&self) -> usize {
        match self {
            Self::V1(content) => content.layout.piece_count(),
            Self::V2(content) => content.metainfo.layout.piece_count(),
        }
    }

    pub const fn payload_length(&self) -> u64 {
        match self {
            Self::V1(content) => content.metainfo.total_length,
            Self::V2(content) => content.metainfo.total_length,
        }
    }

    pub fn files(&self) -> impl ExactSizeIterator<Item = ContentFileRef<'_>> {
        let files = match self {
            Self::V1(content) => content
                .metainfo
                .files
                .iter()
                .map(ContentFileRef::V1)
                .collect::<Vec<_>>(),
            Self::V2(content) => content
                .metainfo
                .files
                .iter()
                .map(ContentFileRef::V2)
                .collect::<Vec<_>>(),
        };
        files.into_iter()
    }

    pub fn trackers(&self) -> &[MetainfoTracker] {
        match self {
            Self::V1(content) => &content.trackers,
            Self::V2(content) => &content.trackers,
        }
    }

    pub fn piece_length_at(&self, index: u32) -> Result<u32, ContentGeometryError> {
        match self {
            Self::V1(content) => content
                .layout
                .piece_length_at(index)
                .map_err(ContentGeometryError::V1),
            Self::V2(content) => content
                .metainfo
                .layout
                .piece(index)
                .map(|piece| piece.payload_length)
                .map_err(ContentGeometryError::V2),
        }
    }

    pub fn v2_piece(&self, index: u32) -> Result<V2PieceGeometry, ContentGeometryError> {
        match self {
            Self::V1(_) => Err(ContentGeometryError::WrongFormat),
            Self::V2(content) => content
                .metainfo
                .layout
                .piece(index)
                .map_err(ContentGeometryError::V2),
        }
    }

    pub fn expected_piece(
        &self,
        integrity: &TorrentIntegrity,
        index: u32,
    ) -> Result<ExpectedPieceIntegrity, ContentGeometryError> {
        match self {
            Self::V1(content) => content
                .metainfo
                .piece_hashes
                .get(usize::try_from(index).map_err(|_| ContentGeometryError::ArithmeticOverflow)?)
                .copied()
                .map(ExpectedPieceIntegrity::V1Sha1)
                .ok_or(ContentGeometryError::InvalidPieceIndex(index)),
            Self::V2(content) => match integrity {
                TorrentIntegrity::V2(catalog) => {
                    match expected_v2_piece(content, catalog, index)? {
                        V2ExpectedPieceQuery::Known(expected) => Ok(expected),
                        V2ExpectedPieceQuery::Missing { .. } => {
                            Err(ContentGeometryError::MissingPieceLayer(
                                content
                                    .metainfo
                                    .layout
                                    .piece(index)
                                    .map_err(ContentGeometryError::V2)?
                                    .file_index,
                            ))
                        }
                    }
                }
                TorrentIntegrity::V1 => Err(ContentGeometryError::WrongFormat),
            },
        }
    }

    pub fn v2_expected_piece(
        &self,
        integrity: &TorrentIntegrity,
        index: u32,
    ) -> Result<V2ExpectedPieceQuery, ContentGeometryError> {
        match (self, integrity) {
            (Self::V2(content), TorrentIntegrity::V2(catalog)) => {
                expected_v2_piece(content, catalog, index)
            }
            _ => Err(ContentGeometryError::WrongFormat),
        }
    }

    pub fn piece_hash_target_height(&self, index: u32) -> Result<Option<u8>, ContentGeometryError> {
        match self {
            Self::V1(content) => content
                .layout
                .piece_length_at(index)
                .map(|_| None)
                .map_err(ContentGeometryError::V1),
            Self::V2(content) => {
                let piece = content
                    .metainfo
                    .layout
                    .piece(index)
                    .map_err(ContentGeometryError::V2)?;
                let file = content
                    .metainfo
                    .files
                    .get(piece.file_index)
                    .ok_or(ContentGeometryError::ArithmeticOverflow)?;
                let file_geometry = content
                    .metainfo
                    .layout
                    .files()
                    .get(piece.file_index)
                    .ok_or(ContentGeometryError::ArithmeticOverflow)?;
                if file_geometry.piece_count() <= 1 {
                    return MerkleTreeShape::new(file.length.div_ceil(MERKLE_BLOCK_SIZE as u64))
                        .map(|shape| Some(shape.height()))
                        .map_err(ContentGeometryError::Merkle);
                }
                piece_layer(content.metainfo.piece_length)
                    .map(Some)
                    .map_err(ContentGeometryError::Merkle)
            }
        }
    }

    pub fn v1(&self) -> Option<&V1TorrentContent> {
        match self {
            Self::V1(content) => Some(content),
            Self::V2(_) => None,
        }
    }

    pub fn v2(&self) -> Option<&V2TorrentContent> {
        match self {
            Self::V1(_) => None,
            Self::V2(content) => Some(content),
        }
    }

    pub fn v2_hash_geometry_for_root(
        &self,
        pieces_root: Sha256Hash,
    ) -> Result<Option<V2FileHashGeometry>, ContentGeometryError> {
        let Self::V2(content) = self else {
            return Err(ContentGeometryError::WrongFormat);
        };
        for (file, geometry) in content
            .metainfo
            .files
            .iter()
            .zip(content.metainfo.layout.files())
        {
            if file.pieces_root != Some(pieces_root) || geometry.piece_count() == 0 {
                continue;
            }
            return V2FileHashGeometry::new(
                pieces_root,
                file.length,
                content.metainfo.piece_length,
                geometry.start_piece(),
                geometry.piece_count(),
            )
            .map(Some)
            .map_err(|_| ContentGeometryError::ArithmeticOverflow);
        }
        Ok(None)
    }
}

fn expected_v2_piece(
    content: &V2TorrentContent,
    catalog: &V2HashCatalog,
    index: u32,
) -> Result<V2ExpectedPieceQuery, ContentGeometryError> {
    let piece = content
        .metainfo
        .layout
        .piece(index)
        .map_err(ContentGeometryError::V2)?;
    let file = content
        .metainfo
        .files
        .get(piece.file_index)
        .ok_or(ContentGeometryError::ArithmeticOverflow)?;
    let file_root = file
        .pieces_root
        .ok_or(ContentGeometryError::MissingFileRoot(piece.file_index))?;
    let file_geometry = content
        .metainfo
        .layout
        .files()
        .get(piece.file_index)
        .ok_or(ContentGeometryError::ArithmeticOverflow)?;
    if file_geometry.piece_count() <= 1 {
        let leaf_count = file.length.div_ceil(MERKLE_BLOCK_SIZE as u64);
        let target_height = MerkleTreeShape::new(leaf_count)
            .map_err(ContentGeometryError::Merkle)?
            .height();
        return Ok(V2ExpectedPieceQuery::Known(
            ExpectedPieceIntegrity::V2Merkle {
                expected_root: file_root,
                target_height,
                source: V2ExpectedRootSource::FileRoot,
            },
        ));
    }
    let expected_root = catalog.piece_root(index);
    if let Some(expected_root) = expected_root {
        return Ok(V2ExpectedPieceQuery::Known(
            ExpectedPieceIntegrity::V2Merkle {
                expected_root,
                target_height: piece_layer(content.metainfo.piece_length)
                    .map_err(ContentGeometryError::Merkle)?,
                source: V2ExpectedRootSource::PieceLayer,
            },
        ));
    }

    let geometry = V2FileHashGeometry::new(
        file_root,
        file.length,
        content.metainfo.piece_length,
        file_geometry.start_piece(),
        file_geometry.piece_count(),
    )
    .map_err(|_| ContentGeometryError::ArithmeticOverflow)?;
    let padded_pieces = file_geometry.piece_count().next_power_of_two();
    let count = padded_pieces.min(MAX_HASH_REQUEST_COUNT);
    let request_index = piece.local_piece / count * count;
    let shape = MerkleTreeShape::new(
        geometry
            .leaf_count()
            .map_err(|_| ContentGeometryError::ArithmeticOverflow)?,
    )
    .map_err(ContentGeometryError::Merkle)?;
    let base_layer = geometry
        .piece_layer()
        .map_err(|_| ContentGeometryError::ArithmeticOverflow)?;
    let range_height = u8::try_from(count.trailing_zeros())
        .map_err(|_| ContentGeometryError::ArithmeticOverflow)?;
    base_layer
        .checked_add(range_height)
        .filter(|subject| *subject <= shape.height())
        .ok_or(ContentGeometryError::ArithmeticOverflow)?;
    let proof_layers = u32::from(
        shape
            .height()
            .checked_sub(base_layer)
            .ok_or(ContentGeometryError::ArithmeticOverflow)?,
    )
    .saturating_sub(u32::from(count > 1));
    Ok(V2ExpectedPieceQuery::Missing {
        geometry,
        request: HashRequest {
            pieces_root: file_root,
            base_layer: u32::from(base_layer),
            index: request_index,
            count,
            proof_layers,
        },
    })
}

fn map_hash_catalog_error(error: crate::v2_hashes::HashExchangeError) -> MetainfoError {
    match error {
        crate::v2_hashes::HashExchangeError::Merkle(error) => MetainfoError::Merkle(error),
        crate::v2_hashes::HashExchangeError::TooManyPieceRoots { actual, maximum } => {
            MetainfoError::TooManyPieces { actual, maximum }
        }
        _ => MetainfoError::Unsupported("invalid complete v2 hash catalog"),
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ContentGeometryError {
    WrongFormat,
    InvalidPieceIndex(u32),
    MissingFileRoot(usize),
    MissingPieceLayer(usize),
    ArithmeticOverflow,
    V1(LayoutError),
    V2(V2LayoutError),
    Merkle(crate::merkle::MerkleError),
}

impl std::fmt::Display for ContentGeometryError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::WrongFormat => formatter.write_str("operation requires BEP 52 content"),
            Self::InvalidPieceIndex(index) => write!(formatter, "invalid piece index {index}"),
            Self::MissingFileRoot(index) => write!(formatter, "v2 file {index} has no pieces root"),
            Self::MissingPieceLayer(index) => {
                write!(formatter, "v2 file {index} has no complete piece layer")
            }
            Self::ArithmeticOverflow => formatter.write_str("content geometry arithmetic overflow"),
            Self::V1(error) => write!(formatter, "v1 layout: {error}"),
            Self::V2(error) => write!(formatter, "v2 layout: {error}"),
            Self::Merkle(error) => write!(formatter, "Merkle geometry: {error}"),
        }
    }
}

impl std::error::Error for ContentGeometryError {}

pub fn v2_layout(content: &TorrentContent) -> Option<&V2TorrentLayout> {
    content.v2().map(|content| &content.metainfo.layout)
}

#[cfg(test)]
mod tests {
    use sha2::{Digest, Sha256};

    use super::*;
    use crate::merkle::{file_root_from_data, piece_root_from_data};
    use crate::metainfo::EXPLICIT_IMPORT_METAINFO_LIMITS;

    fn bstr(output: &mut Vec<u8>, value: &[u8]) {
        output.extend_from_slice(value.len().to_string().as_bytes());
        output.push(b':');
        output.extend_from_slice(value);
    }

    fn pure_v2_source(files: &[(&[u8], &[u8])], piece_length: u32) -> Vec<u8> {
        let roots = files
            .iter()
            .map(|(_, data)| file_root_from_data(data).expect("nonempty fixture file"))
            .collect::<Vec<_>>();
        let mut info = b"d9:file treed".to_vec();
        for ((name, data), root) in files.iter().zip(&roots) {
            bstr(&mut info, name);
            info.extend_from_slice(b"d0:d6:lengthi");
            info.extend_from_slice(data.len().to_string().as_bytes());
            info.extend_from_slice(b"e11:pieces root32:");
            info.extend_from_slice(root);
            info.extend_from_slice(b"ee");
        }
        info.extend_from_slice(b"e12:meta versioni2e4:name4:root12:piece lengthi");
        info.extend_from_slice(piece_length.to_string().as_bytes());
        info.extend_from_slice(b"ee");

        let mut source = b"d4:info".to_vec();
        source.extend_from_slice(&info);
        let large = files
            .iter()
            .zip(&roots)
            .filter(|((_, data), _)| data.len() > piece_length as usize)
            .collect::<Vec<_>>();
        if !large.is_empty() {
            source.extend_from_slice(b"12:piece layersd");
            for ((_, data), root) in large {
                bstr(&mut source, root);
                let hashes = data
                    .chunks(piece_length as usize)
                    .map(|piece| piece_root_from_data(piece, piece_length).expect("piece root"))
                    .collect::<Vec<_>>();
                bstr(&mut source, &hashes.concat());
            }
            source.push(b'e');
        }
        source.push(b'e');
        source
    }

    #[test]
    fn complete_v2_projection_owns_identity_geometry_and_layers() {
        let small = vec![7; 17];
        let large = (0..40_000)
            .map(|index| (index % 251) as u8)
            .collect::<Vec<_>>();
        let source = pure_v2_source(&[(b"a", &small), (b"b", &large)], 32 * 1024);
        let projection = TorrentContentProjection::from_bytes_with_limits(
            &source,
            EXPLICIT_IMPORT_METAINFO_LIMITS,
        )
        .expect("complete pure-v2 source");

        assert_eq!(projection.content.format(), MetainfoFormat::V2);
        assert_eq!(projection.content.piece_count(), 3);
        assert_eq!(projection.content.piece_length_at(0), Ok(17));
        assert_eq!(projection.content.piece_length_at(1), Ok(32 * 1024));
        assert_eq!(projection.content.piece_length_at(2), Ok(7_232));
        assert_eq!(
            projection.content.swarm_key().as_bytes(),
            &Sha256::digest(&source[projection.info_span]).as_slice()[..20]
        );
        assert!(matches!(
            projection.content.expected_piece(&projection.integrity, 0),
            Ok(ExpectedPieceIntegrity::V2Merkle {
                target_height: 0,
                source: V2ExpectedRootSource::FileRoot,
                ..
            })
        ));
        for piece in [1, 2] {
            assert!(matches!(
                projection
                    .content
                    .expected_piece(&projection.integrity, piece),
                Ok(ExpectedPieceIntegrity::V2Merkle {
                    target_height: 1,
                    source: V2ExpectedRootSource::PieceLayer,
                    ..
                })
            ));
        }
    }

    #[test]
    fn info_only_v2_has_geometry_and_explicit_missing_hash_need() {
        let data = vec![1; 40_000];
        let source = pure_v2_source(&[(b"a", &data)], 32 * 1024);
        let info =
            ParsedOuterMetainfo::from_bytes_with_limits(&source, EXPLICIT_IMPORT_METAINFO_LIMITS)
                .expect("complete source")
                .info_span();
        let runtime = TorrentContent::from_v2_info_bytes_with_limits(
            &source[info.clone()],
            EXPLICIT_IMPORT_METAINFO_LIMITS,
        )
        .expect("strict info-only v2 descriptor");
        assert_eq!(runtime.content.piece_count(), 2);
        assert_eq!(
            runtime.content.v2().expect("v2 descriptor").raw_info,
            source[info]
        );
        assert!(matches!(
            runtime.content.v2_expected_piece(&runtime.integrity, 0),
            Ok(V2ExpectedPieceQuery::Missing {
                request: HashRequest {
                    base_layer: 1,
                    index: 0,
                    count: 2,
                    proof_layers: 0,
                    ..
                },
                ..
            })
        ));
        assert!(
            ParsedOuterMetainfo::from_bytes_with_limits(
                runtime
                    .content
                    .v2()
                    .expect("v2 descriptor")
                    .raw_info
                    .as_slice(),
                EXPLICIT_IMPORT_METAINFO_LIMITS,
            )
            .is_err()
        );
    }
}
