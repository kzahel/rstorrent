# Peer Flag Vocabulary

Topic: `peer-flag-vocabulary`

Status: Implemented by Tactical `051` on 2026-08-02. Rust now projects the
typed semantic set, generated v1 bindings carry it additively, and React uses
one exhaustive definition table for compact cells and the accessible legend.
The compact glyph mapping remains explicitly revisable as broader client
comparison and real inspection use provide evidence. Planned Tactical
[`086`](../tactical/086-long-lived-torrent-peer-runtime.md) will make the
existing incoming, upload-relationship, metadata, and optimistic-unchoke
variants truthful for routed incoming seed connections.

Related topics: [`peer-lifecycle`](peer-lifecycle.md),
[`application-view-api`](application-view-api.md), and
[`web-ui-design`](web-ui-design.md).

## Purpose

The Peers table needs a compact way to expose direction, transfer relationship,
transport, negotiated capabilities, and exceptional scheduler state. Those
facts are useful only if every letter has one stable meaning and unavailable
engine state is not presented as false.

This topic owns:

- the semantic peer-flag catalog exposed by Rust application views;
- the provisional compact glyph and short user-facing label for each semantic
  flag;
- the ownership split between engine facts, application projection, and
  client presentation;
- the mature-client references used to select the initial vocabulary; and
- the boundary between current truthful support and documented future flags.

It does not make a flag true merely because another client exposes it. The
engine owner must implement and observe the corresponding state first.

## Observed Defect

The initial React adapter constructed one opaque string:

```text
E = peer supports BEP 10 extensions
I = RSTorrent is interested
C = peer is choking RSTorrent
```

That mapping is both incomplete and incompatible with the product references:

- libtorrent's example uses `x` for extension support, `I` for local interest,
  `C` for local choke, and lowercase `c` for remote choke;
- JSTorrent uses `E` for encryption and `I` for an incoming connection; and
- JSTorrent uses `D`/`d` and `U`/`u` pairs to convey both interest direction
  and the corresponding choke state.

Consequently an `EI` cell in RSTorrent can be read as “encrypted incoming” by
a JSTorrent user while actually meaning “extensions supported; locally
interested.” A legend for that accidental subset would document the defect
rather than fix it.

The deterministic demo adapter compounds the problem by emitting historical
`d`, `D`, and `u` strings while the live adapter emits `E`, `I`, and `C`.
There is no single current vocabulary.

## Reference Dossier

The external reference revisions are pinned by `reference/pins.toml`:

- Rasterbar libtorrent revision
  `7d7fc38fac61177fa5e02148f791b2f65250b09d` (`v2.0.13`); and
- JSTorrent revision `9895410beeed6aff554053769bd006a3fbd373ef`.

### Rasterbar libtorrent

- `include/libtorrent/peer_info.hpp::peer_info` defines current interest and
  choke directions, extension support, outgoing direction, handshake/connect
  lifecycle, parole, seed, optimistic unchoke, snubbed, upload-only, endgame,
  hole-punch, I2P/uTP/SSL transport, and encryption flags. It separately
  defines tracker, DHT, PEX, LSD, resume, and incoming source flags plus
  per-direction disk/rate/network blocking state.
- `src/peer_connection.cpp::peer_connection::get_peer_info` projects scheduler,
  integrity, source, lifecycle, transport, and blocking facts from their
  actual owners rather than deriving them in a UI.
- `src/bt_peer_connection.cpp::get_specific_peer_info` projects the four
  interest/choke directions, extension support, outgoing direction,
  handshake/connect state, transport, and encryption.
- `examples/client_test.cpp::print_peer_info` renders separate compact segments
  for peer flags, download/upload blocking reasons, and discovery sources.
- `examples/client_test.cpp::print_peer_legend` provides a complete terminal
  legend instead of expecting users to infer the characters.
- `test/test_peer_list.cpp` covers incoming and source-related peer-record
  behavior; `test/test_fast_extension.cpp` checks observed seed flag state.
  The broader parole, endgame, and optimistic-unchoke behavior is owned by its
  picker, torrent, and simulation tests rather than the display helper.

Adopted lessons are that flags are derived from authoritative engine state,
interest and choke have two directions, unusual scheduler state remains
visible, incoming direction matters, and source/blocking dimensions should not
be silently folded into the same opaque string.

RSTorrent does not copy libtorrent's bit positions, terminal color encoding,
class ownership, exact characters, I2P surface, or the convention that
incoming is represented only by absence of its outgoing flag.

### JSTorrent

- `packages/ui/src/tables/PeerTable.tsx::formatFlags` uses `E` for encrypted,
  `I` for incoming, `D`/`d` for download interest under unchoked/choked state,
  and `U`/`u` for remote interest under locally unchoked/choked state.
