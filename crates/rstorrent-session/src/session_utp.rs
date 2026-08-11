//! Joined application ownership for the fixed uTP service and incoming peers.

use std::time::Duration;

use rstorrent_engine::{
    IncomingPeerError, IncomingPeerHandle, UtpHandle, UtpService, UtpServiceSnapshot,
};
use tokio::task::{JoinHandle, JoinSet};
use tokio_util::sync::CancellationToken;

#[derive(Debug)]
pub(crate) struct SessionUtpPeerService {
    handle: UtpHandle,
    cancellation: CancellationToken,
    task: Option<JoinHandle<Result<UtpServiceSnapshot, String>>>,
}

impl SessionUtpPeerService {
    pub(crate) fn start(
        service: UtpService,
        incoming: IncomingPeerHandle,
        handshake_timeout: Duration,
    ) -> Self {
        let handle = service.handle();
        let cancellation = CancellationToken::new();
        let task = tokio::spawn(run_session_utp(
            service,
            incoming,
            handshake_timeout,
            cancellation.clone(),
        ));
        Self {
            handle,
            cancellation,
            task: Some(task),
        }
    }

    pub(crate) fn handle(&self) -> UtpHandle {
        self.handle.clone()
    }

    pub(crate) async fn shutdown(mut self) -> Result<UtpServiceSnapshot, String> {
        self.cancellation.cancel();
        self.task
            .take()
            .expect("session uTP task exists before shutdown")
            .await
            .map_err(|error| format!("session uTP supervisor: {error}"))?
    }
}

impl Drop for SessionUtpPeerService {
    fn drop(&mut self) {
        self.cancellation.cancel();
        if let Some(task) = self.task.take() {
            task.abort();
        }
    }
}

async fn run_session_utp(
    mut service: UtpService,
    incoming: IncomingPeerHandle,
    handshake_timeout: Duration,
    cancellation: CancellationToken,
) -> Result<UtpServiceSnapshot, String> {
    let mut admissions = JoinSet::new();
    let mut admission_error = None;
    loop {
        tokio::select! {
            biased;
            _ = cancellation.cancelled() => break,
            joined = admissions.join_next(), if !admissions.is_empty() => {
                remember_admission_error(&mut admission_error, joined);
            }
            stream = service.accept() => match stream {
                Some(stream) => {
                    let incoming = incoming.clone();
                    admissions.spawn(async move {
                        incoming.admit_utp(stream, handshake_timeout).await
                    });
                }
                None => break,
            }
        }
    }

    let snapshot = service
        .shutdown()
        .await
        .map_err(|error| format!("uTP service: {error}"))?;
    while let Some(joined) = admissions.join_next().await {
        remember_admission_error(&mut admission_error, Some(joined));
    }
    if let Some(error) = admission_error {
        return Err(error);
    }
    Ok(snapshot)
}

fn remember_admission_error(
    retained: &mut Option<String>,
    joined: Option<Result<Result<(), IncomingPeerError>, tokio::task::JoinError>>,
) {
    let error = match joined {
        Some(Ok(Ok(()))) | None => return,
        Some(Ok(Err(error))) => format!("incoming uTP admission: {error}"),
        Some(Err(error)) => format!("incoming uTP admission task: {error}"),
    };
    if retained.is_none() {
        *retained = Some(error);
    }
}
