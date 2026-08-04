//! Deterministic snapshot-diff and patch-coalescing operations.
//!
//! The hub and delivery accumulators supply immutable values to this module.
//! It owns no shared state, lock, channel, or task.

use std::collections::{BTreeMap, BTreeSet};

use crate::control::StorageSettingsSnapshot;
use crate::diagnostics::{
    MAX_DIAGNOSTIC_PATCH_BYTES, MAX_DIAGNOSTIC_PATCH_EVENTS, patch_encoded_len,
};
use crate::file_views::FileView;
use crate::tracker_views::TrackerView;

use super::ranges::{difference, insert_interval, remove_interval};
use super::{
    ActivePiece, DiskSessionView, IndexRange, PeerView, SubscriptionSpec, SwarmModel, TorrentModel,
    TorrentView, ViewPatch, ViewProjection, ViewSelector, ViewUpdate, ViewUpdatePayload,
};

pub(super) fn patch_for(
    spec: &SubscriptionSpec,
    previous: &BTreeMap<String, TorrentModel>,
    current: &BTreeMap<String, TorrentModel>,
    previous_storage: Option<&StorageSettingsSnapshot>,
    current_storage: &StorageSettingsSnapshot,
) -> Option<ViewPatch> {
    match (&spec.selector, spec.projection) {
        (ViewSelector::TorrentList, ViewProjection::Summary) => {
            let upsert = current
                .iter()
                .filter(|(id, model)| previous.get(*id).map(|old| &old.view) != Some(&model.view))
                .map(|(_, model)| model.view.clone())
                .collect::<Vec<_>>();
            let removed = previous
                .keys()
                .filter(|id| !current.contains_key(*id))
                .cloned()
                .collect::<Vec<_>>();
            let storage = previous_storage.map(|_| current_storage.clone());
            (!upsert.is_empty() || !removed.is_empty() || storage.is_some()).then_some(
                ViewPatch::TorrentList {
                    upsert,
                    removed,
                    storage,
                },
            )
        }
        (ViewSelector::Torrent { torrent_id }, ViewProjection::Summary) => {
            let old = previous.get(torrent_id).map(|model| &model.view);
            let next = current.get(torrent_id).map(|model| &model.view);
            (old != next).then(|| ViewPatch::Torrent {
                torrent: next.cloned(),
            })
        }
        (ViewSelector::Torrent { torrent_id }, ViewProjection::PieceActivity) => {
            let old = previous.get(torrent_id);
            let next = current.get(torrent_id);
            if old == next {
                return None;
            }
            let old_verified = old.map_or(&[][..], |model| model.verified.as_slice());
            let next_verified = next.map_or(&[][..], |model| model.verified.as_slice());
            let verified = difference(next_verified, old_verified);
            let cleared = difference(old_verified, next_verified);
            let empty = BTreeMap::new();
            let old_active = old.map_or(&empty, |model| &model.active);
            let next_active = next.map_or(&empty, |model| &model.active);
            let (active_upsert, active_removed) = active_piece_patch(old_active, next_active);
            Some(ViewPatch::PieceActivity {
                torrent_id: torrent_id.clone(),
                piece_count: next.map_or(0, |model| model.view.piece_count),
                verified,
                cleared,
                active_upsert,
                active_removed,
            })
        }
        (ViewSelector::Torrent { torrent_id }, ViewProjection::Peers) => {
            let empty = BTreeMap::new();
            let old = previous
                .get(torrent_id)
                .map_or(&empty, |model| &model.peers);
            let next = current.get(torrent_id).map_or(&empty, |model| &model.peers);
            peer_collection_patch(torrent_id, old, next)
        }
        (ViewSelector::Torrent { torrent_id }, ViewProjection::Swarm) => {
            match (previous.get(torrent_id), current.get(torrent_id)) {
                (Some(old), Some(next)) => {
                    swarm_collection_patch(torrent_id, &old.swarm, &next.swarm)
                }
                _ => None,
            }
        }
        (ViewSelector::Torrent { torrent_id }, ViewProjection::Files) => {
            let old = previous
                .get(torrent_id)
                .and_then(|model| model.files.as_ref());
            let next = current
                .get(torrent_id)
                .and_then(|model| model.files.as_ref());
            match (old, next) {
                (Some(old), Some(next)) if old.catalog_matches(next) => {
                    let upsert = next.rows_changed_since(old);
                    (!upsert.is_empty()).then(|| ViewPatch::Files {
                        torrent_id: torrent_id.clone(),
                        upsert,
                        removed: Vec::new(),
                    })
                }
                _ => None,
            }
        }
        (ViewSelector::Torrent { torrent_id }, ViewProjection::Trackers) => {
            let empty = BTreeMap::new();
            let old = previous
                .get(torrent_id)
                .map_or(&empty, |model| model.trackers.row_map());
            let next = current
                .get(torrent_id)
                .map_or(&empty, |model| model.trackers.row_map());
            tracker_collection_patch(torrent_id, old, next)
        }
        (
            ViewSelector::TorrentList,
            ViewProjection::PieceActivity
            | ViewProjection::Peers
            | ViewProjection::Swarm
            | ViewProjection::Files
            | ViewProjection::Trackers,
        ) => None,
        (
            _,
            ViewProjection::Disk
            | ViewProjection::Dht
            | ViewProjection::Speed
            | ViewProjection::Diagnostics,
        ) => None,
        (ViewSelector::SessionDht, _) => None,
        (ViewSelector::SessionSpeed { .. }, _) => None,
    }
}

