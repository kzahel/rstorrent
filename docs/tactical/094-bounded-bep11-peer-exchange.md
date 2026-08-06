# Tactical 094: Bounded BEP 11 Peer Exchange

Status: Authorized and in progress on 2026-08-06. Gate 1 general bounded
BEP 10 negotiation and PEX codecs are the current executable action.

Topics: `protocol-support`, `peer-lifecycle`, `download-correctness`,
`application-control`, `incoming-reachability-and-seeding`,
`capability-readiness`

Dependencies: completed Tacticals
[`010`](010-peer-registry-magnet-failover.md),
[`016`](016-dht-discovery-foundation.md),
[`017`](017-adversarial-multi-peer-liveness.md),
[`081`](081-v1-torrent-byte-intake.md),
[`086`](086-long-lived-torrent-peer-runtime.md), and
[`089`](089-coordinated-session-listen-sockets.md) establish bounded source-
attributed peer records, private-metadata transitions, multiple handshaken
connections, verified metainfo privacy, retained peer ownership, and truthful
listen-socket facts. Completed Tactical
[`090`](090-peer-id-duplicate-connection-resolution.md) and
[`092`](092-truthful-tracker-and-dht-peer-advertisement.md) are hard
prerequisites. Completed Tacticals `091` and `093` precede PEX in the recorded
completeness sequence but are not wire dependencies.

## Decision And Motivation

Implement one bounded, bidirectional BEP 11 `ut_pex` capability through the
ordinary BEP 10 connection and peer-registry owners.

PEX can provide fresher peer liveness than tracker or DHT intervals and reduce
repeated discovery load after a swarm has bootstrapped. It also accepts
attacker-selected endpoints from an untrusted peer and can be abused to fill
the registry or make distributed connection attempts toward victim ranges.
The first slice must therefore implement source diversity, duplicate-address,
rate, entry, byte, privacy, network-policy, and lifecycle bounds together with
the common path.

Receive-only parsing or sending only `added` entries would not be a complete
slice. Advertising `ut_pex` commits RSTorrent to connection-local extension
IDs, bounded incoming admission, and exact `added`/`dropped` lifecycle.

This tactical owns stable correctness scenario DL-C32 in
[`download-correctness`](../topics/download-correctness.md).

## Stopping Condition

This tactical is complete when all of the following hold:

1. one general bounded BEP 10 per-connection extension map supports
   connection-local IDs, repeated additive handshakes, disable-by-zero, and
   unknown-extension ignore behavior for `ut_metadata` and `ut_pex`;
2. verified public torrents negotiate `ut_pex`, while private or privacy-
   unknown torrents neither advertise nor accept PEX and a private transition
   purges PEX-only candidates before content scheduling;
3. incoming PEX obeys exact compact strides, bencode, flag, byte, entry,
   duplicate-address, source, cadence, and network-policy bounds before adding
   observations to the ordinary peer registry;
4. outgoing PEX reports only fully handshaken endpoints known to be
   connectable, sends no more than once per minute, and eventually sends one
   matching `dropped` event for every advertised live peer;
5. one PEX source cannot monopolize the registry, dial budget, or one victim
   IP/subnet through alternate ports, and PEX-only records retain exact source
   provenance for purging and diagnostics;
6. disconnect, extension disable, metadata privacy change, pause, torrent
   removal, and application shutdown release all diff/cursor state and join
   through existing connection owners; and
7. deterministic hostile-input, scripted lifecycle, controlled two-hop
   RSTorrent/libtorrent, and exact high-water evidence pass.

## Scope

### BEP 10 prerequisite growth

- Replace the `ut_metadata`-specific negotiated-ID shape with a small
  connection-local map of recognized extensions. Unknown names are ignored and
  do not allocate retained arbitrary strings beyond the bounded handshake
  parser.
- Preserve separate local receive IDs and peer-advertised send IDs. A repeated
  handshake is additive; value zero disables only the named extension.
- Parse the bounded BEP 10 listen-port field `p`. It is a claimed remote TCP
  port, not proof of reachability, and is used only where connection direction
  and address policy make it safe to construct a PEX contact.
- Keep current BEP 9 negotiation and metadata transfer behavior unchanged.

### Incoming PEX

- Decode `added`, `added.f`, `added6`, `added6.f`, `dropped`, and `dropped6`
  from one bounded bencoded payload.
- Cap the extension payload at 16 KiB. Process at most 50 combined IPv4/IPv6
  additions and 50 combined drops in every message, including the first.
  A standards-compliant larger initial list may be truncated and diagnosed;
  it is not grounds to allocate or admit beyond the product bound.
