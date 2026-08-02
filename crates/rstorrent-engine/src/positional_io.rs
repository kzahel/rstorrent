use std::fs::File;
use std::io;

pub(crate) fn read_exact_at(file: &File, bytes: &mut [u8], offset: u64) -> io::Result<()> {
    read_exact_at_with(bytes, offset, |bytes, offset| {
        positional_read(file, bytes, offset)
    })
}

pub(crate) fn write_all_at(file: &File, bytes: &[u8], offset: u64) -> io::Result<()> {
    write_all_at_with(bytes, offset, |bytes, offset| {
        positional_write(file, bytes, offset)
    })
}

fn read_exact_at_with(
    mut bytes: &mut [u8],
    mut offset: u64,
    mut read: impl FnMut(&mut [u8], u64) -> io::Result<usize>,
) -> io::Result<()> {
    while !bytes.is_empty() {
        match read(bytes, offset) {
            Ok(0) => {
                return Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "positional read ended before the requested range",
                ));
            }
            Ok(consumed) => {
                offset = advance_offset(offset, consumed)?;
                bytes = &mut bytes[consumed..];
            }
            Err(source) if source.kind() == io::ErrorKind::Interrupted => {}
            Err(source) => return Err(source),
        }
    }
    Ok(())
}

fn write_all_at_with(
    mut bytes: &[u8],
    mut offset: u64,
    mut write: impl FnMut(&[u8], u64) -> io::Result<usize>,
) -> io::Result<()> {
    while !bytes.is_empty() {
        match write(bytes, offset) {
            Ok(0) => {
                return Err(io::Error::new(
                    io::ErrorKind::WriteZero,
                    "positional write made no progress",
                ));
            }
            Ok(consumed) => {
                offset = advance_offset(offset, consumed)?;
                bytes = &bytes[consumed..];
            }
            Err(source) if source.kind() == io::ErrorKind::Interrupted => {}
            Err(source) => return Err(source),
        }
    }
    Ok(())
}

fn advance_offset(offset: u64, length: usize) -> io::Result<u64> {
    offset
        .checked_add(
            u64::try_from(length).map_err(|_| io::Error::other("positional length overflow"))?,
        )
        .ok_or_else(|| io::Error::other("positional offset overflow"))
}

#[cfg(unix)]
fn positional_read(file: &File, bytes: &mut [u8], offset: u64) -> io::Result<usize> {
    use std::os::unix::fs::FileExt;

    file.read_at(bytes, offset)
}

#[cfg(windows)]
fn positional_read(file: &File, bytes: &mut [u8], offset: u64) -> io::Result<usize> {
    use std::os::windows::fs::FileExt;

    file.seek_read(bytes, offset)
}

#[cfg(not(any(unix, windows)))]
fn positional_read(_file: &File, _bytes: &mut [u8], _offset: u64) -> io::Result<usize> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "positional file reads are unsupported on this platform",
    ))
}

#[cfg(unix)]
fn positional_write(file: &File, bytes: &[u8], offset: u64) -> io::Result<usize> {
    use std::os::unix::fs::FileExt;

    file.write_at(bytes, offset)
}

#[cfg(windows)]
fn positional_write(file: &File, bytes: &[u8], offset: u64) -> io::Result<usize> {
    use std::os::windows::fs::FileExt;

    file.seek_write(bytes, offset)
}

#[cfg(not(any(unix, windows)))]
fn positional_write(_file: &File, _bytes: &[u8], _offset: u64) -> io::Result<usize> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "positional file writes are unsupported on this platform",
    ))
}

#[cfg(test)]
mod tests {
    use std::fs::OpenOptions;
    use std::io;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    use super::{read_exact_at, read_exact_at_with, write_all_at, write_all_at_with};

    static TEST_SEQUENCE: AtomicU64 = AtomicU64::new(0);

    fn test_path() -> PathBuf {
        let sequence = TEST_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!(
            "rstorrent-positional-io-test-{}-{sequence}",
            std::process::id()
        ))
    }

    #[test]
    fn writes_out_of_order_and_reads_unaligned_ranges() {
        let path = test_path();
        let file = OpenOptions::new()
            .create_new(true)
            .read(true)
            .write(true)
            .open(&path)
            .expect("create positional file");
        file.set_len(12).expect("size positional file");

        write_all_at(&file, b"world", 6).expect("write tail");
        write_all_at(&file, b"hello ", 0).expect("write prefix");
        let mut bytes = [0_u8; 7];
        read_exact_at(&file, &mut bytes, 3).expect("read unaligned range");
        assert_eq!(&bytes, b"lo worl");

        drop(file);
        std::fs::remove_file(path).expect("remove positional file");
    }

    #[test]
    fn retries_interruptions_and_advances_partial_operations() {
        let mut written_offsets = Vec::new();
        let mut write_calls = 0;
        write_all_at_with(b"abcdef", 9, |bytes, offset| {
            write_calls += 1;
            if write_calls == 1 {
                return Err(io::Error::from(io::ErrorKind::Interrupted));
            }
            written_offsets.push(offset);
            Ok(bytes.len().min(2))
        })
        .expect("retry partial writes");
        assert_eq!(written_offsets, [9, 11, 13]);

        let source = b"abcdef";
        let mut output = [0_u8; 6];
        let mut read_calls = 0;
        read_exact_at_with(&mut output, 20, |bytes, offset| {
            read_calls += 1;
            if read_calls == 1 {
                return Err(io::Error::from(io::ErrorKind::Interrupted));
            }
            let source_offset = usize::try_from(offset - 20).expect("source offset");
            let length = bytes.len().min(2);
            bytes[..length].copy_from_slice(&source[source_offset..source_offset + length]);
            Ok(length)
        })
        .expect("retry partial reads");
        assert_eq!(&output, source);
    }

    #[test]
    fn reports_zero_progress_short_reads_and_offset_overflow() {
        let write_error = write_all_at_with(b"x", 0, |_bytes, _offset| Ok(0))
            .expect_err("zero write is terminal");
        assert_eq!(write_error.kind(), io::ErrorKind::WriteZero);

        let mut byte = [0_u8; 1];
        let read_error = read_exact_at_with(&mut byte, 0, |_bytes, _offset| Ok(0))
            .expect_err("zero read is terminal");
        assert_eq!(read_error.kind(), io::ErrorKind::UnexpectedEof);

        let overflow = write_all_at_with(b"xy", u64::MAX, |_bytes, _offset| Ok(1))
            .expect_err("offset overflow is terminal");
        assert_eq!(overflow.kind(), io::ErrorKind::Other);
    }
}
