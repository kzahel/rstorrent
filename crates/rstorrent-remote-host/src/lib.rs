#![forbid(unsafe_code)]
//! Product-owned runtime composition for local Tactical 192 validation.
//!
//! Durable authority transitions stay in `rstorrent-remote-access`, endpoint
//! cryptography stays in `rstorrent-remote-crypto`, and the relay remains an
//! opaque router. This crate owns sockets, tasks, live circuits and the local
//! application adapter beneath desktop/headless lifecycles.

mod application;
#[cfg(feature = "direct-file-webrtc")]
mod direct_file;
mod error;
mod owner;
mod runtime;
mod wire;

pub use application::RemoteApplicationRuntime;
pub use error::RemoteHostError;
pub use owner::{
    DirectFileSecurityView, DisableRemoteAccessOutcome, LiveCircuitView, RemoteAccessOwner,
    RemoteHostConfig, RemoteSecurityView,
};

pub use wire::{
    AUTHENTICATED_READY_MAGIC, AUTHENTICATION_SUCCEEDED_MAGIC, AUTHORIZATION_CHOICE_MAGIC,
    AuthenticationReady, AuthenticationSucceeded, AuthorizationChoice, AuthorizationSucceeded,
    HOST_GREETING_MAGIC, HostGreeting, LOGIN_FINALIZATION, LOGIN_REQUEST, LOGIN_RESPONSE,
    REMOTE_CONTROL_REQUEST_MAGIC, REMOTE_CONTROL_RESPONSE_MAGIC, RESUME_FINALIZATION,
    RESUME_REQUEST, RESUME_RESPONSE, RemoteControlOperation, RemoteControlOutcome,
    RemoteControlRequest, RemoteControlResponse, decode_control_request, decode_control_response,
    decode_json_record, decode_resume_request, encode_control_request, encode_control_response,
    encode_json_record, encode_resume_request, protocol_payload,
};

#[cfg(feature = "direct-file-webrtc")]
pub use wire::{
    DIRECT_FILE_REQUEST_MAGIC, DIRECT_FILE_RESPONSE_MAGIC, DirectCandidateClass,
    DirectFileCloseOutcome, DirectFileFailure, DirectFileRequest, DirectFileResponse,
    DirectFileStatus, DirectIceCandidate, DirectSdpType, DirectSessionDescription,
    MAX_DIRECT_FILE_SIGNALING_BYTES, decode_direct_file_request, decode_direct_file_response,
    encode_direct_file_request, encode_direct_file_response,
};
