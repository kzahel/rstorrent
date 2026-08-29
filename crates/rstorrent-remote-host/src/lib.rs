#![forbid(unsafe_code)]
//! Product-owned runtime composition for local Tactical 192 validation.
//!
//! Durable authority transitions stay in `rstorrent-remote-access`, endpoint
//! cryptography stays in `rstorrent-remote-crypto`, and the relay remains an
//! opaque router. This crate owns sockets, tasks, live circuits and the local
//! application adapter beneath desktop/headless lifecycles.

mod application;
mod error;
mod owner;
mod runtime;
mod wire;

pub use application::RemoteApplicationRuntime;
pub use error::RemoteHostError;
pub use owner::{
    DisableRemoteAccessOutcome, LiveCircuitView, RemoteAccessOwner, RemoteHostConfig,
    RemoteSecurityView,
};

pub use wire::{
    AUTHENTICATED_READY_MAGIC, AUTHENTICATION_SUCCEEDED_MAGIC, AUTHORIZATION_CHOICE_MAGIC,
    AuthenticationReady, AuthenticationSucceeded, AuthorizationChoice, AuthorizationSucceeded,
    HOST_GREETING_MAGIC, HostGreeting, LOGIN_FINALIZATION, LOGIN_REQUEST, LOGIN_RESPONSE,
    RESUME_FINALIZATION, RESUME_REQUEST, RESUME_RESPONSE, decode_json_record,
    decode_resume_request, encode_json_record, encode_resume_request, protocol_payload,
};
