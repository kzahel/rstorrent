use std::error::Error;
use std::ffi::OsString;
use std::fmt;
use std::io;
use std::path::{Path, PathBuf};

use rstorrent_protocol::metainfo::Metainfo;
use rstorrent_protocol::storage_layout::{
    FileSelection, LayoutError, SegmentTarget, TorrentLayout,
};
use sha1::{Digest, Sha1};
use tokio::fs::{File, OpenOptions};
use tokio::io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt, SeekFrom};

use crate::part_file::{PartFile, PartFileError, PartFileIdentity};
use crate::storage::VERIFICATION_CHUNK_LENGTH;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct SelectiveWriteStats {
    pub wanted_bytes: usize,
    pub skipped_bytes: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MaterializationReport {
    pub file_index: usize,
    pub bytes: u64,
    pub slots_before: usize,
    pub slots_after: usize,
}

#[derive(Debug)]
pub enum SelectiveStorageError {
    InvalidOutputPath,
    ExistingOutput(PathBuf),
    ExistingStaging(PathBuf),
    ExistingPartFile(PathBuf),
    ExistingMaterialization(PathBuf),
    Layout(LayoutError),
    PartFile(PartFileError),
    MissingWantedFile {
        file_index: usize,
    },
    PaddingInPeerBlock,
    InvalidVerifiedPiece {
        piece_index: usize,
    },
    IncompleteSelection {
        piece_index: usize,
    },
    NotPublished,
    AlreadyWanted {
        file_index: usize,
    },
    IncompleteMaterialization {
        file_index: usize,
        piece_index: usize,
    },
    Io {
        operation: &'static str,
        source: io::Error,
    },
}

impl fmt::Display for SelectiveStorageError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidOutputPath => write!(formatter, "output path has no file name"),
            Self::ExistingOutput(path) => {
                write!(formatter, "output already exists: {}", path.display())
            }
            Self::ExistingStaging(path) => {
                write!(
                    formatter,
                    "staging output already exists: {}",
                    path.display()
                )
            }
            Self::ExistingPartFile(path) => {
                write!(formatter, "part file already exists: {}", path.display())
            }
            Self::ExistingMaterialization(path) => write!(
                formatter,
                "materialization path already exists: {}",
                path.display()
            ),
            Self::Layout(error) => write!(formatter, "storage layout: {error}"),
            Self::PartFile(error) => write!(formatter, "part file: {error}"),
            Self::MissingWantedFile { file_index } => {
                write!(formatter, "wanted file {file_index} is not open")
            }
            Self::PaddingInPeerBlock => {
                write!(formatter, "peer block unexpectedly includes padding")
            }
            Self::InvalidVerifiedPiece { piece_index } => {
                write!(formatter, "verified piece index {piece_index} is invalid")
            }
            Self::IncompleteSelection { piece_index } => {
                write!(formatter, "required piece {piece_index} is not verified")
            }
            Self::NotPublished => {
                write!(formatter, "selected tree is not published")
            }
            Self::AlreadyWanted { file_index } => {
                write!(formatter, "file {file_index} is already wanted")
            }
            Self::IncompleteMaterialization {
                file_index,
                piece_index,
            } => write!(
                formatter,
                "file {file_index} cannot be materialized without verified piece {piece_index}"
            ),
            Self::Io { operation, source } => write!(formatter, "{operation}: {source}"),
        }
    }
}

impl Error for SelectiveStorageError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Layout(error) => Some(error),
            Self::PartFile(error) => Some(error),
            Self::Io { source, .. } => Some(source),
            _ => None,
        }
    }
}

impl From<LayoutError> for SelectiveStorageError {
    fn from(error: LayoutError) -> Self {
        Self::Layout(error)
    }
}

impl From<PartFileError> for SelectiveStorageError {
    fn from(error: PartFileError) -> Self {
        Self::PartFile(error)
    }
}