- `packages/engine/src/core/peer-coordinator/types.ts` retains incoming
  direction as a typed connection fact rather than reconstructing it from an
  endpoint or discovery source.

Adopted lessons are the user-facing incoming/encryption meanings and paired
upper/lowercase transfer notation. RSTorrent does not copy the component,
mutable engine objects, or assume JSTorrent's current six symbols exhaust
mature peer diagnostics.

No reference source, fixture, or asset is imported by this work. The source is
used to establish independently authored vocabulary and tests.

## Ownership And Dependency Direction

```text
peer registry / connection task / scheduler / protocol negotiation
                             |
                 coherent engine observation
                             |
          Rust application projection computes semantic flags
                             |
        generated PeerFlagView values in bounded peer rows
                             |
       client model maps semantic values to glyphs and labels
                             |
        React cell accessibility + one header legend popover
```

Rust owns whether a semantic flag is present. It must derive each flag from
the same coherent connection-generation observation used for the rest of the
peer row. React does not infer parole, seed, encryption, optimistic unchoke,
or endgame state from rates, logs, client names, or timing.

Rust does **not** own the compact character, ordering, color, spacing, tooltip,
or localization. The application contract carries a typed semantic enum, not
an opaque `"EI"` string. The web client owns one data-driven definition table
that maps those enum values to glyphs and labels. The visible glyphs may
therefore evolve toward another client convention without changing engine
state or the wire meaning.

Within API v1 the peer flag field is additive and defaults to an empty set for
an older producer. The live adapter may derive only the already-represented
initial subset from older v1 row fields as a compatibility fallback; new Rust
producers are authoritative. Unknown closed semantic variants require a
contract capability or version decision rather than silent reinterpretation.

## Initial Semantic Catalog And Provisional Glyphs

The semantic enum reserves the mature states already justified by the pinned
references. Rust emits only states it currently owns.

| Semantic flag | Provisional glyph | Meaning | Initial RSTorrent state |
| --- | --- | --- | --- |
| `incoming` | `I` | Remote peer initiated this connection | Live intake now joins ordinary peer observation; compact projection lands in Tactical `086` Gate 4 |
| `encrypted` | `E` | Peer transport is encrypted or obfuscated | Reserved; no application-view fact yet |
| `download_allowed` | `D` | We are interested and the peer is not choking us | Derivable for current content peers |
| `download_choked` | `d` | We are interested but the peer is choking us | Derivable for current content peers |
| `upload_allowed` | `U` | Peer is interested and we are not choking it | Upload owner implements the fact; ordinary connection projection remains in Tactical `086` |
| `upload_choked` | `u` | Peer is interested but we are choking it | Upload owner implements the fact; ordinary connection projection remains in Tactical `086` |
| `extension_protocol` | `x` | Peer supports the BEP 10 extension protocol | Negotiated and represented |
| `metadata_extension` | `m` | Peer advertised the BEP 9 `ut_metadata` extension | Contract field exists; incoming negotiation projection remains in Tactical `086` |
| `utp` | `T` | Connection uses uTP transport | Transport vocabulary exists; runtime uTP remains test-only |
| `hole_punched` | `h` | Connection succeeded through NAT hole punching | Reserved; not implemented |
| `on_parole` | `p` | Integrity policy restricts this peer after suspect data | Reserved; current connection view has no full parole fact |
| `optimistic_unchoke` | `O` | Peer occupies an optimistic unchoke slot | Upload scheduler implements the grant; connection projection remains in Tactical `086` |
| `snubbed` | `S` | Peer is under degraded request policy after timeout | Reserved; current `stalled` phase is not silently equated with libtorrent snubbing |
| `upload_only` | `L` | Peer reports it will not download from us | Reserved; not implemented |
| `endgame` | `e` | This connection is participating in endgame requests | Reserved; no connection-scoped view fact yet |
| `seed` | `s` | Peer is known to have every piece | Reserved; piece availability is currently unavailable |

`T` and `L` intentionally differ from libtorrent's terminal characters because
`u`/`U` are retained for JSTorrent-compatible upload relationship state.
Connecting, handshaking, connected, stalled, and disconnecting remain in the
State column. TCP and outgoing are ordinary defaults represented by absence
of `T` and `I`; the accessible cell label still expands every present flag.

Tracker, DHT, PEX, LSD, incoming observation, manual, magnet-hint, and cache
provenance remain the Source dimension. A peer may accumulate multiple
sources. The current React adapter collapses that set to one prioritized label;
retaining all source provenance is a known separate presentation improvement,
not a reason to overload the Flags string.

Per-direction disk, bandwidth-limit, and network blocking reasons also remain
a separate future dimension, following libtorrent's example. They should not
be confused with choke state.

## Presentation Contract

