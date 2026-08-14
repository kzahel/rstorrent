//! Owned, runtime-free torrent content and integrity facts.

use std::ops::Range;

use crate::identity::{InfoHashes, SwarmKey, V1InfoHash};
use crate::merkle::{MERKLE_BLOCK_SIZE, MerkleTreeShape, Sha256Hash, piece_layer};
use crate::metainfo::{
    CompletePieceLayers, HybridMetainfo, Metainfo, MetainfoError, MetainfoFile, MetainfoFormat,
    MetainfoLimits, MetainfoTracker, ParsedInfo, ParsedInfoKind, ParsedOuterMetainfo, V2File,
    V2Metainfo,
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
    Hybrid {
        v1_sha1: [u8; 20],
        v2_expected_root: Sha256Hash,
        v2_target_height: u8,
        v2_source: V2ExpectedRootSource,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HybridVerificationOutcome {
    Verified,
    Invalid,
    Inconsistent { v1_matched: bool, v2_matched: bool },
}

impl HybridVerificationOutcome {
    pub const fn classify(v1_matched: bool, v2_matched: bool) -> Self {
        match (v1_matched, v2_matched) {
            (true, true) => Self::Verified,
            (false, false) => Self::Invalid,
            _ => Self::Inconsistent {
                v1_matched,
                v2_matched,
            },
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HybridPaddingSpan {
    pub piece_index: u32,
    pub begin: u32,
    pub length: u32,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct HybridPaddingMap {
    spans: Vec<HybridPaddingSpan>,
}

impl HybridPaddingMap {
    fn from_metainfo(metainfo: &HybridMetainfo) -> Result<Self, MetainfoError> {
        let piece_length = u64::from(metainfo.v1.piece_length);
        let mut spans = Vec::new();
        for file in metainfo.v1.files.iter().filter(|file| file.padding) {
            let mut offset = file.offset;
            let mut remaining = file.length;
            while remaining != 0 {
                let piece_index = u32::try_from(offset / piece_length)
                    .map_err(|_| MetainfoError::InvalidField("info.files padding offset"))?;
                let begin = u32::try_from(offset % piece_length)
                    .map_err(|_| MetainfoError::InvalidField("info.files padding offset"))?;
                let length = remaining.min(piece_length - u64::from(begin));
                spans.push(HybridPaddingSpan {
                    piece_index,
                    begin,
                    length: u32::try_from(length)
                        .map_err(|_| MetainfoError::InvalidField("info.files padding length"))?,
                });
                offset = offset
                    .checked_add(length)
                    .ok_or(MetainfoError::TotalLengthOverflow)?;
                remaining -= length;
            }
        }
        Ok(Self { spans })
    }

    pub fn spans(&self) -> &[HybridPaddingSpan] {
        &self.spans
    }

    pub fn piece_spans(&self, piece_index: u32) -> impl Iterator<Item = HybridPaddingSpan> + '_ {
        self.spans
            .iter()
            .copied()
            .filter(move |span| span.piece_index == piece_index)
    }

    pub fn zero_length(&self, piece_index: u32) -> u32 {
        self.piece_spans(piece_index)
            .fold(0, |total, span| total.saturating_add(span.length))
    }
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
pub struct HybridContentDescriptor {
    pub info_hashes: InfoHashes,
    pub raw_info: Vec<u8>,
    pub metainfo: HybridMetainfo,
    pub v1_layout: TorrentLayout,
    pub padding: HybridPaddingMap,
    pub trackers: Vec<MetainfoTracker>,
}

pub type HybridTorrentContent = HybridContentDescriptor;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TorrentContent {
    V1(V1TorrentContent),
    V2(V2TorrentContent),
    Hybrid(HybridTorrentContent),
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
            TorrentContent::Hybrid(content) => TorrentIntegrity::Hybrid(
                V2HashCatalog::new(content.metainfo.v2.layout.piece_count())
                    .expect("parsed hybrid piece count fits the catalog bound"),
            ),
        };
        Self { content, integrity }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TorrentIntegrity {
    V1,
    V2(V2HashCatalog),
    Hybrid(V2HashCatalog),
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

    pub fn from_hybrid_info_bytes_with_limits(
        bytes: &[u8],
        limits: MetainfoLimits,
    ) -> Result<TorrentContentWithIntegrity, MetainfoError> {
        let parsed = ParsedInfo::from_bytes_with_limits(bytes, limits)?;
        let ParsedInfoKind::Hybrid(metainfo) = parsed.kind() else {
            return Err(MetainfoError::Unsupported("hybrid info required"));
        };
        let catalog =
            V2HashCatalog::new(metainfo.v2.layout.piece_count()).map_err(map_hash_catalog_error)?;
        Ok(TorrentContentWithIntegrity {
            content: Self::Hybrid(HybridContentDescriptor {
                info_hashes: parsed.info_hashes(),
                raw_info: bytes.to_vec(),
                padding: HybridPaddingMap::from_metainfo(metainfo)?,
                v1_layout: TorrentLayout::from_metainfo(&metainfo.v1),
                metainfo: metainfo.clone(),
                trackers: Vec::new(),
            }),
            integrity: TorrentIntegrity::Hybrid(catalog),
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
                let catalog = complete_v2_catalog(metainfo, layers)?;
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
            ParsedInfoKind::Hybrid(metainfo) => {
                let layers = parsed
                    .piece_layers()
                    .ok_or(MetainfoError::MissingPieceLayers)?;
                let catalog = complete_v2_catalog(&metainfo.v2, layers)?;
                Ok((
                    Self::Hybrid(HybridContentDescriptor {
                        info_hashes: parsed.info().info_hashes(),
                        raw_info: parsed.info().exact_info_bytes().to_vec(),
                        padding: HybridPaddingMap::from_metainfo(metainfo)?,
                        v1_layout: TorrentLayout::from_metainfo(&metainfo.v1),
                        metainfo: metainfo.clone(),
                        trackers,
                    }),
                    TorrentIntegrity::Hybrid(catalog),
                ))
            }
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
            Self::Hybrid(_) => MetainfoFormat::Hybrid,
        }
    }

    pub fn info_hashes(&self) -> InfoHashes {
        match self {
            Self::V1(content) => InfoHashes::v1(V1InfoHash::new(content.metainfo.info_hash)),
            Self::V2(content) => content.info_hashes,
            Self::Hybrid(content) => content.info_hashes,
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
            Self::Hybrid(content) => SwarmKey::V1(
                content
                    .info_hashes
                    .v1_hash()
                    .expect("hybrid content has a v1 identity"),
            ),
        }
    }

    pub fn swarm_keys(&self) -> impl ExactSizeIterator<Item = SwarmKey> {
        let hashes = self.info_hashes();
        let mut keys = Vec::with_capacity(hashes.identity_count());
        hashes.for_each(|identity| keys.push(identity.swarm_key()));
        keys.into_iter()
    }

    pub fn name(&self) -> &str {
        match self {
            Self::V1(content) => &content.metainfo.name,
            Self::V2(content) => &content.metainfo.name,
            Self::Hybrid(content) => &content.metainfo.v2.name,
        }
    }

    pub const fn private(&self) -> bool {
        match self {
            Self::V1(content) => content.metainfo.private,
            Self::V2(content) => content.metainfo.private,
            Self::Hybrid(content) => content.metainfo.v2.private,
        }
    }

    pub const fn piece_length(&self) -> u32 {
        match self {
            Self::V1(content) => content.metainfo.piece_length,
            Self::V2(content) => content.metainfo.piece_length,
            Self::Hybrid(content) => content.metainfo.v2.piece_length,
        }
    }

    pub fn piece_count(&self) -> usize {
        match self {
            Self::V1(content) => content.layout.piece_count(),
            Self::V2(content) => content.metainfo.layout.piece_count(),
            Self::Hybrid(content) => content.metainfo.v2.layout.piece_count(),
        }
    }

    pub const fn payload_length(&self) -> u64 {
        match self {
            Self::V1(content) => content.metainfo.total_length,
            Self::V2(content) => content.metainfo.total_length,
            Self::Hybrid(content) => content.metainfo.v2.total_length,
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
            Self::Hybrid(content) => content
                .metainfo
                .v2
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
            Self::Hybrid(content) => &content.trackers,
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
            Self::Hybrid(content) => content
                .metainfo
                .v2
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
            Self::Hybrid(content) => content
                .metainfo
                .v2
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
                    match expected_v2_piece(&content.metainfo, catalog, index)? {
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
                TorrentIntegrity::V1 | TorrentIntegrity::Hybrid(_) => {
                    Err(ContentGeometryError::WrongFormat)
                }
            },
            Self::Hybrid(content) => match integrity {
                TorrentIntegrity::Hybrid(catalog) => {
                    let v1_sha1 = content
                        .metainfo
                        .v1
                        .piece_hashes
                        .get(
                            usize::try_from(index)
                                .map_err(|_| ContentGeometryError::ArithmeticOverflow)?,
                        )
                        .copied()
                        .ok_or(ContentGeometryError::InvalidPieceIndex(index))?;
                    match expected_v2_piece(&content.metainfo.v2, catalog, index)? {
                        V2ExpectedPieceQuery::Known(ExpectedPieceIntegrity::V2Merkle {
                            expected_root,
                            target_height,
                            source,
                        }) => Ok(ExpectedPieceIntegrity::Hybrid {
                            v1_sha1,
                            v2_expected_root: expected_root,
                            v2_target_height: target_height,
                            v2_source: source,
                        }),
                        V2ExpectedPieceQuery::Missing { .. } => {
                            Err(ContentGeometryError::MissingPieceLayer(
                                content
                                    .metainfo
                                    .v2
                                    .layout
                                    .piece(index)
                                    .map_err(ContentGeometryError::V2)?
                                    .file_index,
                            ))
                        }
                        V2ExpectedPieceQuery::Known(_) => Err(ContentGeometryError::WrongFormat),
                    }
                }
                TorrentIntegrity::V1 | TorrentIntegrity::V2(_) => {
                    Err(ContentGeometryError::WrongFormat)
                }
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
                expected_v2_piece(&content.metainfo, catalog, index)
            }
            (Self::Hybrid(content), TorrentIntegrity::Hybrid(catalog)) => {
                expected_v2_piece(&content.metainfo.v2, catalog, index)
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
            Self::V2(content) => piece_hash_target_height(&content.metainfo, index),
            Self::Hybrid(content) => piece_hash_target_height(&content.metainfo.v2, index),
        }
    }

    pub fn v1(&self) -> Option<&V1TorrentContent> {
        match self {
            Self::V1(content) => Some(content),
            Self::V2(_) | Self::Hybrid(_) => None,
        }
    }

    pub fn v2(&self) -> Option<&V2TorrentContent> {
        match self {
            Self::V1(_) => None,
            Self::V2(content) => Some(content),
            Self::Hybrid(_) => None,
        }
    }

    pub fn hybrid(&self) -> Option<&HybridTorrentContent> {
        match self {
            Self::Hybrid(content) => Some(content),
            Self::V1(_) | Self::V2(_) => None,
        }
    }

    pub fn v2_metainfo(&self) -> Option<&V2Metainfo> {
        match self {
            Self::V1(_) => None,
            Self::V2(content) => Some(&content.metainfo),
            Self::Hybrid(content) => Some(&content.metainfo.v2),
        }
    }

    pub fn hybrid_padding(&self) -> Option<&HybridPaddingMap> {
        self.hybrid().map(|content| &content.padding)
    }

    pub fn hybrid_peer_piece_length_at(&self, index: u32) -> Result<u32, ContentGeometryError> {
        let Self::Hybrid(content) = self else {
            return Err(ContentGeometryError::WrongFormat);
        };
        content
            .v1_layout
            .piece_length_at(index)
            .map_err(ContentGeometryError::V1)
    }

    pub fn v2_hash_geometry_for_root(
        &self,
        pieces_root: Sha256Hash,
    ) -> Result<Option<V2FileHashGeometry>, ContentGeometryError> {
        let metainfo = self
            .v2_metainfo()
            .ok_or(ContentGeometryError::WrongFormat)?;
        for (file, geometry) in metainfo.files.iter().zip(metainfo.layout.files()) {
            if file.pieces_root != Some(pieces_root) || geometry.piece_count() == 0 {
                continue;
            }
            return V2FileHashGeometry::new(
                pieces_root,
                file.length,
                metainfo.piece_length,
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
    metainfo: &V2Metainfo,
    catalog: &V2HashCatalog,
    index: u32,
) -> Result<V2ExpectedPieceQuery, ContentGeometryError> {
    let piece = metainfo
        .layout
        .piece(index)
        .map_err(ContentGeometryError::V2)?;
    let file = metainfo
        .files
        .get(piece.file_index)
        .ok_or(ContentGeometryError::ArithmeticOverflow)?;
    let file_root = file
        .pieces_root
        .ok_or(ContentGeometryError::MissingFileRoot(piece.file_index))?;
    let file_geometry = metainfo
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
                target_height: piece_layer(metainfo.piece_length)
                    .map_err(ContentGeometryError::Merkle)?,
                source: V2ExpectedRootSource::PieceLayer,
            },
        ));
    }

    let geometry = V2FileHashGeometry::new(
        file_root,
        file.length,
        metainfo.piece_length,
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

fn piece_hash_target_height(
    metainfo: &V2Metainfo,
    index: u32,
) -> Result<Option<u8>, ContentGeometryError> {
    let piece = metainfo
        .layout
        .piece(index)
        .map_err(ContentGeometryError::V2)?;
    let file = metainfo
        .files
        .get(piece.file_index)
        .ok_or(ContentGeometryError::ArithmeticOverflow)?;
    let file_geometry = metainfo
        .layout
        .files()
        .get(piece.file_index)
        .ok_or(ContentGeometryError::ArithmeticOverflow)?;
    if file_geometry.piece_count() <= 1 {
        return MerkleTreeShape::new(file.length.div_ceil(MERKLE_BLOCK_SIZE as u64))
            .map(|shape| Some(shape.height()))
            .map_err(ContentGeometryError::Merkle);
    }
    piece_layer(metainfo.piece_length)
        .map(Some)
        .map_err(ContentGeometryError::Merkle)
}

fn complete_v2_catalog(
    metainfo: &V2Metainfo,
    layers: &CompletePieceLayers,
) -> Result<V2HashCatalog, MetainfoError> {
    let mut catalog =
        V2HashCatalog::new(metainfo.layout.piece_count()).map_err(map_hash_catalog_error)?;
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
    Ok(catalog)
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
    content.v2_metainfo().map(|metainfo| &metainfo.layout)
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

    fn hybrid_info_with_internal_padding() -> Vec<u8> {
        let roots = [
            file_root_from_data(&[1]).expect("first root"),
            file_root_from_data(&[2]).expect("second root"),
        ];
        let mut tree = vec![b'd'];
        for (name, root) in [(b'a', roots[0]), (b'b', roots[1])] {
            bstr(&mut tree, &[name]);
            tree.extend_from_slice(b"d0:d6:lengthi1e11:pieces root32:");
            tree.extend_from_slice(&root);
            tree.extend_from_slice(b"ee");
        }
        tree.push(b'e');

        let mut info = b"d9:file tree".to_vec();
        info.extend_from_slice(&tree);
        info.extend_from_slice(
            concat!(
                "5:filesl",
                "d6:lengthi1e4:pathl1:aee",
                "d4:attr1:p6:lengthi16383ee",
                "d6:lengthi1e4:pathl1:bee",
                "e12:meta versioni2e4:name4:root12:piece lengthi16384e",
                "6:pieces40:"
            )
            .as_bytes(),
        );
        info.extend_from_slice(&[7; 40]);
        info.push(b'e');
        info
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

    #[test]
    fn hybrid_descriptor_owns_both_hashes_padding_and_dual_expectation() {
        let info = hybrid_info_with_internal_padding();
        let runtime = TorrentContent::from_hybrid_info_bytes_with_limits(
            &info,
            EXPLICIT_IMPORT_METAINFO_LIMITS,
        )
        .expect("strict info-only hybrid descriptor");
        assert_eq!(runtime.content.format(), MetainfoFormat::Hybrid);
        assert!(runtime.content.info_hashes().is_hybrid());
        assert_eq!(runtime.content.files().len(), 2);
        assert_eq!(runtime.content.payload_length(), 2);
        assert_eq!(runtime.content.piece_length_at(0), Ok(1));
        assert_eq!(runtime.content.hybrid_peer_piece_length_at(0), Ok(16_384));
        let padding = runtime.content.hybrid_padding().expect("hybrid padding");
        assert_eq!(
            padding.spans(),
            &[HybridPaddingSpan {
                piece_index: 0,
                begin: 1,
                length: 16_383,
            }]
        );
        assert_eq!(padding.zero_length(0), 16_383);
        assert_eq!(padding.zero_length(1), 0);
        assert!(matches!(
            runtime.content.expected_piece(&runtime.integrity, 0),
            Ok(ExpectedPieceIntegrity::Hybrid {
                v1_sha1,
                v2_source: V2ExpectedRootSource::FileRoot,
                ..
            }) if v1_sha1 == [7; 20]
        ));
        assert_eq!(
            HybridVerificationOutcome::classify(true, true),
            HybridVerificationOutcome::Verified
        );
        assert_eq!(
            HybridVerificationOutcome::classify(false, false),
            HybridVerificationOutcome::Invalid
        );
        assert!(matches!(
            HybridVerificationOutcome::classify(true, false),
            HybridVerificationOutcome::Inconsistent { .. }
        ));
    }
}