#[derive(Debug)]
pub struct SelectiveStorage {
    output_root: PathBuf,
    staging_root: PathBuf,
    part_path: PathBuf,
    identity: PartFileIdentity,
    layout: TorrentLayout,
    selection: FileSelection,
    files: Vec<Option<File>>,
    part_file: Option<PartFile>,
    verified: Vec<bool>,
    published: bool,
}

impl SelectiveStorage {
    pub async fn create(
        output_root: PathBuf,
        metainfo: &Metainfo,
        layout: TorrentLayout,
        selection: FileSelection,
    ) -> Result<Self, SelectiveStorageError> {
        let staging_root = selective_staging_path(&output_root)?;
        let part_path = selective_part_path(&output_root)?;
        if path_exists(&output_root, "inspect selected output").await? {
            return Err(SelectiveStorageError::ExistingOutput(output_root));
        }
        if path_exists(&staging_root, "inspect selected staging").await? {
            return Err(SelectiveStorageError::ExistingStaging(staging_root));
        }
        if path_exists(&part_path, "inspect selected part file").await? {
            return Err(SelectiveStorageError::ExistingPartFile(part_path));
        }

        tokio::fs::create_dir(&staging_root)
            .await
            .map_err(|source| SelectiveStorageError::Io {
                operation: "create selected staging root",
                source,
            })?;

        let mut files = Vec::with_capacity(layout.files().len());
        for (file_index, metainfo_file) in layout.files().iter().enumerate() {
            if metainfo_file.padding || !selection.is_wanted(file_index) {
                files.push(None);
                continue;
            }
            let path = joined_path(&staging_root, &metainfo_file.path);
            let parent = path
                .parent()
                .ok_or(SelectiveStorageError::InvalidOutputPath)?;
            tokio::fs::create_dir_all(parent).await.map_err(|source| {
                SelectiveStorageError::Io {
                    operation: "create selected file parent",
                    source,
                }
            })?;
            let file = OpenOptions::new()
                .read(true)
                .write(true)
                .create_new(true)
                .open(&path)
                .await
                .map_err(|source| SelectiveStorageError::Io {
                    operation: "create selected staging file",
                    source,
                })?;
            file.set_len(metainfo_file.length).await.map_err(|source| {
                SelectiveStorageError::Io {
                    operation: "size selected staging file",
                    source,
                }
            })?;
            files.push(Some(file));
        }

        let identity = PartFileIdentity {
            info_hash: metainfo.info_hash,
            piece_count: layout.piece_count(),
            piece_length: layout.piece_length(),
            total_length: layout.total_length(),
        };
        let part_file = PartFile::create(part_path.clone(), identity).await?;
        let piece_count = layout.piece_count();

        Ok(Self {
            output_root,
            staging_root,
            part_path,
            identity,
            layout,
            selection,
            files,
            part_file: Some(part_file),
            verified: vec![false; piece_count],
            published: false,
        })
    }

    pub fn selected_bytes(&self) -> u64 {
        self.layout
            .files()
            .iter()
            .enumerate()
            .filter(|(index, file)| !file.padding && self.selection.is_wanted(*index))
            .map(|(_, file)| file.length)
            .sum()
    }

    pub fn skipped_bytes(&self) -> u64 {
        self.layout
            .files()
            .iter()
            .enumerate()
            .filter(|(index, file)| !file.padding && !self.selection.is_wanted(*index))
            .map(|(_, file)| file.length)
            .sum()
    }

    pub fn padding_bytes(&self) -> u64 {
        self.layout
            .files()
            .iter()
            .filter(|file| file.padding)
            .map(|file| file.length)
            .sum()
    }

    pub fn part_path(&self) -> &Path {
        &self.part_path
    }

    pub fn part_slots(&self) -> usize {
        self.part_file
            .as_ref()
            .map_or(0, PartFile::mapped_piece_count)
    }

