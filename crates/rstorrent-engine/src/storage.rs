use std::error::Error;
use std::ffi::OsString;
use std::fmt;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use sha1::{Digest, Sha1};
use tokio::fs::{File, OpenOptions};

use crate::positional_io::{read_exact_at, write_all_at};

pub const VERIFICATION_CHUNK_LENGTH: usize = 16 * 1024;

#[derive(Debug)]
pub enum StorageError {
    InvalidOutputPath,
    ExistingOutput(PathBuf),
    ExistingStaging(PathBuf),
    BlockOutOfRange {
        begin: u64,
        length: usize,
        file_length: u64,
    },
    Io {
        operation: &'static str,
        source: io::Error,
    },
}

impl fmt::Display for StorageError {
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
            Self::BlockOutOfRange {
                begin,
                length,
                file_length,
            } => write!(
                formatter,
                "block at {begin} with length {length} exceeds file length {file_length}"
            ),
            Self::Io { operation, source } => write!(formatter, "{operation}: {source}"),
        }
    }
}

impl Error for StorageError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            _ => None,
        }
    }
}

#[derive(Debug)]
pub struct StagingFile {
    file: Option<File>,
    positional: Option<Arc<std::fs::File>>,
    staging_path: PathBuf,
    output_path: PathBuf,
    file_length: u64,
}

#[derive(Clone, Debug)]
pub(crate) struct StagingWritePlan {
    file: Arc<std::fs::File>,
    begin: u64,
    payload: Arc<Vec<u8>>,
}

#[derive(Clone, Debug)]
pub(crate) struct StagingHashPlan {
    file: Arc<std::fs::File>,
    begin: u64,
    length: u32,
}

impl StagingWritePlan {
    pub(crate) async fn execute(self) -> Result<(), StorageError> {
        tokio::task::spawn_blocking(move || write_all_at(&self.file, &self.payload, self.begin))
            .await
            .map_err(|source| StorageError::Io {
                operation: "join positional staging write",
                source: io::Error::other(source),
            })?
            .map_err(|source| StorageError::Io {
                operation: "write unverified block",
                source,
            })
    }
}

impl StagingHashPlan {
    pub(crate) async fn execute(self) -> Result<[u8; 20], StorageError> {
        tokio::task::spawn_blocking(move || {
            let mut hasher = Sha1::new();
            let mut buffer = [0_u8; VERIFICATION_CHUNK_LENGTH];
            let mut remaining = self.length as usize;
            let mut offset = self.begin;
            while remaining > 0 {
                let chunk_length = remaining.min(buffer.len());
                read_exact_at(&self.file, &mut buffer[..chunk_length], offset)?;
                hasher.update(&buffer[..chunk_length]);
                remaining -= chunk_length;
                offset = offset
                    .checked_add(chunk_length as u64)
                    .ok_or_else(|| io::Error::other("verification offset overflow"))?;
            }
            Ok::<[u8; 20], io::Error>(hasher.finalize().into())
        })
        .await
        .map_err(|source| StorageError::Io {
            operation: "join positional staging verification",
            source: io::Error::other(source),
        })?
        .map_err(|source| StorageError::Io {
            operation: "read staging output for verification",
            source,
        })
    }
}

impl StagingFile {
    pub async fn create(output_path: PathBuf, file_length: u64) -> Result<Self, StorageError> {
        let staging_path = staging_path(&output_path)?;
        if path_exists(&output_path).await? {
            return Err(StorageError::ExistingOutput(output_path));
        }
        if path_exists(&staging_path).await? {
            return Err(StorageError::ExistingStaging(staging_path));
        }

        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .open(&staging_path)
            .await
            .map_err(|source| StorageError::Io {
                operation: "create staging output",
                source,
            })?;
        file.set_len(file_length)
            .await
            .map_err(|source| StorageError::Io {
                operation: "size staging output",
                source,
            })?;
        let positional = Arc::new(
            file.try_clone()
                .await
                .map_err(|source| StorageError::Io {
                    operation: "retain staging output for positional access",
                    source,
                })?
                .into_std()
                .await,
        );

        Ok(Self {
            file: Some(file),
            positional: Some(positional),
            staging_path,
            output_path,
            file_length,
        })
    }

    #[cfg(test)]
    async fn write_block(&self, begin: u64, bytes: Vec<u8>) -> Result<(), StorageError> {
        let plan = self.plan_write(begin, bytes)?;
        plan.execute().await
    }

    pub(crate) fn plan_write(
        &self,
        begin: u64,
        bytes: Vec<u8>,
    ) -> Result<StagingWritePlan, StorageError> {
        let end = begin
            .checked_add(bytes.len() as u64)
            .filter(|end| *end <= self.file_length)
            .ok_or(StorageError::BlockOutOfRange {
                begin,
                length: bytes.len(),
                file_length: self.file_length,
            })?;
        debug_assert!(end <= self.file_length);
        Ok(StagingWritePlan {
            file: self
                .positional
                .as_ref()
                .expect("positional staging handle is present until finalization")
                .clone(),
            begin,
            payload: Arc::new(bytes),
        })
    }

    #[cfg(test)]
    async fn hash_piece(&self, begin: u64, length: u32) -> Result<[u8; 20], StorageError> {
        self.plan_hash(begin, length)?.execute().await
    }

    pub(crate) fn plan_hash(
        &self,
        begin: u64,
        length: u32,
    ) -> Result<StagingHashPlan, StorageError> {
        let end = begin
            .checked_add(u64::from(length))
            .filter(|end| *end <= self.file_length)
            .ok_or(StorageError::BlockOutOfRange {
                begin,
                length: length as usize,
                file_length: self.file_length,
            })?;
        debug_assert!(end <= self.file_length);
        Ok(StagingHashPlan {
            file: self
                .positional
                .as_ref()
                .expect("positional staging handle is present until finalization")
                .clone(),
            begin,
            length,
        })
    }

