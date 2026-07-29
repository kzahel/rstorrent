use std::ffi::c_void;
use std::io;
use std::os::fd::RawFd;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread::{self, JoinHandle};
use std::time::Duration;

const BLOCK_LENGTH: usize = 16 * 1024;
const HEAD_SEED: u8 = 11;
const TAIL_SEED: u8 = 23;
const MATERIALIZED_SEED: u8 = 41;

const ERROR_DUP: i64 = 1 << 0;
const ERROR_TRUNCATE: i64 = 1 << 1;
const ERROR_HEAD_WRITE: i64 = 1 << 2;
const ERROR_TAIL_WRITE: i64 = 1 << 3;
const ERROR_SYNC: i64 = 1 << 4;
const ERROR_HEAD_READ: i64 = 1 << 5;
const ERROR_TAIL_READ: i64 = 1 << 6;
const ERROR_STAT: i64 = 1 << 7;
const ERROR_LENGTH: i64 = 1 << 8;

type JNIEnv = *mut c_void;
type JClass = *mut c_void;

struct CancelJob {
    cancel: Arc<AtomicBool>,
    written: Arc<AtomicU64>,
    handle: JoinHandle<Result<u64, io::Error>>,
}

static CANCEL_JOB: OnceLock<Mutex<Option<CancelJob>>> = OnceLock::new();

