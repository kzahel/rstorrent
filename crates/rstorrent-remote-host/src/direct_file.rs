use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use rstorrent_direct_file::{
    DirectFileEndpoint, DirectFileEndpointFactory, DirectFileEndpointSnapshot, OfferAnswer,
    RTCIceCandidateInit, RTCSessionDescription,
};
use rstorrent_session::ApplicationService;
use tokio::sync::Mutex;

use crate::wire::{DirectIceCandidate, DirectSessionDescription};

const MAX_DIRECT_PEERS: usize = 4;

pub(crate) struct DirectFileOpened {
    pub(crate) host_peer_generation: u64,
    pub(crate) file_length: u64,
    pub(crate) answer: DirectSessionDescription,
    pub(crate) candidates: Vec<DirectIceCandidate>,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct DirectFileSupervisorSnapshot {
    pub(crate) state: String,
    pub(crate) active_circuit: Option<[u8; 16]>,
    pub(crate) bytes_sent: u64,
    pub(crate) candidate_class: Option<String>,
    pub(crate) active_tasks: usize,
    pub(crate) open_sockets: usize,
    pub(crate) active_requests: usize,
    pub(crate) queued_bytes: usize,
}

struct ActivePeer {
    request_id: u32,
    circuit_generation: u64,
    browser_peer_generation: u64,
    host_peer_generation: u64,
    torrent_id: String,
    file_index: u32,
    file_length: u64,
    endpoint: DirectFileEndpoint,
}

struct SupervisorState {
    enabled: bool,
    peers: BTreeMap<[u8; 16], ActivePeer>,
}

pub(crate) struct ClosedDirectFile {
    pub(crate) circuit_id: [u8; 16],
    pub(crate) host_peer_generation: u64,
    pub(crate) torrent_id: String,
    pub(crate) file_index: u32,
    pub(crate) file_length: u64,
    pub(crate) snapshot: DirectFileEndpointSnapshot,
}

pub(crate) struct DirectFileSupervisor {
    application: Arc<Mutex<ApplicationService>>,
    factory: DirectFileEndpointFactory,
    state: Mutex<SupervisorState>,
    next_peer_generation: AtomicU64,
}

impl DirectFileSupervisor {
    pub(crate) fn new(application: Arc<Mutex<ApplicationService>>) -> Self {
        Self {
            factory: DirectFileEndpointFactory::new(application.clone()),
            application,
            state: Mutex::new(SupervisorState {
                enabled: true,
                peers: BTreeMap::new(),
            }),
            next_peer_generation: AtomicU64::new(1),
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) async fn open(
        &self,
        circuit_id: [u8; 16],
        request_id: u32,
        circuit_generation: u64,
        browser_peer_generation: u64,
        torrent_id: String,
        file_index: u32,
        offer: DirectSessionDescription,
    ) -> Result<DirectFileOpened, &'static str> {
        let mut state = self.state.lock().await;
        if !state.enabled {
            return Err("disabled");
        }
        if state.peers.contains_key(&circuit_id) || state.peers.len() >= MAX_DIRECT_PEERS {
            return Err("busy");
        }
        let capability = self
            .application
            .lock()
            .await
            .create_completed_media_capability(&torrent_id, file_index)
            .await
            .map_err(|_| "file_unavailable")?;
        let offer = RTCSessionDescription::offer(offer.sdp).map_err(|_| "invalid_request")?;
        let (answer, endpoint) = self
            .factory
            .answer_product_offer(capability.token, offer)
            .await
            .map_err(|_| "direct_unavailable")?;
        let host_peer_generation = self.next_peer_generation.fetch_add(1, Ordering::Relaxed);
        if host_peer_generation == 0 {
            let mut endpoint = endpoint;
            let _ = endpoint.shutdown().await;
            return Err("internal");
        }
        let opened = opened_answer(host_peer_generation, capability.length, &answer);
        state.peers.insert(
            circuit_id,
            ActivePeer {
                request_id,
                circuit_generation,
                browser_peer_generation,
                host_peer_generation,
                torrent_id,
                file_index,
                file_length: capability.length,
                endpoint,
            },
        );
        Ok(opened)
    }

    pub(crate) async fn add_candidate(
        &self,
        circuit_id: [u8; 16],
        circuit_generation: u64,
        browser_peer_generation: u64,
        request_id: u32,
        candidate: DirectIceCandidate,
    ) -> Result<u64, &'static str> {
        let state = self.state.lock().await;
        let peer = state.peers.get(&circuit_id).ok_or("invalid_request")?;
        if peer.circuit_generation != circuit_generation
            || peer.browser_peer_generation != browser_peer_generation
            || peer.request_id != request_id
        {
            return Err("invalid_request");
        }
        peer.endpoint
            .add_remote_candidate(RTCIceCandidateInit {
                candidate: candidate.candidate,
                sdp_mid: candidate.sdp_mid,
                sdp_mline_index: candidate.sdp_m_line_index,
                username_fragment: candidate.username_fragment,
                url: None,
            })
            .await
            .map_err(|_| "signaling_limit")?;
        Ok(peer.host_peer_generation)
    }