    pub async fn write_block(
        &mut self,
        piece_index: u32,
        begin: u32,
        bytes: Vec<u8>,
    ) -> Result<SelectiveWriteStats, SelectiveStorageError> {
        let length = u32::try_from(bytes.len())
            .map_err(|_| SelectiveStorageError::Layout(LayoutError::ArithmeticOverflow))?;
        let segments = self
            .layout
            .segments(piece_index, begin, length, &self.selection)?;
        let mut stats = SelectiveWriteStats::default();
        for segment in segments {
            let segment_bytes = &bytes[segment.block_offset..segment.block_offset + segment.length];
            match segment.target {
                SegmentTarget::WantedFile {
                    file_index,
                    file_offset,
                } => {
                    let file = self.files[file_index]
                        .as_mut()
                        .ok_or(SelectiveStorageError::MissingWantedFile { file_index })?;
                    file.seek(SeekFrom::Start(file_offset))
                        .await
                        .map_err(|source| SelectiveStorageError::Io {
                            operation: "seek selected staging file for write",
                            source,
                        })?;
                    file.write_all(segment_bytes).await.map_err(|source| {
                        SelectiveStorageError::Io {
                            operation: "write selected staging range",
                            source,
                        }
                    })?;
                    stats.wanted_bytes += segment.length;
                }
                SegmentTarget::SkippedFile { .. } => {
                    self.part_file_mut()?
                        .write_piece_range(
                            usize::try_from(piece_index).map_err(|_| {
                                SelectiveStorageError::Layout(LayoutError::ArithmeticOverflow)
                            })?,
                            segment.piece_offset,
                            segment_bytes,
                        )
                        .await?;
                    stats.skipped_bytes += segment.length;
                }
                SegmentTarget::Padding => {
                    return Err(SelectiveStorageError::PaddingInPeerBlock);
                }
            }
        }
        Ok(stats)
    }

    pub async fn hash_piece(
        &mut self,
        piece_index: u32,
    ) -> Result<[u8; 20], SelectiveStorageError> {
        let piece_length = self.layout.piece_length_at(piece_index)?;
        let mut hasher = Sha1::new();
        let mut buffer = [0_u8; VERIFICATION_CHUNK_LENGTH];
        let mut begin = 0_u32;
        while begin < piece_length {
            let length = u32::try_from(
                usize::try_from(piece_length - begin)
                    .map_err(|_| SelectiveStorageError::Layout(LayoutError::ArithmeticOverflow))?
                    .min(buffer.len()),
            )
            .map_err(|_| SelectiveStorageError::Layout(LayoutError::ArithmeticOverflow))?;
            buffer[..length as usize].fill(0);
            let segments = self
                .layout
                .segments(piece_index, begin, length, &self.selection)?;
            for segment in segments {
                let destination =
                    &mut buffer[segment.block_offset..segment.block_offset + segment.length];
                match segment.target {
                    SegmentTarget::WantedFile {
                        file_index,
                        file_offset,
                    } => {
                        let file = self.files[file_index]
                            .as_mut()
                            .ok_or(SelectiveStorageError::MissingWantedFile { file_index })?;
                        file.seek(SeekFrom::Start(file_offset))
                            .await
                            .map_err(|source| SelectiveStorageError::Io {
                                operation: "seek selected staging file for verification",
                                source,
                            })?;
                        file.read_exact(destination).await.map_err(|source| {
                            SelectiveStorageError::Io {
                                operation: "read selected staging range for verification",
                                source,
                            }
                        })?;
                    }
                    SegmentTarget::SkippedFile { .. } => {
                        self.part_file_mut()?
                            .read_piece_range(
                                usize::try_from(piece_index).map_err(|_| {
                                    SelectiveStorageError::Layout(LayoutError::ArithmeticOverflow)
                                })?,
                                segment.piece_offset,
                                destination,
                            )
                            .await?;
                    }
                    SegmentTarget::Padding => {}
                }
            }
            hasher.update(&buffer[..length as usize]);
            begin += length;
        }
        Ok(hasher.finalize().into())
    }

