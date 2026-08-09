//! Projection and lifecycle for the session-scoped DHT observation view.

use std::fmt::Write as _;

use rstorrent_engine::dht::{DhtFamilyObservation, DhtLifecycle, DhtObservation};
use rstorrent_engine::{AddressFamily, NetworkPolicy};
use rstorrent_protocol::dht::NodeId;
use tokio::sync::watch;
use tokio::task::{JoinError, JoinHandle};
use tokio_util::sync::CancellationToken;

use crate::views::{
    DhtAddressFamilyView, DhtBucketView, DhtFamilyInspectionView, DhtInspectionView,
    DhtLifecycleView, DhtLookupView, DhtNetworkPolicyView, SubscriptionError, ViewHub,
};

pub(crate) fn inspection_view(observation: &DhtObservation) -> DhtInspectionView {
    DhtInspectionView {
        lifecycle: lifecycle_view(observation.lifecycle),
        network_policy: match observation.network_policy {
            NetworkPolicy::Offline => DhtNetworkPolicyView::Offline,
            NetworkPolicy::LoopbackOnly => DhtNetworkPolicyView::LoopbackOnly,
            NetworkPolicy::Online => DhtNetworkPolicyView::Online,
        },
        captured_millis: observation.captured_millis.to_string(),
        active_transactions: observation.stats.active_transactions,
        active_lookups: observation.stats.active_lookups,
        queries_sent: observation.stats.queries_sent.to_string(),
        responses_received: observation.stats.responses_received.to_string(),
        queries_received: observation.stats.queries_received.to_string(),
        malformed_received: observation.stats.malformed_received.to_string(),
        family_mismatched: observation.stats.family_mismatched.to_string(),
        rate_limited: observation.stats.rate_limited.to_string(),
        discovered_peers: observation.stats.discovered_peers.to_string(),
        bootstrap_attempts: observation.stats.bootstrap_attempts.to_string(),
        routing_refreshes: observation.stats.routing_refreshes.to_string(),
        datagram_bytes_sent: observation.stats.datagram_bytes_sent.to_string(),
        datagram_bytes_received: observation.stats.datagram_bytes_received.to_string(),
        announces_sent: observation.stats.announces_sent.to_string(),
        announces_succeeded: observation.stats.announces_succeeded.to_string(),
        announces_failed: observation.stats.announces_failed.to_string(),
        families: observation
            .families
            .iter()
            .map(family_inspection_view)
            .collect(),
        lookups: observation
            .families
            .iter()
            .flat_map(|family| family.lookups.iter())
            .map(|lookup| DhtLookupView {
                family: address_family_view(lookup.family),
                lookup_id: lookup.lookup_id.to_string(),
                target_id: node_id_hex(lookup.target),
                age_millis: lookup.age_millis.to_string(),
                deadline_in_millis: lookup.deadline_in_millis.to_string(),
                unqueried_candidates: lookup.unqueried_candidates,
                in_flight_candidates: lookup.in_flight_candidates,
                responded_candidates: lookup.responded_candidates,
                failed_candidates: lookup.failed_candidates,
                discovered_peers: lookup.discovered_peers,
                closest_responded_prefix_bits: lookup.closest_responded_prefix_bits,
                last_convergence_improvement_age_millis: lookup
                    .last_convergence_improvement_age_millis
                    .map(|millis| millis.to_string()),
            })
            .collect(),
    }
}

fn family_inspection_view(family: &DhtFamilyObservation) -> DhtFamilyInspectionView {
    DhtFamilyInspectionView {
        family: address_family_view(family.family),
        lifecycle: lifecycle_view(family.lifecycle),
        local_node_id: node_id_hex(family.local_node_id),
        local_address: family.local_address.to_string(),
        observed_external_address: family
            .observed_external_address
            .map(|address| address.to_string()),
        routing_nodes: family.routing_nodes,
        occupied_buckets: family.occupied_buckets,
        deepest_shared_prefix_bits: family.deepest_shared_prefix_bits,
        active_transactions: family.stats.active_transactions,
        active_lookups: family.stats.active_lookups,
        queries_sent: family.stats.queries_sent.to_string(),
        responses_received: family.stats.responses_received.to_string(),
        queries_received: family.stats.queries_received.to_string(),
        malformed_received: family.stats.malformed_received.to_string(),
        family_mismatched: family.stats.family_mismatched.to_string(),
        rate_limited: family.stats.rate_limited.to_string(),
        discovered_peers: family.stats.discovered_peers.to_string(),
        bootstrap_attempts: family.stats.bootstrap_attempts.to_string(),
        routing_refreshes: family.stats.routing_refreshes.to_string(),
        datagram_bytes_sent: family.stats.datagram_bytes_sent.to_string(),
        datagram_bytes_received: family.stats.datagram_bytes_received.to_string(),
        announces_sent: family.stats.announces_sent.to_string(),
        announces_succeeded: family.stats.announces_succeeded.to_string(),
        announces_failed: family.stats.announces_failed.to_string(),
        buckets: family
            .buckets
            .iter()
            .map(|bucket| DhtBucketView {
                bucket_index: bucket.bucket_index,
                good_nodes: bucket.good_nodes,
                questionable_nodes: bucket.questionable_nodes,
                replacement_candidates: bucket.replacement_candidates,
                oldest_live_response_age_millis: bucket
                    .oldest_live_response_age_seconds
                    .map(|seconds| seconds.saturating_mul(1_000).to_string()),
            })
            .collect(),
    }
}