#[unsafe(no_mangle)]
pub extern "system" fn Java_org_rstorrent_storageprobe_NativeProbe_runSparse(
    _environment: JNIEnv,
    _class: JClass,
    borrowed_fd: i32,
    logical_length: i64,
) -> i64 {
    if logical_length < BLOCK_LENGTH as i64 {
        return ERROR_LENGTH;
    }
    let fd = unsafe { libc::dup(borrowed_fd) };
    if fd < 0 {
        return ERROR_DUP;
    }
    let result = run_sparse(fd, logical_length);
    unsafe {
        libc::close(fd);
    }
    result
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_org_rstorrent_storageprobe_NativeProbe_truncateSparse(
    _environment: JNIEnv,
    _class: JClass,
    borrowed_fd: i32,
    logical_length: i64,
) -> i64 {
    if logical_length < BLOCK_LENGTH as i64 {
        return ERROR_LENGTH;
    }
    with_duplicate(borrowed_fd, |fd| {
        if unsafe { libc::ftruncate(fd, logical_length as libc::off_t) } != 0 {
            ERROR_TRUNCATE
        } else {
            0
        }
    })
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_org_rstorrent_storageprobe_NativeProbe_writeSparseMarkers(
    _environment: JNIEnv,
    _class: JClass,
    borrowed_fd: i32,
    logical_length: i64,
) -> i64 {
    if logical_length < BLOCK_LENGTH as i64 {
        return ERROR_LENGTH;
    }
    with_duplicate(borrowed_fd, |fd| {
        let mut result = 0;
        if pwrite_all(fd, &pattern(HEAD_SEED), 0).is_err() {
            result |= ERROR_HEAD_WRITE;
        }
        let tail_offset = logical_length - BLOCK_LENGTH as i64;
        if pwrite_all(fd, &pattern(TAIL_SEED), tail_offset).is_err() {
            result |= ERROR_TAIL_WRITE;
        }
        result
    })
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_org_rstorrent_storageprobe_NativeProbe_syncDescriptor(
    _environment: JNIEnv,
    _class: JClass,
    borrowed_fd: i32,
) -> i64 {
    with_duplicate(borrowed_fd, |fd| {
        if unsafe { libc::fsync(fd) } != 0 {
            ERROR_SYNC
        } else {
            0
        }
    })
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_org_rstorrent_storageprobe_NativeProbe_verifySparse(
    _environment: JNIEnv,
    _class: JClass,
    borrowed_fd: i32,
    logical_length: i64,
) -> i64 {
    let fd = unsafe { libc::dup(borrowed_fd) };
    if fd < 0 {
        return ERROR_DUP;
    }
    let result = verify_sparse(fd, logical_length);
    unsafe {
        libc::close(fd);
    }
    result
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_org_rstorrent_storageprobe_NativeProbe_writeMaterialized(
    _environment: JNIEnv,
    _class: JClass,
    borrowed_fd: i32,
) -> i64 {
    let fd = unsafe { libc::dup(borrowed_fd) };
    if fd < 0 {
        return ERROR_DUP;
    }
    let buffer = pattern(MATERIALIZED_SEED);
    let mut result = 0;
    if unsafe { libc::ftruncate(fd, BLOCK_LENGTH as libc::off_t) } != 0 {
        result |= ERROR_TRUNCATE;
    } else if pwrite_all(fd, &buffer, 0).is_err() {
        result |= ERROR_HEAD_WRITE;
    } else if unsafe { libc::fsync(fd) } != 0 {
        result |= ERROR_SYNC;
    }
    unsafe {
        libc::close(fd);
    }
    result
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_org_rstorrent_storageprobe_NativeProbe_verifyMaterialized(
    _environment: JNIEnv,
    _class: JClass,
    borrowed_fd: i32,
) -> i64 {
    let fd = unsafe { libc::dup(borrowed_fd) };
    if fd < 0 {
        return ERROR_DUP;
    }
    let expected = pattern(MATERIALIZED_SEED);
    let mut actual = [0_u8; BLOCK_LENGTH];
    let mut result = 0;
    if pread_exact(fd, &mut actual, 0).is_err() || actual != expected {
        result |= ERROR_HEAD_READ;
    }
    unsafe {
        libc::close(fd);
    }
    result
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_org_rstorrent_storageprobe_NativeProbe_duplicate(
    _environment: JNIEnv,
    _class: JClass,
    borrowed_fd: i32,
) -> i32 {
    unsafe { libc::dup(borrowed_fd) }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_org_rstorrent_storageprobe_NativeProbe_verifyOwned(
    _environment: JNIEnv,
    _class: JClass,
    owned_fd: i32,
    logical_length: i64,
) -> i64 {
    verify_sparse(owned_fd, logical_length)
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_org_rstorrent_storageprobe_NativeProbe_closeOwned(
    _environment: JNIEnv,
    _class: JClass,
    owned_fd: i32,
) -> i32 {
    unsafe { libc::close(owned_fd) }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_org_rstorrent_storageprobe_NativeProbe_logicalBytes(
    _environment: JNIEnv,
    _class: JClass,
    borrowed_fd: i32,
) -> i64 {
    stat_value(borrowed_fd, |stat| stat.st_size)
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_org_rstorrent_storageprobe_NativeProbe_allocatedBytes(
    _environment: JNIEnv,
    _class: JClass,
    borrowed_fd: i32,
) -> i64 {
    stat_value(borrowed_fd, |stat| stat.st_blocks.saturating_mul(512))
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_org_rstorrent_storageprobe_NativeProbe_filesystemType(
    _environment: JNIEnv,
    _class: JClass,
    borrowed_fd: i32,
) -> i64 {
    statfs_value(borrowed_fd, |stat| stat.f_type as u64)
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_org_rstorrent_storageprobe_NativeProbe_filesystemBlockBytes(
    _environment: JNIEnv,
    _class: JClass,
    borrowed_fd: i32,
) -> i64 {
    statfs_value(borrowed_fd, |stat| stat.f_bsize as u64)
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_org_rstorrent_storageprobe_NativeProbe_startCancellable(
    _environment: JNIEnv,
    _class: JClass,
    borrowed_fd: i32,
    maximum_bytes: i64,
) -> i32 {
    if maximum_bytes <= 0 {
        return libc::EINVAL;
    }
    let fd = unsafe { libc::dup(borrowed_fd) };
    if fd < 0 {
        return last_errno();
    }
    let jobs = CANCEL_JOB.get_or_init(|| Mutex::new(None));
    let mut job = jobs.lock().expect("cancel job mutex poisoned");
    if job.is_some() {
        unsafe {
            libc::close(fd);
        }
        return libc::EBUSY;
    }
    let cancel = Arc::new(AtomicBool::new(false));
    let written = Arc::new(AtomicU64::new(0));
    let worker_cancel = Arc::clone(&cancel);
    let worker_written = Arc::clone(&written);
    let maximum_bytes = maximum_bytes as u64;
    let handle = thread::spawn(move || {
        let result = cancellable_write(fd, maximum_bytes, &worker_cancel, &worker_written);
        unsafe {
            libc::close(fd);
        }
        result
    });
    *job = Some(CancelJob {
        cancel,
        written,
        handle,
    });
    0
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_org_rstorrent_storageprobe_NativeProbe_cancellableProgress(
    _environment: JNIEnv,
    _class: JClass,
) -> i64 {
    let jobs = CANCEL_JOB.get_or_init(|| Mutex::new(None));
    let job = jobs.lock().expect("cancel job mutex poisoned");
    let Some(job) = job.as_ref() else {
        return -(libc::ENOENT as i64);
    };
    i64::try_from(job.written.load(Ordering::Acquire)).unwrap_or(i64::MAX)
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_org_rstorrent_storageprobe_NativeProbe_cancelAndJoin(
    _environment: JNIEnv,
    _class: JClass,
) -> i64 {
    let jobs = CANCEL_JOB.get_or_init(|| Mutex::new(None));
    let mut job = jobs.lock().expect("cancel job mutex poisoned");
    let Some(job) = job.take() else {
        return -(libc::ENOENT as i64);
    };
    job.cancel.store(true, Ordering::Release);
    match job.handle.join() {
        Ok(Ok(written)) => i64::try_from(written).unwrap_or(i64::MAX),
        Ok(Err(error)) => -(error.raw_os_error().unwrap_or(libc::EIO) as i64),
        Err(_) => -(libc::ECANCELED as i64),
    }
}

fn run_sparse(fd: RawFd, logical_length: i64) -> i64 {
    let head = pattern(HEAD_SEED);
    let tail = pattern(TAIL_SEED);
    let tail_offset = logical_length - BLOCK_LENGTH as i64;
    let mut result = 0;
    if unsafe { libc::ftruncate(fd, logical_length as libc::off_t) } != 0 {
        result |= ERROR_TRUNCATE;
        return result;
    }
    if pwrite_all(fd, &head, 0).is_err() {
        result |= ERROR_HEAD_WRITE;
    }
    if pwrite_all(fd, &tail, tail_offset).is_err() {
        result |= ERROR_TAIL_WRITE;
    }
    if unsafe { libc::fsync(fd) } != 0 {
        result |= ERROR_SYNC;
    }
    result | verify_sparse(fd, logical_length)
}

fn with_duplicate(borrowed_fd: RawFd, operation: impl FnOnce(RawFd) -> i64) -> i64 {
    let fd = unsafe { libc::dup(borrowed_fd) };
    if fd < 0 {
        return ERROR_DUP;
    }
    let result = operation(fd);
    unsafe {
        libc::close(fd);
    }
    result
}

fn verify_sparse(fd: RawFd, logical_length: i64) -> i64 {
    let mut stat = unsafe { std::mem::zeroed::<libc::stat>() };
    let mut result = 0;
    if unsafe { libc::fstat(fd, &mut stat) } != 0 {
        result |= ERROR_STAT;
    } else if stat.st_size != logical_length {
        result |= ERROR_LENGTH;
    }

    let mut head = [0_u8; BLOCK_LENGTH];
    if pread_exact(fd, &mut head, 0).is_err() || head != pattern(HEAD_SEED) {
        result |= ERROR_HEAD_READ;
    }
    let mut tail = [0_u8; BLOCK_LENGTH];
    let tail_offset = logical_length - BLOCK_LENGTH as i64;
    if pread_exact(fd, &mut tail, tail_offset).is_err() || tail != pattern(TAIL_SEED) {
        result |= ERROR_TAIL_READ;
    }
    result
}

fn cancellable_write(
    fd: RawFd,
    maximum_bytes: u64,
    cancel: &AtomicBool,
    progress: &AtomicU64,
) -> Result<u64, io::Error> {
    let buffer = pattern(73);
    let mut written = 0_u64;
    while written < maximum_bytes && !cancel.load(Ordering::Acquire) {
        let length = usize::try_from((maximum_bytes - written).min(BLOCK_LENGTH as u64))
            .map_err(|_| io::Error::from_raw_os_error(libc::EOVERFLOW))?;
        pwrite_all(fd, &buffer[..length], written as i64)?;
        written += length as u64;
        progress.store(written, Ordering::Release);
        thread::sleep(Duration::from_millis(1));
    }
    if unsafe { libc::fsync(fd) } != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(written)
}

fn pattern(seed: u8) -> [u8; BLOCK_LENGTH] {
    let mut buffer = [0_u8; BLOCK_LENGTH];
    for (index, byte) in buffer.iter_mut().enumerate() {
        *byte = (index as u8).wrapping_mul(37).wrapping_add(seed);
    }
    buffer
}

fn pwrite_all(fd: RawFd, mut bytes: &[u8], mut offset: i64) -> Result<(), io::Error> {
    while !bytes.is_empty() {
        let written = unsafe {
            libc::pwrite(
                fd,
                bytes.as_ptr().cast(),
                bytes.len(),
                offset as libc::off_t,
            )
        };
        if written < 0 {
            let error = io::Error::last_os_error();
            if error.kind() == io::ErrorKind::Interrupted {
                continue;
            }
            return Err(error);
        }
        if written == 0 {
            return Err(io::Error::from_raw_os_error(libc::EIO));
        }
        let written = written as usize;
        bytes = &bytes[written..];
        offset = offset
            .checked_add(written as i64)
            .ok_or_else(|| io::Error::from_raw_os_error(libc::EOVERFLOW))?;
    }
    Ok(())
}

fn pread_exact(fd: RawFd, mut bytes: &mut [u8], mut offset: i64) -> Result<(), io::Error> {
    while !bytes.is_empty() {
        let read = unsafe {
            libc::pread(
                fd,
                bytes.as_mut_ptr().cast(),
                bytes.len(),
                offset as libc::off_t,
            )
        };
        if read < 0 {
            let error = io::Error::last_os_error();
            if error.kind() == io::ErrorKind::Interrupted {
                continue;
            }
            return Err(error);
        }
        if read == 0 {
            return Err(io::Error::from_raw_os_error(libc::EIO));
        }
        let read = read as usize;
        bytes = &mut bytes[read..];
        offset = offset
            .checked_add(read as i64)
            .ok_or_else(|| io::Error::from_raw_os_error(libc::EOVERFLOW))?;
    }
    Ok(())
}

fn stat_value(fd: RawFd, select: impl FnOnce(&libc::stat) -> i64) -> i64 {
    let mut stat = unsafe { std::mem::zeroed::<libc::stat>() };
    if unsafe { libc::fstat(fd, &mut stat) } != 0 {
        -(last_errno() as i64)
    } else {
        select(&stat)
    }
}

fn statfs_value(fd: RawFd, select: impl FnOnce(&libc::statfs) -> u64) -> i64 {
    let mut stat = unsafe { std::mem::zeroed::<libc::statfs>() };
    if unsafe { libc::fstatfs(fd, &mut stat) } != 0 {
        -(last_errno() as i64)
    } else {
        i64::try_from(select(&stat)).unwrap_or(i64::MAX)
    }
}

fn last_errno() -> i32 {
    io::Error::last_os_error()
        .raw_os_error()
        .unwrap_or(libc::EIO)
}

#[cfg(test)]
mod tests {
    use std::fs::OpenOptions;
    use std::os::fd::AsRawFd;

    use super::{BLOCK_LENGTH, run_sparse, statfs_value, verify_sparse};

    #[test]
    fn sparse_round_trip_uses_fixed_blocks() {
        let path = std::env::temp_dir().join(format!(
            "rstorrent-android-native-probe-{}",
            std::process::id()
        ));
        let file = OpenOptions::new()
            .create(true)
            .truncate(true)
            .read(true)
            .write(true)
            .open(&path)
            .expect("create probe file");
        let logical_length = (4 * BLOCK_LENGTH) as i64;
        assert_eq!(run_sparse(file.as_raw_fd(), logical_length), 0);
        assert_eq!(verify_sparse(file.as_raw_fd(), logical_length), 0);
        assert!(statfs_value(file.as_raw_fd(), |stat| stat.f_type as u64) > 0);
        assert!(statfs_value(file.as_raw_fd(), |stat| stat.f_bsize as u64) > 0);
        drop(file);
        std::fs::remove_file(path).expect("remove probe file");
    }
}