- Accept one initial message and then at most one message per 60-second window
  using an injectable monotonic clock. Egregious repeated violations close the
  connection after a small fixed strike bound; ordinary buffering jitter must
  not create an unbounded timestamp history.
- Require compact strings to have exact six-byte IPv4 or eighteen-byte IPv6
  stride. Optional flag strings must match the associated peer count. Unknown
  flag bits are ignored and known flags remain untrusted hints.
- Reject zero, unspecified, multicast, broadcast, and policy-ineligible
  endpoints. Local/private addresses are accepted only from a source on an
  eligible local network under the existing network policy.
- Ignore duplicate IP addresses from one message/source even when ports differ.
  Do not delete or rewrite an independently tracker-, DHT-, manual-, or
  incoming-observed endpoint merely because PEX supplied a conflicting value.
- Retain at most 50 PEX-only candidate records attributable to one live source
  and 200 PEX-only candidates per torrent. Existing total registry and dial
  ceilings remain stricter where applicable.

### Outgoing PEX

- Advertise only successfully handshaken peers. Outgoing connections have a
  known target endpoint; incoming connections require a validated BEP 10 `p`
  and eligible address before they can be propagated.
- Never advertise a raw incoming socket source port, an unverified mapped
  endpoint, a banned peer, a privacy-ineligible peer, or the receiving peer
  itself.
- Send an initial bounded snapshot only when useful, then at most one diff per
  minute. Elide transient add/drop pairs and never add and drop the same
  endpoint in one message.
- Retain one bounded torrent event timeline plus a fixed cursor per negotiated
  connection rather than a full peer-set copy per connection. If a cursor
  falls behind bounded history, send a fresh bounded snapshot and resume.
- Set only flags backed by current authoritative state. Unsupported encryption,
  uTP, hole-punch, and upload-only facts remain clear; an outgoing/reachable
  hint is set only when the endpoint meets the BEP's meaning.

## Non-Goals

- PEX before verified metadata establishes that the torrent is public.
- Recently disconnected underpopulated-list exemptions. They may be a later
  compatible slice after the live-only lifecycle is proven.
- BEP 40 Canonical Peer Priority, although this tactical records selection
  evidence needed to decide whether it should follow.
- BEP 21 upload-only, encryption, uTP, hole punching, LSD, IPv6 DHT runtime,
  tracker changes, peer caches, or durable PEX state.
- Trusting PEX seed/reachability flags as verified capability or changing
  connection replacement policy based solely on them.
- A plugin framework or arbitrary user-defined BEP 10 extension registry.

## Normative And Reference Dossier

Pinned BEP revision `7b7b41f46d57ff1d1cb1e24ed6e9bacfbf958c06`
was inspected:

- `reference/bittorrent.org/beps/bep_0010.rst` defines connection-local
  directional IDs, repeated additive handshakes, disable-by-zero, `p`, and
  unknown-name behavior;
- `reference/bittorrent.org/beps/bep_0011.rst` defines `ut_pex`, compact
  contacts and flags, fully established `added` peers, matching `dropped`
  events, one-minute cadence, 50-contact normal diffs, and hostile-source
  considerations;
- `reference/bittorrent.org/beps/bep_0027.rst` restricts private torrents to
  private-tracker peers, excluding PEX discovery; and
- `reference/bittorrent.org/beps/bep_0040.rst` supplies the optional canonical
  peer-priority defense referenced by BEP 11.

Pinned libtorrent revision
`7d7fc38fac61177fa5e02148f791b2f65250b09d` was inspected:

- `reference/libtorrent/src/ut_pex.cpp` builds minute-level diffs, admits only
  bounded PEX peers, parses IPv4/IPv6 fields and flags, filters local addresses,
  and handles cadence violations;
- `reference/libtorrent/include/libtorrent/extensions/ut_pex.hpp` owns the
  extension entry point; and
- `reference/libtorrent/include/libtorrent/settings_pack.hpp` defaults
  `max_pex_peers` to 50.

The pinned implementation permits a much larger wire payload and its source
uses a 100-addition diff in places despite the current BEP's 50-contact normal
limit. RSTorrent follows its declared tighter bounds and the pinned normative
text. No dedicated comprehensive PEX test file was found at the pin, so
independently authored hostile and lifecycle tests are required.

The first-party JSTorrent checkout was inspected:

- `packages/engine/src/extensions/pex-handler.ts` receives `added` and
  `added6` contacts through a negotiated ID; and
- `packages/engine/test/extensions/pex-handler.test.ts` covers basic handshake
  and compact-contact cases.

The handler does not implement the required dropped lifecycle, cadence,
source-diversity, byte/entry, or private-transition evidence and is therefore
a warning reference rather than a target. No source or fixtures are imported.

## Owner, Task, And Data-Flow Map