const fn lifecycle_view(lifecycle: DhtLifecycle) -> DhtLifecycleView {
    match lifecycle {
        DhtLifecycle::Offline => DhtLifecycleView::Offline,
        DhtLifecycle::BootstrapEmpty => DhtLifecycleView::BootstrapEmpty,
        DhtLifecycle::Participating => DhtLifecycleView::Participating,
        DhtLifecycle::Inactive => DhtLifecycleView::Inactive,
    }
}

const fn address_family_view(family: AddressFamily) -> DhtAddressFamilyView {
    match family {
        AddressFamily::Ipv4 => DhtAddressFamilyView::Ipv4,
        AddressFamily::Ipv6 => DhtAddressFamilyView::Ipv6,
    }
}

fn node_id_hex(node_id: NodeId) -> String {
    let mut output = String::with_capacity(40);
    for byte in node_id.0 {
        write!(&mut output, "{byte:02x}").expect("writing to a String cannot fail");
    }
    output
}

#[derive(Debug)]
pub(crate) struct DhtObservationRuntime {
    cancellation: CancellationToken,
    task: Option<JoinHandle<Result<(), SubscriptionError>>>,
}

impl DhtObservationRuntime {
    pub(crate) fn start(mut observations: watch::Receiver<DhtObservation>, views: ViewHub) -> Self {
        let cancellation = CancellationToken::new();
        let task_cancellation = cancellation.clone();
        let task = tokio::spawn(async move {
            loop {
                tokio::select! {
                    biased;
                    _ = task_cancellation.cancelled() => return Ok(()),
                    changed = observations.changed() => {
                        if changed.is_err() {
                            return Ok(());
                        }
                        let inspection = inspection_view(&observations.borrow_and_update());
                        views.publish_dht(inspection)?;
                    }
                }
            }
        });
        Self {
            cancellation,
            task: Some(task),
        }
    }

    pub(crate) async fn join(mut self) -> Result<Result<(), SubscriptionError>, JoinError> {
        let Some(task) = self.task.take() else {
            return Ok(Ok(()));
        };
        task.await
    }
}

impl Drop for DhtObservationRuntime {
    fn drop(&mut self) {
        self.cancellation.cancel();
        if let Some(task) = self.task.take() {
            task.abort();
        }
    }
}

#[cfg(test)]
mod tests {
    use rstorrent_engine::dht::{DhtFamilyObservation, DhtLookupObservation, DhtStats};
    use rstorrent_protocol::dht::RoutingBucketInspection;

    use super::*;

    #[test]
    fn observation_projection_preserves_bounded_exact_state() {
        let mut buckets = (0..160)
            .map(|bucket_index| RoutingBucketInspection {
                bucket_index,
                good_nodes: 0,
                questionable_nodes: 0,
                replacement_candidates: 0,
                oldest_live_response_age_seconds: None,
            })
            .collect::<Vec<_>>();
        buckets[142] = RoutingBucketInspection {
            bucket_index: 142,
            good_nodes: 6,
            questionable_nodes: 2,
            replacement_candidates: 3,
            oldest_live_response_age_seconds: Some(901),
        };
        let observation = DhtObservation {
            lifecycle: DhtLifecycle::Participating,
            network_policy: NetworkPolicy::LoopbackOnly,
            captured_millis: u64::MAX,
            stats: DhtStats {
                active_transactions: 2,
                active_lookups: 1,
                queries_sent: u64::MAX,
                datagram_bytes_received: 1_024,
                ..DhtStats::default()
            },
            families: vec![DhtFamilyObservation {
                family: AddressFamily::Ipv6,
                lifecycle: DhtLifecycle::Participating,
                local_node_id: NodeId([0xab; 20]),
                local_address: "[::1]:6881".parse().expect("local address"),
                observed_external_address: Some("2001:4860::1".parse().expect("external address")),
                routing_nodes: 8,
                occupied_buckets: 1,
                deepest_shared_prefix_bits: Some(17),
                stats: DhtStats {
                    active_transactions: 2,
                    active_lookups: 1,
                    queries_sent: u64::MAX,
                    datagram_bytes_received: 1_024,
                    ..DhtStats::default()
                },
                buckets,
                lookups: vec![DhtLookupObservation {
                    family: AddressFamily::Ipv6,
                    lookup_id: u64::MAX,
                    target: NodeId([0xcd; 20]),
                    age_millis: 1_500,
                    deadline_in_millis: 28_500,
                    unqueried_candidates: 5,
                    in_flight_candidates: 3,
                    responded_candidates: 11,
                    failed_candidates: 2,
                    discovered_peers: 7,
                    closest_responded_prefix_bits: Some(17),
                    last_convergence_improvement_age_millis: Some(250),
                }],
            }],
        };

        let view = inspection_view(&observation);

        assert_eq!(view.lifecycle, DhtLifecycleView::Participating);
        assert_eq!(view.network_policy, DhtNetworkPolicyView::LoopbackOnly);
        assert_eq!(view.captured_millis, u64::MAX.to_string());
        assert_eq!(view.queries_sent, u64::MAX.to_string());
        assert_eq!(view.families[0].family, DhtAddressFamilyView::Ipv6);
        assert_eq!(view.families[0].local_node_id, "ab".repeat(20));
        assert_eq!(view.families[0].buckets.len(), 160);
        assert_eq!(
            view.families[0].buckets[142]
                .oldest_live_response_age_millis
                .as_deref(),
            Some("901000")
        );
        assert_eq!(view.lookups[0].lookup_id, u64::MAX.to_string());
        assert_eq!(view.lookups[0].family, DhtAddressFamilyView::Ipv6);
        assert_eq!(view.lookups[0].target_id, "cd".repeat(20));
        assert_eq!(view.lookups[0].closest_responded_prefix_bits, Some(17));
    }
}