    pub async fn finalize(mut self) -> Result<(), StorageError> {
        self.file_mut()
            .sync_data()
            .await
            .map_err(|source| StorageError::Io {
                operation: "flush verified staging output",
                source,
            })?;
        self.file.take();
        self.positional.take();

        if path_exists(&self.output_path).await? {
            return Err(StorageError::ExistingOutput(self.output_path));
        }
        tokio::fs::rename(&self.staging_path, &self.output_path)
            .await
            .map_err(|source| StorageError::Io {
                operation: "publish verified output",
                source,
            })
    }

    fn file_mut(&mut self) -> &mut File {
        self.file
            .as_mut()
            .expect("staging file is present until finalization")
    }
}

pub fn staging_path(output_path: &Path) -> Result<PathBuf, StorageError> {
    let file_name = output_path
        .file_name()
        .ok_or(StorageError::InvalidOutputPath)?;
    let mut staging_name = OsString::from(".");
    staging_name.push(file_name);
    staging_name.push(".rstorrent-part");
    Ok(output_path.with_file_name(staging_name))
}

pub async fn remove_staging_if_present(path: &Path) -> Result<(), io::Error> {
    match tokio::fs::remove_file(path).await {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

async fn path_exists(path: &Path) -> Result<bool, StorageError> {
    tokio::fs::try_exists(path)
        .await
        .map_err(|source| StorageError::Io {
            operation: "inspect output path",
            source,
        })
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    use sha1::{Digest, Sha1};

    use super::{
        StagingFile, StorageError, VERIFICATION_CHUNK_LENGTH, remove_staging_if_present,
        staging_path,
    };

    static TEST_SEQUENCE: AtomicU64 = AtomicU64::new(0);

    fn test_path(name: &str) -> PathBuf {
        let sequence = TEST_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!(
            "rstorrent-storage-test-{}-{sequence}-{name}",
            std::process::id()
        ))
    }

    async fn clean(path: &PathBuf) {
        let _ = tokio::fs::remove_file(path).await;
        if let Ok(staging) = staging_path(path) {
            let _ = remove_staging_if_present(&staging).await;
        }
    }

    #[tokio::test]
    async fn writes_out_of_order_hashes_in_chunks_and_publishes_once() {
        let output = test_path("verified.bin");
        clean(&output).await;
        let length = 3 * VERIFICATION_CHUNK_LENGTH;
        let bytes: Vec<u8> = (0..length).map(|offset| (offset % 251) as u8).collect();
        let expected_hash: [u8; 20] = Sha1::digest(&bytes).into();
        let storage = StagingFile::create(output.clone(), length as u64)
            .await
            .expect("create staging");

        storage
            .write_block(
                (2 * VERIFICATION_CHUNK_LENGTH) as u64,
                bytes[2 * VERIFICATION_CHUNK_LENGTH..].to_vec(),
            )
            .await
            .expect("last block");
        storage
            .write_block(0, bytes[..VERIFICATION_CHUNK_LENGTH].to_vec())
            .await
            .expect("first block");
        storage
            .write_block(
                VERIFICATION_CHUNK_LENGTH as u64,
                bytes[VERIFICATION_CHUNK_LENGTH..2 * VERIFICATION_CHUNK_LENGTH].to_vec(),
            )
            .await
            .expect("middle block");

        assert!(!tokio::fs::try_exists(&output).await.expect("output status"));
        assert_eq!(
            storage
                .hash_piece(0, length as u32)
                .await
                .expect("streamed hash"),
            expected_hash
        );
        storage.finalize().await.expect("publish verified file");
        assert_eq!(
            tokio::fs::read(&output).await.expect("published bytes"),
            bytes
        );
        clean(&output).await;
    }

    #[tokio::test]
    async fn rejects_existing_paths_and_out_of_range_writes() {
        let output = test_path("existing.bin");
        clean(&output).await;
        tokio::fs::write(&output, b"existing")
            .await
            .expect("create existing output");
        assert!(matches!(
            StagingFile::create(output.clone(), 1).await,
            Err(StorageError::ExistingOutput(_))
        ));
        tokio::fs::remove_file(&output)
            .await
            .expect("remove existing output");

        let staging = staging_path(&output).expect("staging path");
        tokio::fs::write(&staging, b"existing")
            .await
            .expect("create existing staging");
        assert!(matches!(
            StagingFile::create(output.clone(), 1).await,
            Err(StorageError::ExistingStaging(_))
        ));
        tokio::fs::remove_file(&staging)
            .await
            .expect("remove existing staging");

        let storage = StagingFile::create(output.clone(), 1)
            .await
            .expect("create staging");
        assert!(matches!(
            storage.write_block(1, vec![1]).await,
            Err(StorageError::BlockOutOfRange { .. })
        ));
        drop(storage);
        clean(&output).await;
    }

    #[tokio::test]
    async fn short_verification_read_is_an_io_error() {
        let output = test_path("short.bin");
        clean(&output).await;
        let mut storage = StagingFile::create(output.clone(), 32)
            .await
            .expect("create staging");
        storage
            .file_mut()
            .set_len(1)
            .await
            .expect("truncate behind logical storage length");

        assert!(matches!(
            storage.hash_piece(0, 32).await,
            Err(StorageError::Io {
                operation: "read staging output for verification",
                ..
            })
        ));
        drop(storage);
        clean(&output).await;
    }
}
