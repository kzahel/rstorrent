//! Runtime ownership for bounded MSE Diffie-Hellman work.

use std::error::Error;
use std::fmt;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use rstorrent_protocol::mse::{
    DhError, DhPrivateExponent, DhPublicKey, DhSharedSecret, compute_public_key,
    compute_shared_secret,
};
use tokio::sync::{Notify, Semaphore};
use tokio::task::JoinError;
use tokio_util::task::TaskTracker;

pub const MAX_MSE_DH_JOBS: usize = 4;

#[derive(Clone)]
pub struct MseDhWorkOwner {
    inner: Arc<Inner>,
}

struct Inner {
    permits: Arc<Semaphore>,
    tracker: TaskTracker,
    admission: Mutex<Admission>,
    waiting: AtomicUsize,
    active: AtomicUsize,
    high_water: AtomicUsize,
    changed: Notify,
}

#[derive(Default)]
struct Admission {
    closed: bool,
}

impl MseDhWorkOwner {
    #[must_use]
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Inner {
                permits: Arc::new(Semaphore::new(MAX_MSE_DH_JOBS)),
                tracker: TaskTracker::new(),
                admission: Mutex::new(Admission::default()),
                waiting: AtomicUsize::new(0),
                active: AtomicUsize::new(0),
                high_water: AtomicUsize::new(0),
                changed: Notify::new(),
            }),
        }
    }

    pub async fn compute_public_key(
        &self,
        private: DhPrivateExponent,
    ) -> Result<(DhPrivateExponent, DhPublicKey), MseDhWorkError> {
        self.run(move || {
            let public = compute_public_key(&private);
            (private, public)
        })
        .await
    }

    pub async fn compute_shared_secret(
        &self,
        private: DhPrivateExponent,
        remote_public: [u8; 96],
    ) -> Result<DhSharedSecret, MseDhWorkError> {
        self.run(move || compute_shared_secret(&private, &remote_public))
            .await?
            .map_err(MseDhWorkError::Dh)
    }

    #[must_use]
    pub fn snapshot(&self) -> MseDhWorkSnapshot {
        MseDhWorkSnapshot {
            waiting: self.inner.waiting.load(Ordering::Acquire),
            active: self.inner.active.load(Ordering::Acquire),
            high_water: self.inner.high_water.load(Ordering::Acquire),
            tracked: self.inner.tracker.len(),
            closed: self
                .inner
                .admission
                .lock()
                .expect("MSE DH admission lock poisoned")
                .closed,
        }
    }

    pub fn close(&self) {
        let mut admission = self
            .inner
            .admission
            .lock()
            .expect("MSE DH admission lock poisoned");
        if admission.closed {
            return;
        }
        admission.closed = true;
        self.inner.permits.close();
        self.inner.tracker.close();
        self.inner.changed.notify_waiters();
    }

    pub async fn wait(&self) {
        self.inner.tracker.wait().await;
    }

    pub async fn shutdown(&self) {
        self.close();
        self.wait().await;
    }

    async fn run<T, F>(&self, operation: F) -> Result<T, MseDhWorkError>
    where
        T: Send + 'static,
        F: FnOnce() -> T + Send + 'static,
    {
        {
            let admission = self
                .inner
                .admission
                .lock()
                .expect("MSE DH admission lock poisoned");
            if admission.closed {
                return Err(MseDhWorkError::Closed);
            }
        }

        let waiting = CounterGuard::increment(&self.inner, Counter::Waiting);
        let permit = self
            .inner
            .permits
            .clone()
            .acquire_owned()
            .await
            .map_err(|_| MseDhWorkError::Closed)?;
        drop(waiting);

        let join = {
            let admission = self
                .inner
                .admission
                .lock()
                .expect("MSE DH admission lock poisoned");
            if admission.closed {
                return Err(MseDhWorkError::Closed);
            }
            let inner = self.inner.clone();
            self.inner.tracker.spawn_blocking(move || {
                let _permit = permit;
                let _active = CounterGuard::increment(&inner, Counter::Active);
                operation()
            })
        };
        join.await.map_err(MseDhWorkError::TaskJoin)
    }

    #[cfg(test)]
    async fn wait_for_snapshot(
        &self,
        predicate: impl Fn(MseDhWorkSnapshot) -> bool,
    ) -> MseDhWorkSnapshot {
        loop {
            let notified = self.inner.changed.notified();
            let snapshot = self.snapshot();
            if predicate(snapshot) {
                return snapshot;
            }
            notified.await;
        }
    }
}

impl Default for MseDhWorkOwner {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Debug for MseDhWorkOwner {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("MseDhWorkOwner")
            .field("snapshot", &self.snapshot())
            .finish()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MseDhWorkSnapshot {
    pub waiting: usize,
    pub active: usize,
    pub high_water: usize,
    pub tracked: usize,
    pub closed: bool,
}

#[derive(Debug)]
pub enum MseDhWorkError {
    Closed,
    Dh(DhError),
    TaskJoin(JoinError),
}

impl fmt::Display for MseDhWorkError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Closed => formatter.write_str("MSE DH work owner is closed"),
            Self::Dh(error) => write!(formatter, "MSE DH computation: {error}"),
            Self::TaskJoin(error) => write!(formatter, "MSE DH task join: {error}"),
        }
    }
}

