//! Byte observations emitted at the owners of network and storage I/O.

use std::fmt;
use std::sync::Arc;

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum ByteMetric {
    PayloadReceived,
    StagedWrite,
    PayloadVerified,
    PeerWireReceived,
    PeerWireSent,
    PeerProtocolReceived,
    PeerProtocolSent,
    MetadataPayloadReceived,
    MetadataPayloadSent,
    PayloadUploaded,
    PeerUnclassifiedReceived,
    PeerUnclassifiedSent,
    DhtReceived,
    DhtSent,
    TrackerReceived,
    TrackerSent,
    LogicalHashRead,
    PayloadRedundant,
    PayloadHashFailed,
}

pub trait ByteMetricSink: Send + Sync + fmt::Debug {
    fn record(&self, metric: ByteMetric, bytes: u64);
}

pub(crate) type SharedByteMetricSink = Arc<dyn ByteMetricSink>;