    pub(crate) async fn snapshot(&self) -> DirectFileSupervisorSnapshot {
        let state = self.state.lock().await;
        let Some((circuit_id, peer)) = state.peers.iter().next() else {
            return DirectFileSupervisorSnapshot {
                state: "idle".to_owned(),
                ..DirectFileSupervisorSnapshot::default()
            };
        };
        snapshot_view(*circuit_id, peer.endpoint.snapshot())
    }

    pub(crate) async fn validate_peer(
        &self,
        circuit_id: [u8; 16],
        circuit_generation: u64,
        browser_peer_generation: u64,
        request_id: u32,
    ) -> Result<u64, &'static str> {
        let state = self.state.lock().await;
        let peer = state.peers.get(&circuit_id).ok_or("invalid_request")?;
        if peer.circuit_generation != circuit_generation
            || peer.browser_peer_generation != browser_peer_generation
            || peer.request_id != request_id
        {
            return Err("invalid_request");
        }
        Ok(peer.host_peer_generation)
    }

    pub(crate) async fn close_matching(
        &self,
        circuit_id: [u8; 16],
        circuit_generation: u64,
        browser_peer_generation: u64,
        request_id: u32,
    ) -> Result<ClosedDirectFile, &'static str> {
        self.validate_peer(
            circuit_id,
            circuit_generation,
            browser_peer_generation,
            request_id,
        )
        .await?;
        self.close(circuit_id).await.ok_or("invalid_request")
    }

    pub(crate) async fn close(&self, circuit_id: [u8; 16]) -> Option<ClosedDirectFile> {
        let mut state = self.state.lock().await;
        let mut peer = state.peers.remove(&circuit_id)?;
        let _ = peer.endpoint.shutdown().await;
        Some(closed_peer(circuit_id, peer))
    }

    pub(crate) async fn close_all(&self) -> Vec<ClosedDirectFile> {
        let mut state = self.state.lock().await;
        let peers = std::mem::take(&mut state.peers);
        let mut closed = Vec::with_capacity(peers.len());
        for (circuit_id, mut peer) in peers {
            let _ = peer.endpoint.shutdown().await;
            closed.push(closed_peer(circuit_id, peer));
        }
        closed
    }

    pub(crate) async fn set_enabled(&self, enabled: bool) -> Vec<ClosedDirectFile> {
        let mut state = self.state.lock().await;
        state.enabled = enabled;
        if enabled {
            return Vec::new();
        }
        let peers = std::mem::take(&mut state.peers);
        let mut closed = Vec::with_capacity(peers.len());
        for (circuit_id, mut peer) in peers {
            let _ = peer.endpoint.shutdown().await;
            closed.push(closed_peer(circuit_id, peer));
        }
        closed
    }
}

fn opened_answer(
    host_peer_generation: u64,
    file_length: u64,
    answer: &OfferAnswer,
) -> DirectFileOpened {
    DirectFileOpened {
        host_peer_generation,
        file_length,
        answer: DirectSessionDescription {
            kind: crate::wire::DirectSdpType::Answer,
            sdp: answer.answer.sdp.clone(),
        },
        candidates: answer.local_candidates.iter().map(wire_candidate).collect(),
    }
}

fn wire_candidate(candidate: &RTCIceCandidateInit) -> DirectIceCandidate {
    DirectIceCandidate {
        candidate: candidate.candidate.clone(),
        sdp_mid: candidate.sdp_mid.clone(),
        sdp_m_line_index: candidate.sdp_mline_index,
        username_fragment: candidate.username_fragment.clone(),
    }
}

fn closed_peer(circuit_id: [u8; 16], peer: ActivePeer) -> ClosedDirectFile {
    ClosedDirectFile {
        circuit_id,
        host_peer_generation: peer.host_peer_generation,
        torrent_id: peer.torrent_id,
        file_index: peer.file_index,
        file_length: peer.file_length,
        snapshot: peer.endpoint.snapshot(),
    }
}

fn snapshot_view(
    circuit_id: [u8; 16],
    endpoint: DirectFileEndpointSnapshot,
) -> DirectFileSupervisorSnapshot {
    DirectFileSupervisorSnapshot {
        state: endpoint.state,
        active_circuit: Some(circuit_id),
        bytes_sent: endpoint.bytes_sent,
        candidate_class: endpoint.selected_candidate_class,
        active_tasks: endpoint.active_tasks,
        open_sockets: endpoint.open_sockets,
        active_requests: endpoint.active_requests,
        queued_bytes: endpoint.queued_bytes + endpoint.transport_buffered_bytes,
    }
}