The common cell renders glyphs as one compact sequence in canonical catalog
order. It has one accessible label containing the names of every present flag,
so a screen reader does not announce an unexplained sequence of letters. Empty
means no currently present/known flag and renders an em dash; it never means
all mature states were observed false.

The Flags header contains a visible help button separate from its sort control.
Activating it by mouse, keyboard, or touch opens a nonmodal legend grouped as:

1. connection and transport;
2. transfer relationship;
3. negotiated capability; and
4. exceptional scheduler/integrity state.

The legend is deliberately terse: it contains only compact section labels and
case-sensitive glyph/name pairs. It omits a redundant title, introductory
copy, and per-flag descriptions; this topic remains the detailed semantics
reference.
The button does not sort the column. Escape and outside activation dismiss the
popover while preserving predictable focus. A hover-only `title` tooltip is
not the primary interaction.

The generic virtual-table column type may accept optional header help, but it
does not own peer vocabulary. The peer-specific definition table supplies the
legend body and cell formatter.

## Invariants

- One semantic enum value has one meaning across Rust, generated schema,
  traces, demo state, sorting, accessibility text, and every web layout.
- Rust emits each semantic flag at most once and in canonical order.
- A paired transfer flag is emitted only when interest is true and the matching
  choke direction is known. Unknown is not converted to choked or allowed.
- `incoming` derives from connection direction, not from discovery source.
- `utp` derives from transport, not client name or endpoint.
- `extension_protocol` and `metadata_extension` remain distinct capabilities.
- Stalled request phase is not labeled snubbed until the engine owns the
  mature degraded-request transition.
- Demo rows use typed semantic flags and cannot invent a second character
  vocabulary.
- The UI never parses glyphs back into application or engine state.
- The flag set remains bounded by the closed semantic catalog and adds no
  payload, bitfield, request list, or history to the application boundary.

## Evolution Policy

Real client comparison may justify changing characters, ordering, grouping,
or whether a state receives a dedicated column. Such presentation changes
update this topic and the frontend definition table but do not rename semantic
Rust variants merely to resemble another client's glyphs.

Adding a new semantic state requires:

1. an identified engine owner and exact transition meaning;
2. reference and protocol review where relevant;
3. unavailable/false/true semantics in the application projection;
4. deterministic owner and generated-contract tests;
5. a short user-facing label and accessible cell meaning; and
6. explicit compatibility handling for the closed enum.

Do not infer a new flag from a diagnostic message, rate heuristic, UI timer,
client fingerprint, or another client's name for a superficially similar
state.

## Current Implementation And Evidence

`PeerView::from_observation` computes the canonical typed set after mapping the
coherent connection-generation observation. It currently emits incoming,
download allowed/choked, upload allowed/choked when both nullable inputs are
known, extension protocol, metadata extension when represented, and uTP. It
does not emit reserved variants from lifecycle names, rates, timing, or demo
policy.

The optional generated v1 field preserves old-producer compatibility. The web
validator bounds it to the 16-value catalog and rejects duplicate or unknown
state. The live adapter prefers any present Rust list and uses only a bounded
typed-fact fallback when an older producer omits it. Demo rows use the same
semantic values.

The Flags cell, sort key, full accessible label, and four-section legend all
come from `clients/web/src/inspection/peerFlags.ts`. A distinct 24 px header
help button opens keyboard-scrollable content, never sorts the column, and
dismisses with Escape or outside activation. The 2026-08-03 presentation
refinement reduced the legend to a 260 px compact glyph/name table with 11 px
regular-weight type, 16 px single-line rows, low-profile section labels, no
redundant title, and no explanatory prose.
Standard Light and Compact Dark passed viewport-bound and serious/critical axe
checks. The full Rust workspace, generated drift, TypeScript, unit/component,
production build, and headless browser suites passed; exact commands and
counts remain in Tactical `051`.

## Current Gaps

- The engine does not yet project encryption, hole-punch, seed, upload-only,
  connection parole, or connection-scoped endgame facts. Optimistic upload
  grants exist but are not yet attached to ordinary connection observations.
- Incoming and uTP are representable and tested vocabulary. Incoming TCP is a
  live capability but is not yet attached to the ordinary peer owner; uTP
  remains vocabulary only.
- Upload interest/choke and incoming metadata-extension facts exist in their
  runtime owners but are currently unavailable or unsupported in Peers rows.
- Per-peer source presentation collapses accumulated sources to one label.
- Per-peer disk/rate/network block reasons are not projected.
- The eventual Swarm view will need record-state flags distinct from this
  active connection-generation vocabulary.

These gaps are documented future work, not permission to show placeholders as
observed peer state. Planned Tactical
[`086`](../tactical/086-long-lived-torrent-peer-runtime.md) owns the incoming,
upload relationship, metadata, and optimistic-grant subset; the other reserved
flags remain outside that slice.