impl Error for MseDhWorkError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::TaskJoin(error) => Some(error),
            Self::Closed | Self::Dh(_) => None,
        }
    }
}

#[derive(Clone, Copy)]
enum Counter {
    Waiting,
    Active,
}

struct CounterGuard {
    inner: Arc<Inner>,
    counter: Counter,
}

impl CounterGuard {
    fn increment(inner: &Arc<Inner>, counter: Counter) -> Self {
        let value = match counter {
            Counter::Waiting => inner.waiting.fetch_add(1, Ordering::AcqRel) + 1,
            Counter::Active => inner.active.fetch_add(1, Ordering::AcqRel) + 1,
        };
        if matches!(counter, Counter::Active) {
            inner.high_water.fetch_max(value, Ordering::AcqRel);
        }
        inner.changed.notify_waiters();
        Self {
            inner: inner.clone(),
            counter,
        }
    }
}

impl Drop for CounterGuard {
    fn drop(&mut self) {
        match self.counter {
            Counter::Waiting => self.inner.waiting.fetch_sub(1, Ordering::AcqRel),
            Counter::Active => self.inner.active.fetch_sub(1, Ordering::AcqRel),
        };
        self.inner.changed.notify_waiters();
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Condvar, Mutex};
    use std::time::Duration;

    use tokio::sync::mpsc;
    use tokio::time::timeout;

    use super::{MAX_MSE_DH_JOBS, MseDhWorkError, MseDhWorkOwner};

    #[tokio::test]
    async fn five_jobs_observe_four_active_and_one_waiting_then_drain() {
        let owner = MseDhWorkOwner::new();
        let gate = Arc::new((Mutex::new(false), Condvar::new()));
        let (started_tx, mut started_rx) = mpsc::unbounded_channel();
        let mut callers = Vec::new();

        for job in 0..=MAX_MSE_DH_JOBS {
            let owner = owner.clone();
            let gate = gate.clone();
            let started_tx = started_tx.clone();
            callers.push(tokio::spawn(async move {
                owner
                    .run(move || {
                        started_tx.send(job).expect("observe started job");
                        let (lock, ready) = &*gate;
                        let released = lock.lock().expect("test gate");
                        drop(
                            ready
                                .wait_while(released, |released| !*released)
                                .expect("test gate wait"),
                        );
                        job
                    })
                    .await
            }));
        }
        drop(started_tx);

        for _ in 0..MAX_MSE_DH_JOBS {
            timeout(Duration::from_secs(1), started_rx.recv())
                .await
                .expect("blocking job start deadline")
                .expect("blocking job start");
        }
        let saturated = timeout(
            Duration::from_secs(1),
            owner.wait_for_snapshot(|snapshot| {
                snapshot.active == MAX_MSE_DH_JOBS && snapshot.waiting == 1
            }),
        )
        .await
        .expect("owner saturation deadline");
        assert_eq!(saturated.tracked, MAX_MSE_DH_JOBS);
        assert_eq!(saturated.high_water, MAX_MSE_DH_JOBS);

        let (lock, ready) = &*gate;
        *lock.lock().expect("test gate") = true;
        ready.notify_all();

        for caller in callers {
            caller
                .await
                .expect("caller join")
                .expect("blocking work result");
        }
        owner.shutdown().await;
        assert_eq!(owner.snapshot().active, 0);
        assert_eq!(owner.snapshot().waiting, 0);
        assert_eq!(owner.snapshot().tracked, 0);
    }

    #[tokio::test]
    async fn cancelled_caller_does_not_release_permit_or_orphan_work() {
        let owner = MseDhWorkOwner::new();
        let gate = Arc::new((Mutex::new(false), Condvar::new()));
        let caller_owner = owner.clone();
        let caller_gate = gate.clone();
        let caller = tokio::spawn(async move {
            caller_owner
                .run(move || {
                    let (lock, ready) = &*caller_gate;
                    let released = lock.lock().expect("test gate");
                    drop(
                        ready
                            .wait_while(released, |released| !*released)
                            .expect("test gate wait"),
                    );
                })
                .await
        });

        timeout(
            Duration::from_secs(1),
            owner.wait_for_snapshot(|snapshot| snapshot.active == 1),
        )
        .await
        .expect("blocking job start deadline");
        caller.abort();
        assert!(caller.await.expect_err("caller aborted").is_cancelled());
        assert_eq!(owner.snapshot().active, 1);
        assert_eq!(owner.snapshot().tracked, 1);

        owner.close();
        assert!(matches!(
            owner.run(|| ()).await,
            Err(MseDhWorkError::Closed)
        ));
        let wait = tokio::spawn({
            let owner = owner.clone();
            async move { owner.wait().await }
        });
        assert!(!wait.is_finished());

        let (lock, ready) = &*gate;
        *lock.lock().expect("test gate") = true;
        ready.notify_all();
        timeout(Duration::from_secs(1), wait)
            .await
            .expect("tracked work drain deadline")
            .expect("wait join");
        assert_eq!(owner.snapshot().active, 0);
        assert_eq!(owner.snapshot().tracked, 0);
    }
}