    pub fn record_verified(&mut self, piece_index: usize) -> Result<(), SelectiveStorageError> {
        let piece = self
            .verified
            .get_mut(piece_index)
            .ok_or(SelectiveStorageError::InvalidVerifiedPiece { piece_index })?;
        *piece = true;
        Ok(())
    }

    pub async fn publish(&mut self) -> Result<(), SelectiveStorageError> {
        for piece_index in 0..self.layout.piece_count() {
            let piece_index_u32 = u32::try_from(piece_index)
                .map_err(|_| SelectiveStorageError::Layout(LayoutError::ArithmeticOverflow))?;
            if !self
                .layout
                .request_ranges(piece_index_u32, &self.selection)?
                .is_empty()
                && !self.verified[piece_index]
            {
                return Err(SelectiveStorageError::IncompleteSelection { piece_index });
            }
        }

        for file in self.files.iter().flatten() {
            file.sync_data()
                .await
                .map_err(|source| SelectiveStorageError::Io {
                    operation: "flush selected staging file",
                    source,
                })?;
        }
        self.part_file_mut()?.sync_payload().await?;
        self.files.iter_mut().for_each(|file| {
            file.take();
        });

        if path_exists(&self.output_root, "inspect selected output before publish").await? {
            return Err(SelectiveStorageError::ExistingOutput(
                self.output_root.clone(),
            ));
        }
        tokio::fs::rename(&self.staging_root, &self.output_root)
            .await
            .map_err(|source| SelectiveStorageError::Io {
                operation: "publish selected tree",
                source,
            })?;
        self.published = true;
        Ok(())
    }

    pub async fn reopen_part_file(&mut self) -> Result<(), SelectiveStorageError> {
        self.part_file.take();
        self.part_file = Some(PartFile::open(self.part_path.clone(), self.identity).await?);
        Ok(())
    }

    pub async fn materialize_file(
        &mut self,
        file_index: usize,
    ) -> Result<MaterializationReport, SelectiveStorageError> {
        if !self.published {
            return Err(SelectiveStorageError::NotPublished);
        }
        let metainfo_file = self
            .layout
            .files()
            .get(file_index)
            .ok_or(LayoutError::InvalidFileIndex {
                index: file_index,
                file_count: self.layout.files().len(),
            })?
            .clone();
        if self.selection.is_wanted(file_index) {
            return Err(SelectiveStorageError::AlreadyWanted { file_index });
        }
        for piece_index in self.layout.file_pieces(file_index)? {
            let piece_index_usize = usize::try_from(piece_index)
                .map_err(|_| SelectiveStorageError::Layout(LayoutError::ArithmeticOverflow))?;
            if !self.verified[piece_index_usize] {
                return Err(SelectiveStorageError::IncompleteMaterialization {
                    file_index,
                    piece_index: piece_index_usize,
                });
            }
        }

        let destination = joined_path(&self.output_root, &metainfo_file.path);
        let temporary = materialization_path(&destination)?;
        if path_exists(&destination, "inspect materialized output").await? {
            return Err(SelectiveStorageError::ExistingMaterialization(destination));
        }
        if path_exists(&temporary, "inspect materialization staging").await? {
            return Err(SelectiveStorageError::ExistingMaterialization(temporary));
        }

        let result = self
            .materialize_file_inner(file_index, &metainfo_file, &destination, &temporary)
            .await;
        if result.is_err() {
            let _ = remove_file_if_present(&temporary).await;
        }
        result
    }

    async fn materialize_file_inner(
        &mut self,
        file_index: usize,
        metainfo_file: &rstorrent_protocol::metainfo::MetainfoFile,
        destination: &Path,
        temporary: &Path,
    ) -> Result<MaterializationReport, SelectiveStorageError> {
        let parent = destination
            .parent()
            .ok_or(SelectiveStorageError::InvalidOutputPath)?;
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|source| SelectiveStorageError::Io {
                operation: "create materialized file parent",
                source,
            })?;
        let mut output = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(temporary)
            .await
            .map_err(|source| SelectiveStorageError::Io {
                operation: "create materialization staging file",
                source,
            })?;
        output
            .set_len(metainfo_file.length)
            .await
            .map_err(|source| SelectiveStorageError::Io {
                operation: "size materialization staging file",
                source,
            })?;