pub(super) fn projection_requires_snapshot(
    spec: &SubscriptionSpec,
    previous: &BTreeMap<String, TorrentModel>,
    current: &BTreeMap<String, TorrentModel>,
) -> bool {
    if let (ViewSelector::Torrent { torrent_id }, ViewProjection::Swarm) =
        (&spec.selector, spec.projection)
    {
        return previous.contains_key(torrent_id) != current.contains_key(torrent_id);
    }
    if let (ViewSelector::Torrent { torrent_id }, ViewProjection::Trackers) =
        (&spec.selector, spec.projection)
    {
        return previous.contains_key(torrent_id) != current.contains_key(torrent_id);
    }
    let (ViewSelector::Torrent { torrent_id }, ViewProjection::Files) =
        (&spec.selector, spec.projection)
    else {
        return false;
    };
    match (previous.get(torrent_id), current.get(torrent_id)) {
        (None, None) => false,
        (Some(old), Some(next)) => match (&old.files, &next.files) {
            (None, None) => false,
            (Some(old), Some(next)) => !old.catalog_matches(next),
            _ => true,
        },
        _ => true,
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn targeted_activity_patch(
    spec: &SubscriptionSpec,
    torrent_id: &str,
    previous_view: &TorrentView,
    next_view: &TorrentView,
    previous_verified: &[IndexRange],
    next_verified: &[IndexRange],
    previous_active: &BTreeMap<u32, ActivePiece>,
    next_active: &BTreeMap<u32, ActivePiece>,
    file_upsert: &[FileView],
) -> Option<ViewPatch> {
    match (&spec.selector, spec.projection) {
        (ViewSelector::TorrentList, ViewProjection::Summary) => {
            (previous_view != next_view).then(|| ViewPatch::TorrentList {
                upsert: vec![next_view.clone()],
                removed: Vec::new(),
                storage: None,
            })
        }
        (
            ViewSelector::Torrent {
                torrent_id: selected,
            },
            ViewProjection::Summary,
        ) if selected == torrent_id => (previous_view != next_view).then(|| ViewPatch::Torrent {
            torrent: Some(next_view.clone()),
        }),
        (
            ViewSelector::Torrent {
                torrent_id: selected,
            },
            ViewProjection::PieceActivity,
        ) if selected == torrent_id => {
            let verified = difference(next_verified, previous_verified);
            let cleared = difference(previous_verified, next_verified);
            let (active_upsert, active_removed) = active_piece_patch(previous_active, next_active);
            (!verified.is_empty()
                || !cleared.is_empty()
                || !active_upsert.is_empty()
                || !active_removed.is_empty())
            .then(|| ViewPatch::PieceActivity {
                torrent_id: torrent_id.to_owned(),
                piece_count: next_view.piece_count,
                verified,
                cleared,
                active_upsert,
                active_removed,
            })
        }
        (
            ViewSelector::Torrent {
                torrent_id: selected,
            },
            ViewProjection::Files,
        ) if selected == torrent_id && !file_upsert.is_empty() => Some(ViewPatch::Files {
            torrent_id: torrent_id.to_owned(),
            upsert: file_upsert.to_vec(),
            removed: Vec::new(),
        }),
        _ => None,
    }
}

pub(super) fn targeted_peer_patch(
    spec: &SubscriptionSpec,
    torrent_id: &str,
    previous_view: &TorrentView,
    next_view: &TorrentView,
    previous_peers: &BTreeMap<String, PeerView>,
    next_peers: &BTreeMap<String, PeerView>,
) -> Option<ViewPatch> {
    match (&spec.selector, spec.projection) {
        (ViewSelector::TorrentList, ViewProjection::Summary) => {
            (previous_view != next_view).then(|| ViewPatch::TorrentList {
                upsert: vec![next_view.clone()],
                removed: Vec::new(),
                storage: None,
            })
        }
        (
            ViewSelector::Torrent {
                torrent_id: selected,
            },
            ViewProjection::Summary,
        ) if selected == torrent_id => (previous_view != next_view).then(|| ViewPatch::Torrent {
            torrent: Some(next_view.clone()),
        }),
        (
            ViewSelector::Torrent {
                torrent_id: selected,
            },
            ViewProjection::Peers,
        ) if selected == torrent_id => {
            peer_collection_patch(torrent_id, previous_peers, next_peers)
        }
        _ => None,
    }
}

pub(super) fn targeted_swarm_patch(
    spec: &SubscriptionSpec,
    torrent_id: &str,
    previous: &SwarmModel,
    current: &SwarmModel,
) -> Option<ViewPatch> {
    match (&spec.selector, spec.projection) {
        (
            ViewSelector::Torrent {
                torrent_id: selected,
            },
            ViewProjection::Swarm,
        ) if selected == torrent_id => swarm_collection_patch(torrent_id, previous, current),
        _ => None,
    }
}

pub(super) fn targeted_tracker_patch(
    spec: &SubscriptionSpec,
    torrent_id: &str,
    previous_view: &TorrentView,
    next_view: &TorrentView,
    previous_trackers: &BTreeMap<String, TrackerView>,
    next_trackers: &BTreeMap<String, TrackerView>,
) -> Option<ViewPatch> {
    match (&spec.selector, spec.projection) {
        (ViewSelector::TorrentList, ViewProjection::Summary) => {
            (previous_view != next_view).then(|| ViewPatch::TorrentList {
                upsert: vec![next_view.clone()],
                removed: Vec::new(),
                storage: None,
            })
        }
        (
            ViewSelector::Torrent {
                torrent_id: selected,
            },
            ViewProjection::Summary,
        ) if selected == torrent_id => (previous_view != next_view).then(|| ViewPatch::Torrent {
            torrent: Some(next_view.clone()),
        }),
        (
            ViewSelector::Torrent {
                torrent_id: selected,
            },
            ViewProjection::Trackers,
        ) if selected == torrent_id => {
            tracker_collection_patch(torrent_id, previous_trackers, next_trackers)
        }
        _ => None,
    }
}

fn peer_collection_patch(
    torrent_id: &str,
    previous: &BTreeMap<String, PeerView>,
    current: &BTreeMap<String, PeerView>,
) -> Option<ViewPatch> {
    let upsert = current
        .iter()
        .filter(|(id, peer)| previous.get(*id) != Some(*peer))
        .map(|(_, peer)| peer.clone())
        .collect::<Vec<_>>();
    let removed = previous
        .keys()
        .filter(|id| !current.contains_key(*id))
        .cloned()
        .collect::<Vec<_>>();
    (!upsert.is_empty() || !removed.is_empty()).then(|| ViewPatch::Peers {
        torrent_id: torrent_id.to_owned(),
        upsert,
        removed,
    })
}

fn swarm_collection_patch(
    torrent_id: &str,
    previous: &SwarmModel,
    current: &SwarmModel,
) -> Option<ViewPatch> {
    if previous == current {
        return None;
    }
    let upsert = current
        .peers
        .iter()
        .filter(|(id, peer)| previous.peers.get(*id) != Some(*peer))
        .map(|(_, peer)| peer.clone())
        .collect();
    let removed = previous
        .peers
        .keys()
        .filter(|id| !current.peers.contains_key(*id))
        .cloned()
        .collect();
    Some(ViewPatch::Swarm {
        torrent_id: torrent_id.to_owned(),
        state: current.state,
        captured_millis: current.captured_millis.clone(),
        maximum_records: current.maximum_records,
        counts: current.counts.clone(),
        upsert,
        removed,
    })
}

fn tracker_collection_patch(
    torrent_id: &str,
    previous: &BTreeMap<String, TrackerView>,
    current: &BTreeMap<String, TrackerView>,
) -> Option<ViewPatch> {
    let upsert = current
        .iter()
        .filter(|(id, tracker)| previous.get(*id) != Some(*tracker))
        .map(|(_, tracker)| tracker.clone())
        .collect::<Vec<_>>();
    let removed = previous
        .keys()
        .filter(|id| !current.contains_key(*id))
        .cloned()
        .collect::<Vec<_>>();
    (!upsert.is_empty() || !removed.is_empty()).then(|| ViewPatch::Trackers {
        torrent_id: torrent_id.to_owned(),
        upsert,
        removed,
    })
}

pub(super) fn disk_patch(
    previous: &DiskSessionView,
    current: &DiskSessionView,
) -> Option<ViewPatch> {
    let upsert = current
        .pieces
        .iter()
        .filter(|(id, piece)| previous.pieces.get(*id) != Some(*piece))
        .map(|(_, piece)| piece.clone())
        .collect::<Vec<_>>();
    let removed = previous
        .pieces
        .keys()
        .filter(|id| !current.pieces.contains_key(*id))
        .cloned()
        .collect::<Vec<_>>();
    (previous.pipeline != current.pipeline || !upsert.is_empty() || !removed.is_empty()).then(
        || ViewPatch::SessionDisk {
            pipeline: current.pipeline.clone(),
            upsert,
            removed,
        },
    )
}

fn active_piece_patch(
    previous: &BTreeMap<u32, ActivePiece>,
    current: &BTreeMap<u32, ActivePiece>,
) -> (Vec<ActivePiece>, Vec<String>) {
    let upsert = current
        .iter()
        .filter(|(piece_index, piece)| previous.get(*piece_index) != Some(*piece))
        .map(|(_, piece)| piece.clone())
        .collect();
    let removed = previous
        .iter()
        .filter(|(piece_index, piece)| {
            current
                .get(*piece_index)
                .is_none_or(|current| current.piece_id != piece.piece_id)
        })
        .map(|(_, piece)| piece.piece_id.clone())
        .collect();
    (upsert, removed)
}

pub(super) fn coalesce(update: &mut ViewUpdate, next: &ViewUpdatePayload) -> bool {
    let (ViewUpdatePayload::Patch { patch: current }, ViewUpdatePayload::Patch { patch: next }) =
        (&mut update.payload, next)
    else {
        return false;
    };
    coalesce_patch(current, next)
}

pub(crate) fn coalesce_patch(current: &mut ViewPatch, next: &ViewPatch) -> bool {
    match (current, next) {
        (
            ViewPatch::TorrentList {
                upsert,
                removed,
                storage,
            },
            ViewPatch::TorrentList {
                upsert: next_upsert,
                removed: next_removed,
                storage: next_storage,
            },
        ) => {
            let mut values = upsert
                .drain(..)
                .map(|torrent| (torrent.torrent_id.clone(), torrent))
                .collect::<BTreeMap<_, _>>();
            for id in next_removed {
                values.remove(id);
            }
            for torrent in next_upsert {
                values.insert(torrent.torrent_id.clone(), torrent.clone());
            }
            let mut removed_ids = removed.drain(..).collect::<std::collections::BTreeSet<_>>();
            for torrent in next_upsert {
                removed_ids.remove(&torrent.torrent_id);
            }
            removed_ids.extend(next_removed.iter().cloned());
            *upsert = values.into_values().collect();
            *removed = removed_ids.into_iter().collect();
            if next_storage.is_some() {
                *storage = next_storage.clone();
            }
            true
        }
        (ViewPatch::Torrent { torrent }, ViewPatch::Torrent { torrent: next }) => {
            *torrent = next.clone();
            true
        }
        (
            ViewPatch::PieceActivity {
                torrent_id,
                piece_count,
                verified,
                cleared,
                active_upsert,
                active_removed,
            },
            ViewPatch::PieceActivity {
                torrent_id: next_id,
                piece_count: next_piece_count,
                verified: next_verified,
                cleared: next_cleared,
                active_upsert: next_active_upsert,
                active_removed: next_active_removed,
            },
        ) if torrent_id == next_id => {
            for range in next_cleared {
                remove_interval(verified, *range);
                insert_interval(cleared, *range);
            }
            for range in next_verified {
                remove_interval(cleared, *range);
                insert_interval(verified, *range);
            }
            *piece_count = *next_piece_count;
            let mut values = active_upsert
                .drain(..)
                .map(|piece| (piece.piece_id.clone(), piece))
                .collect::<BTreeMap<_, _>>();
            for id in next_active_removed {
                values.remove(id);
            }
            for piece in next_active_upsert {
                values.insert(piece.piece_id.clone(), piece.clone());
            }
            let mut removed_ids = active_removed.drain(..).collect::<BTreeSet<_>>();
            for piece in next_active_upsert {
                removed_ids.remove(&piece.piece_id);
            }
            removed_ids.extend(next_active_removed.iter().cloned());
            *active_upsert = values.into_values().collect();
            *active_removed = removed_ids.into_iter().collect();
            true
        }
        (
            ViewPatch::SessionDisk {
                pipeline,
                upsert,
                removed,
            },
            ViewPatch::SessionDisk {
                pipeline: next_pipeline,
                upsert: next_upsert,
                removed: next_removed,
            },
        ) => {
            let mut values = upsert
                .drain(..)
                .map(|piece| (piece.row_id.clone(), piece))
                .collect::<BTreeMap<_, _>>();
            for id in next_removed {
                values.remove(id);
            }
            for piece in next_upsert {
                values.insert(piece.row_id.clone(), piece.clone());
            }
            let mut removed_ids = removed.drain(..).collect::<BTreeSet<_>>();
            for piece in next_upsert {
                removed_ids.remove(&piece.row_id);
            }
            removed_ids.extend(next_removed.iter().cloned());
            *pipeline = next_pipeline.clone();
            *upsert = values.into_values().collect();
            *removed = removed_ids.into_iter().collect();
            true
        }
        (
            ViewPatch::SessionDht { inspection },
            ViewPatch::SessionDht {
                inspection: next_inspection,
            },
        ) => {
            *inspection = next_inspection.clone();
            true
        }
        (
            ViewPatch::SessionSpeed { history },
            ViewPatch::SessionSpeed {
                history: next_history,
            },
        ) => {
            *history = next_history.clone();
            true
        }
        (
            ViewPatch::Peers {
                torrent_id,
                upsert,
                removed,
            },
            ViewPatch::Peers {
                torrent_id: next_id,
                upsert: next_upsert,
                removed: next_removed,
            },
        ) if torrent_id == next_id => {
            let mut values = upsert
                .drain(..)
                .map(|peer| (peer.connection_id.clone(), peer))
                .collect::<BTreeMap<_, _>>();
            for id in next_removed {
                values.remove(id);
            }
            for peer in next_upsert {
                values.insert(peer.connection_id.clone(), peer.clone());
            }
            let mut removed_ids = removed.drain(..).collect::<std::collections::BTreeSet<_>>();
            for peer in next_upsert {
                removed_ids.remove(&peer.connection_id);
            }
            removed_ids.extend(next_removed.iter().cloned());
            *upsert = values.into_values().collect();
            *removed = removed_ids.into_iter().collect();
            true
        }
        (
            ViewPatch::Swarm {
                torrent_id,
                state,
                captured_millis,
                maximum_records,
                counts,
                upsert,
                removed,
            },
            ViewPatch::Swarm {
                torrent_id: next_id,
                state: next_state,
                captured_millis: next_captured_millis,
                maximum_records: next_maximum_records,
                counts: next_counts,
                upsert: next_upsert,
                removed: next_removed,
            },
        ) if torrent_id == next_id => {
            let mut values = upsert
                .drain(..)
                .map(|peer| (peer.peer_record_id.clone(), peer))
                .collect::<BTreeMap<_, _>>();
            for id in next_removed {
                values.remove(id);
            }
            for peer in next_upsert {
                values.insert(peer.peer_record_id.clone(), peer.clone());
            }
            let mut removed_ids = removed.drain(..).collect::<BTreeSet<_>>();
            for peer in next_upsert {
                removed_ids.remove(&peer.peer_record_id);
            }
            removed_ids.extend(next_removed.iter().cloned());
            *state = *next_state;
            *captured_millis = next_captured_millis.clone();
            *maximum_records = *next_maximum_records;
            *counts = next_counts.clone();
            *upsert = values.into_values().collect();
            *removed = removed_ids.into_iter().collect();
            true
        }
        (
            ViewPatch::Files {
                torrent_id,
                upsert,
                removed,
            },
            ViewPatch::Files {
                torrent_id: next_id,
                upsert: next_upsert,
                removed: next_removed,
            },
        ) if torrent_id == next_id => {
            let mut values = upsert
                .drain(..)
                .map(|file| (file.file_id.clone(), file))
                .collect::<BTreeMap<_, _>>();
            for id in next_removed {
                values.remove(id);
            }
            for file in next_upsert {
                values.insert(file.file_id.clone(), file.clone());
            }
            let mut removed_ids = removed.drain(..).collect::<std::collections::BTreeSet<_>>();
            for file in next_upsert {
                removed_ids.remove(&file.file_id);
            }
            removed_ids.extend(next_removed.iter().cloned());
            *upsert = values.into_values().collect();
            *removed = removed_ids.into_iter().collect();
            true
        }
        (
            ViewPatch::Trackers {
                torrent_id,
                upsert,
                removed,
            },
            ViewPatch::Trackers {
                torrent_id: next_id,
                upsert: next_upsert,
                removed: next_removed,
            },
        ) if torrent_id == next_id => {
            let mut values = upsert
                .drain(..)
                .map(|tracker| (tracker.tracker_id.clone(), tracker))
                .collect::<BTreeMap<_, _>>();
            for id in next_removed {
                values.remove(id);
            }
            for tracker in next_upsert {
                values.insert(tracker.tracker_id.clone(), tracker.clone());
            }
            let mut removed_ids = removed.drain(..).collect::<std::collections::BTreeSet<_>>();
            for tracker in next_upsert {
                removed_ids.remove(&tracker.tracker_id);
            }
            removed_ids.extend(next_removed.iter().cloned());
            *upsert = values.into_values().collect();
            *removed = removed_ids.into_iter().collect();
            true
        }
        (
            ViewPatch::Diagnostics { events, retention },
            ViewPatch::Diagnostics {
                events: next_events,
                retention: next_retention,
            },
        ) => {
            if events.len().saturating_add(next_events.len()) > MAX_DIAGNOSTIC_PATCH_EVENTS
                || patch_encoded_len(events).saturating_add(patch_encoded_len(next_events))
                    > MAX_DIAGNOSTIC_PATCH_BYTES
            {
                return false;
            }
            events.extend(next_events.iter().cloned());
            *retention = next_retention.clone();
            true
        }
        _ => false,
    }
}