```text
verified public torrent + admitted live connection
                         |
              BEP 10 connection-local map
                         |
          +--------------+--------------+
          v                             v
bounded PEX decoder/admission      torrent live-peer event timeline
          |                             |
network/privacy/source filters     per-connection bounded cursor
          |                             |
          v                             v
ordinary PeerObservation(PEX)      ordered extension writer action
          |
bounded peer registry -> existing dial selector and connection budgets
```

- Deterministic BEP 10/11 codecs, maps, rate state, diff state, and admission
  decisions contain no sockets or tasks.
- The torrent peer owner supplies verified privacy state and authoritative live
  connection events.
- The peer registry remains the only source merger and dial-candidate owner.
  PEX does not open sockets or bypass dial policy.
- Existing connection tasks carry extension frames through their bounded
  queues and existing cancellation/join paths; PEX adds no task.
- Network policy is applied before registry mutation and again before dial, as
  with other untrusted discovery sources.

## Resource And Security Invariants

| Resource | Initial bound |
| --- | --- |
| PEX extension payload | 16 KiB |
| Additions per message | 50 combined IPv4/IPv6 |
| Drops per message | 50 combined IPv4/IPv6 |
| Send/accepted cadence | One initial, then at most one per 60 seconds |
| PEX-only records per source | 50 |
| PEX-only records per torrent | 200, beneath the existing total registry cap |
| Retained timeline | At most 4,096 normalized add/drop events; lagging cursors reset to a snapshot |
| Per-connection state | Recognized extension IDs, cadence/strike state, and one timeline cursor; no full registry copy |

- Parsing and filtering complete before any peer-record or dial state changes.
- A malformed message either has no effect or follows one typed close policy;
  partially admitted prefixes are forbidden.
- One source cannot create multiple accepted records for one IP by varying the
  port. IPv4 and IPv4-mapped IPv6 normalization is explicit.
- PEX source provenance is retained when an endpoint merges with an existing
  record and can be removed independently during a private transition.
- Dropped means the sender no longer reports a live connection; it does not ban
  or delete independently sourced peer knowledge.
- All PEX-only state is volatile, bounded, and cleared on torrent generation
  termination.

## Implementation Gates

1. Generalize the recognized BEP 10 map and prove unchanged BEP 9 behavior,
   repeated handshakes, and disable transitions.
2. Add pure bounded PEX parsing, normalization, source quotas, privacy/network
   filters, and atomic admission decisions.
3. Add the live-peer timeline, cursor/reset behavior, exact add/drop diffs, and
   one-minute scheduling through existing connection actions.
4. Integrate with the peer registry without bypassing candidate selection,
   duplicate peer-ID resolution, capacity, or retry policy.
5. Prove private-metadata transition, pause/removal/shutdown cleanup, malicious
   source isolation, and exact resource high-water marks.
6. Run a controlled two-hop topology in which PEX is the only source for the
   second hop, then update BEP 10/11 and readiness claims to the exact passing
   subset.

## Validation Matrix

| Layer | Required evidence |
| --- | --- |
| Codec/state | compact strides, flags, missing/empty fields, duplicate/add-drop conflicts, malformed bencode, 16-KiB ceiling, 50-entry caps, repeated extension handshakes, disable-by-zero |
| Security/admission | same-IP alternate ports, self endpoint, local/multicast/unspecified ranges, source quota, total PEX quota, private/unknown metadata, PEX plus tracker/DHT provenance |
| Lifecycle | established-only add, matching drop, transient elision, cursor lag/reset, incoming without `p`, disconnect, pause, removal, shutdown, one-minute cadence and bounded strikes |
| Scripted runtime | fragmented/coalesced frames, event/writer backpressure, malicious flood, source disconnect, registry saturation, cancellation and exact joins |
| Controlled interoperability | pinned libtorrent seed introduces a second independently controlled peer through PEX; RSTorrent dials it and verifies content, then observes its drop |
| Resource evidence | payload bytes, decoded items, normalized contacts, per-source/total records, timeline, cursors, queues, dials, sockets, tasks, and terminal zeros |

No public swarm, visible client, schema migration, new dependency, or physical
device is required.

## Escalation And Next Boundary

Ordinary pure-module extraction, additional hostile cases, conservative bound
tightening, and internal selection details are authorized only when this
tactical is explicitly selected for implementation. Stop for direction if
interoperability requires accepting PEX before privacy is known, weakening
source/network bounds, persisting PEX state, changing private-torrent policy,
or adding BEP 40 or underpopulated recently-seen behavior in this slice.

After this tactical, evidence may select BEP 40, the underpopulated-list
extension, persisted peer caches, or another measured discovery gap. PEX does
not itself authorize uTP or hole punching.