        let mut buffer = [0_u8; VERIFICATION_CHUNK_LENGTH];
        let mut file_offset = 0_u64;
        while file_offset < metainfo_file.length {
            let torrent_offset = metainfo_file.offset.checked_add(file_offset).ok_or(
                SelectiveStorageError::Layout(LayoutError::ArithmeticOverflow),
            )?;
            let piece_index = torrent_offset / u64::from(self.layout.piece_length());
            let piece_offset_u64 = torrent_offset % u64::from(self.layout.piece_length());
            let piece_offset = u32::try_from(piece_offset_u64)
                .map_err(|_| SelectiveStorageError::Layout(LayoutError::ArithmeticOverflow))?;
            let piece_remaining = u64::from(self.layout.piece_length()) - piece_offset_u64;
            let length = usize::try_from(
                (metainfo_file.length - file_offset)
                    .min(piece_remaining)
                    .min(buffer.len() as u64),
            )
            .map_err(|_| SelectiveStorageError::Layout(LayoutError::ArithmeticOverflow))?;
            self.part_file_mut()?
                .read_piece_range(
                    usize::try_from(piece_index).map_err(|_| {
                        SelectiveStorageError::Layout(LayoutError::ArithmeticOverflow)
                    })?,
                    piece_offset,
                    &mut buffer[..length],
                )
                .await?;
            output
                .write_all(&buffer[..length])
                .await
                .map_err(|source| SelectiveStorageError::Io {
                    operation: "write materialization staging file",
                    source,
                })?;
            file_offset += length as u64;
        }
        output
            .sync_data()
            .await
            .map_err(|source| SelectiveStorageError::Io {
                operation: "flush materialization staging file",
                source,
            })?;
        drop(output);
        tokio::fs::rename(temporary, destination)
            .await
            .map_err(|source| SelectiveStorageError::Io {
                operation: "publish materialized file",
                source,
            })?;

        let slots_before = self.part_slots();
        self.selection.set_wanted(&self.layout, file_index, true)?;
        for piece_index in self.layout.file_pieces(file_index)? {
            if !self
                .layout
                .piece_has_skipped_file(piece_index, &self.selection)?
            {
                self.part_file_mut()?
                    .release_piece(usize::try_from(piece_index).map_err(|_| {
                        SelectiveStorageError::Layout(LayoutError::ArithmeticOverflow)
                    })?)
                    .await?;
            }
        }
        Ok(MaterializationReport {
            file_index,
            bytes: metainfo_file.length,
            slots_before,
            slots_after: self.part_slots(),
        })
    }

    fn part_file_mut(&mut self) -> Result<&mut PartFile, SelectiveStorageError> {
        self.part_file
            .as_mut()
            .ok_or(SelectiveStorageError::NotPublished)
    }
}

pub fn selective_staging_path(output_root: &Path) -> Result<PathBuf, SelectiveStorageError> {
    sibling_with_suffix(output_root, ".rstorrent-staging")
}

pub fn selective_part_path(output_root: &Path) -> Result<PathBuf, SelectiveStorageError> {
    sibling_with_suffix(output_root, ".rstorrent-parts")
}

fn materialization_path(destination: &Path) -> Result<PathBuf, SelectiveStorageError> {
    sibling_with_suffix(destination, ".rstorrent-materializing")
}

fn sibling_with_suffix(path: &Path, suffix: &str) -> Result<PathBuf, SelectiveStorageError> {
    let file_name = path
        .file_name()
        .ok_or(SelectiveStorageError::InvalidOutputPath)?;
    let mut sibling = OsString::from(".");
    sibling.push(file_name);
    sibling.push(suffix);
    Ok(path.with_file_name(sibling))
}

fn joined_path(root: &Path, components: &[String]) -> PathBuf {
    let mut path = root.to_path_buf();
    path.extend(components);
    path
}

async fn path_exists(path: &Path, operation: &'static str) -> Result<bool, SelectiveStorageError> {
    tokio::fs::try_exists(path)
        .await
        .map_err(|source| SelectiveStorageError::Io { operation, source })
}

async fn remove_file_if_present(path: &Path) -> Result<(), io::Error> {
    match tokio::fs::remove_file(path).await {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

pub async fn remove_selective_staging_if_present(output_root: &Path) -> Result<(), io::Error> {
    let staging = selective_staging_path(output_root)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "invalid output root"))?;
    match tokio::fs::remove_dir_all(staging).await {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

pub async fn remove_selective_part_if_present(output_root: &Path) -> Result<(), io::Error> {
    let part = selective_part_path(output_root)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "invalid output root"))?;
    remove_file_if_present(&part).await
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicU64, Ordering};

    use rstorrent_protocol::metainfo::{Metainfo, MetainfoFile, MetainfoMode};
    use rstorrent_protocol::storage_layout::{FileSelection, TorrentLayout};
    use sha1::{Digest, Sha1};

    use super::{
        SelectiveStorage, SelectiveStorageError, materialization_path,
        remove_selective_part_if_present, remove_selective_staging_if_present, selective_part_path,
        selective_staging_path,
    };

    static TEST_SEQUENCE: AtomicU64 = AtomicU64::new(0);

    fn test_path(name: &str) -> PathBuf {
        let sequence = TEST_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!(
            "rstorrent-selective-storage-test-{}-{sequence}-{name}",
            std::process::id()
        ))
    }

    fn fixture() -> Metainfo {
        let lengths = [20_000, 50_000, 7_000, 18_000, 0, 3_304, 35_000];
        let paths = [
            &["wanted", "start.bin"][..],
            &["skip", "large.bin"][..],
            &["later.bin"][..],
            &["wanted", "end.bin"][..],
            &["wanted", "empty.bin"][..],
            &[".pad", "3304"][..],
            &["tail.bin"][..],
        ];
        let mut offset = 0_u64;
        let files = lengths
            .into_iter()
            .enumerate()
            .map(|(index, length)| {
                let file = MetainfoFile {
                    path: paths[index].iter().map(|part| (*part).to_owned()).collect(),
                    length,
                    offset,
                    padding: index == 5,
                };
                offset += length;
                file
            })
            .collect();
        Metainfo {
            info_hash: [1; 20],
            piece_hashes: vec![[2; 20]; 5],
            piece_length: 32_768,
            total_length: 133_304,
            name: "fixture".to_owned(),
            mode: MetainfoMode::MultiFile,
            files,
        }
    }

    fn torrent_bytes(metainfo: &Metainfo) -> Vec<u8> {
        let mut bytes: Vec<u8> = (0..metainfo.total_length)
            .map(|offset| (offset % 251) as u8)
            .collect();
        for file in metainfo.files.iter().filter(|file| file.padding) {
            bytes[file.offset as usize..(file.offset + file.length) as usize].fill(0);
        }
        bytes
    }

    async fn clean(output: &Path) {
        let _ = tokio::fs::remove_dir_all(output).await;
        let _ = remove_selective_staging_if_present(output).await;
        let _ = remove_selective_part_if_present(output).await;
    }

    #[tokio::test]
    async fn stages_hashes_publishes_reopens_and_materializes() {
        let output = test_path("fixture");
        clean(&output).await;
        let metainfo = fixture();
        let layout = TorrentLayout::from_metainfo(&metainfo);
        let selection = FileSelection::new(&layout, &[1, 2]).expect("selection");
        let bytes = torrent_bytes(&metainfo);
        let mut storage =
            SelectiveStorage::create(output.clone(), &metainfo, layout.clone(), selection)
                .await
                .expect("create selected storage");

        assert_eq!(storage.selected_bytes(), 73_000);
        assert_eq!(storage.skipped_bytes(), 57_000);
        assert_eq!(storage.padding_bytes(), 3_304);
        assert!(!tokio::fs::try_exists(&output).await.expect("output state"));
        let staging = selective_staging_path(&output).expect("staging path");
        assert!(
            tokio::fs::try_exists(staging.join("wanted/start.bin"))
                .await
                .expect("wanted staging")
        );
        assert!(
            !tokio::fs::try_exists(staging.join("skip/large.bin"))
                .await
                .expect("skipped staging")
        );
        assert!(
            !tokio::fs::try_exists(staging.join(".pad/3304"))
                .await
                .expect("padding staging")
        );
        assert!(matches!(
            storage.publish().await,
            Err(SelectiveStorageError::IncompleteSelection { piece_index: 0 })
        ));
        assert!(!tokio::fs::try_exists(&output).await.expect("output state"));
        assert!(matches!(
            storage.hash_piece(0).await,
            Err(SelectiveStorageError::PartFile(
                crate::part_file::PartFileError::MissingSlot { piece_index: 0 }
            ))
        ));

        let mut wanted_written = 0;
        let mut skipped_written = 0;
        for piece_index in [0_u32, 2, 3, 4] {
            for request in layout
                .request_ranges(piece_index, &storage.selection)
                .expect("requests")
            {
                let torrent_offset =
                    piece_index as usize * layout.piece_length() as usize + request.begin as usize;
                let block =
                    bytes[torrent_offset..torrent_offset + request.length as usize].to_vec();
                let stats = storage
                    .write_block(piece_index, request.begin, block)
                    .await
                    .expect("write mapped block");
                wanted_written += stats.wanted_bytes;
                skipped_written += stats.skipped_bytes;
            }
            let piece_start = piece_index as usize * layout.piece_length() as usize;
            let piece_length = layout.piece_length_at(piece_index).expect("piece length") as usize;
            let expected: [u8; 20] =
                Sha1::digest(&bytes[piece_start..piece_start + piece_length]).into();
            assert_eq!(
                storage.hash_piece(piece_index).await.expect("mixed hash"),
                expected
            );
            storage
                .record_verified(piece_index as usize)
                .expect("record verified");
        }
        assert_eq!(wanted_written, 73_000);
        assert_eq!(skipped_written, 24_232);
        assert_eq!(storage.part_slots(), 2);

        storage.publish().await.expect("publish selected tree");
        assert!(
            tokio::fs::try_exists(output.join("wanted/empty.bin"))
                .await
                .expect("empty output")
        );
        assert!(
            !tokio::fs::try_exists(output.join("skip/large.bin"))
                .await
                .expect("skipped output")
        );
        assert!(
            !tokio::fs::try_exists(output.join(".pad/3304"))
                .await
                .expect("padding output")
        );
        assert!(
            !tokio::fs::try_exists(&staging)
                .await
                .expect("staging removed")
        );

        storage.reopen_part_file().await.expect("reopen part file");
        let report = storage
            .materialize_file(2)
            .await
            .expect("materialize later file");
        assert_eq!(report.bytes, 7_000);
        assert_eq!(report.slots_before, 2);
        assert_eq!(report.slots_after, 2);
        assert_eq!(
            tokio::fs::read(output.join("later.bin"))
                .await
                .expect("materialized bytes"),
            bytes[70_000..77_000]
        );
        assert!(
            tokio::fs::try_exists(selective_part_path(&output).expect("part path"))
                .await
                .expect("part exists")
        );
        clean(&output).await;
    }

    #[tokio::test]
    async fn releases_a_slot_after_the_last_skipped_file_is_materialized() {
        let output = test_path("release");
        clean(&output).await;
        let metainfo = fixture();
        let layout = TorrentLayout::from_metainfo(&metainfo);
        let selection = FileSelection::new(&layout, &[2]).expect("selection");
        let mut storage = SelectiveStorage::create(output.clone(), &metainfo, layout, selection)
            .await
            .expect("create selected storage");

        storage
            .write_block(2, 0, vec![0; 16_384])
            .await
            .expect("store skipped file range");
        assert_eq!(storage.part_slots(), 1);
        for piece in 0..5 {
            storage.record_verified(piece).expect("record verification");
        }
        storage.publish().await.expect("publish selected tree");
        storage.reopen_part_file().await.expect("reopen part file");
        let report = storage
            .materialize_file(2)
            .await
            .expect("materialize final skipped file");
        assert_eq!(report.slots_before, 1);
        assert_eq!(report.slots_after, 0);

        storage
            .reopen_part_file()
            .await
            .expect("reopen released map");
        assert_eq!(storage.part_slots(), 0);
        clean(&output).await;
    }

    #[tokio::test]
    async fn rejects_incomplete_materialization_without_final_or_partial_file() {
        let output = test_path("incomplete");
        clean(&output).await;
        let metainfo = fixture();
        let layout = TorrentLayout::from_metainfo(&metainfo);
        let selection = FileSelection::new(&layout, &[1, 2]).expect("selection");
        let mut storage = SelectiveStorage::create(output.clone(), &metainfo, layout, selection)
            .await
            .expect("create selected storage");
        for piece in [0, 2, 3, 4] {
            storage.record_verified(piece).expect("record verification");
        }
        storage.publish().await.expect("publish selected tree");
        assert!(matches!(
            storage.materialize_file(1).await,
            Err(SelectiveStorageError::IncompleteMaterialization {
                file_index: 1,
                piece_index: 1,
            })
        ));
        let final_path = output.join("skip/large.bin");
        assert!(
            !tokio::fs::try_exists(&final_path)
                .await
                .expect("final state")
        );
        assert!(
            !tokio::fs::try_exists(materialization_path(&final_path).expect("temporary path"))
                .await
                .expect("temporary state")
        );
        clean(&output).await;
    }

    #[tokio::test]
    async fn refuses_and_preserves_every_preexisting_artifact() {
        let output = test_path("existing");
        clean(&output).await;
        let metainfo = fixture();
        let layout = TorrentLayout::from_metainfo(&metainfo);
        let selection = FileSelection::new(&layout, &[1, 2]).expect("selection");

        tokio::fs::create_dir(&output)
            .await
            .expect("create existing output");
        assert!(matches!(
            SelectiveStorage::create(output.clone(), &metainfo, layout.clone(), selection.clone())
                .await,
            Err(SelectiveStorageError::ExistingOutput(_))
        ));
        assert!(tokio::fs::try_exists(&output).await.expect("output state"));
        tokio::fs::remove_dir(&output)
            .await
            .expect("remove existing output");

        let staging = selective_staging_path(&output).expect("staging path");
        tokio::fs::create_dir(&staging)
            .await
            .expect("create existing staging");
        assert!(matches!(
            SelectiveStorage::create(output.clone(), &metainfo, layout.clone(), selection.clone())
                .await,
            Err(SelectiveStorageError::ExistingStaging(_))
        ));
        assert!(
            tokio::fs::try_exists(&staging)
                .await
                .expect("staging state")
        );
        tokio::fs::remove_dir(&staging)
            .await
            .expect("remove existing staging");

        let part = selective_part_path(&output).expect("part path");
        tokio::fs::write(&part, b"owned elsewhere")
            .await
            .expect("create existing part");
        assert!(matches!(
            SelectiveStorage::create(output.clone(), &metainfo, layout, selection).await,
            Err(SelectiveStorageError::ExistingPartFile(_))
        ));
        assert_eq!(
            tokio::fs::read(&part).await.expect("preserved part"),
            b"owned elsewhere"
        );
        clean(&output).await;
    }
}
