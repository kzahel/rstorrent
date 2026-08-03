# Tactical 056: Peer Client Identification

Status: In progress on 2026-08-03.

Topics: `peer-lifecycle`, `application-view-api`, `web-ui-design`

## Motivation And Outcome

The active-peer contract and React table already carry a nullable
`client_name`, but the Rust projection always emits `null` and marks the field
unsupported. The visible Client column therefore shows an em dash even after
the BitTorrent handshake has supplied the peer ID needed to identify common
clients such as µTorrent, qBittorrent, Transmission, and Deluge.

Add one runtime-independent, bounded peer-ID identification function in the
protocol crate. Project its result through the existing `PeerView.client_name`
field and capability status so the existing live adapter and Client column
display it without a parallel frontend parser or a view/API version change.

## Stable Scenarios

- `-UT3550-...` displays `µTorrent 3.5.5`.
- `-qB4500-...`, `-TR3000-...`, and `-DE2000-...` display their registered
  client names and versions.
- Base-64-like Azureus version digits identify `-LT20D0-...` as
  `libtorrent 2.0.13`, rather than treating `D` as zero or malformed.
- `-lt0D80-...` displays `rTorrent 0.13.8`; the similarly named Rasterbar
  `LT` code remains `libtorrent`.
- RSTorrent, JSTorrent, rqbit, µTorrent Web, and WebTorrent codes receive
  first-party or current registered names.
- Registered Shadow-style and Mainline-style IDs display bounded versions.
- Common BEP 20 legacy BitComet/BitLord, XBT, Opera, and Tixati IDs identify
  without reading beyond the fixed 20-byte peer ID.
- A structurally valid Azureus-style unknown code displays the printable
  two-character code and version, matching mature-client diagnosability.
- Random, truncated, control-character, malformed-version, and all-zero peer
  IDs remain unidentified; the Client column displays `—` while the separate
  peer-ID field retains its hex evidence.
- Before handshake completion the field is unavailable. After a recognized
  handshake it is available. An unrecognized peer ID remains unavailable,
  never fabricated as `Unknown [...]`.

## Scope

- Add a pure, task-free peer-ID client identifier to `rstorrent-protocol`.
- Recognize the BEP 20 Azureus-style convention with a broad registered-name
  table and bounded version formatting.
- Recognize the registered Shadow style, Mainline style, and a small set of
  precisely described nonstandard BEP 20 formats used by common clients.
- Include current first-party `RS`/`JS`, rqbit `rQ`, and current WebTorrent
  family identifiers.
- Populate the existing session `PeerView.client_name` and publish truthful
  `Available`/`Unavailable` capability state.
- Add protocol parser, application projection, TypeScript adapter, and
  existing-table rendering evidence.
- Update the owning topics and tactical index with exact validation.

## Non-goals

- Change `VIEW_CONTRACT_VERSION`, `API_VERSION`, generated TypeScript, JSON
  Schema, routes, or storage formats. The nullable field already exists.
- Claim that a client fingerprint is authenticated identity. Peer IDs are
  peer-controlled hints and may be spoofed.
- Preserve libtorrent's `Unknown [....................]` fallback or expose
  arbitrary peer-controlled bytes as a client label.
- Mechanically copy libtorrent's source table, parser, tests, or fixtures.
- Parse every historical one-off peer-ID convention ever observed.
- Parse or prioritize the BEP 10 extended-handshake `v` string. RSTorrent's
  current extension-handshake value does not retain it; adding that distinct
  source and precedence rule belongs to a later bounded slice.
- Move client-name parsing into React or infer a client from endpoint,
  transport, flags, behavior, or user agent.
- Add a client icon, filter, grouping policy, tooltip, or another column.
- Commit, push, publish, or launch a visible product client without separate
  authorization.

## Normative And Reference Dossier

No reference code, fixture, or test data is copied.

- Pinned BEP checkout revision
  `7b7b41f46d57ff1d1cb1e24ed6e9bacfbf958c06`,
  `beps/bep_0020.rst`, defines the 20-byte peer-ID purpose, Mainline,
  Azureus-style, Shadow-style, BitComet/BitLord, XBT, Opera, and other known
  conventions. It is the naming and wire-shape starting point.
- Pinned Rasterbar libtorrent 2.0.13 revision
  `7d7fc38fac61177fa5e02148f791b2f65250b09d`:
  `include/libtorrent/identify_client.hpp`,
  `src/identify_client.cpp::{parse_az_style,parse_shadow_style,
  parse_mainline_style,lookup,identify_client_impl}`,
  `test/test_identify_client.cpp::identify_client`, and
  `src/bt_peer_connection.cpp::{on_extended_handshake,read_peer_id,
  get_specific_peer_info}` establish the completeness oracle. The adopted
  behavior is structured format recognition, base-36 Azureus digits, three
  required version components with an optional nonzero fourth, correct
  `LT`/`lt` distinction, and backend-owned projection. Intentional differences
  are stricter ASCII validation, a smaller public-convention name catalog,
  no arbitrary-byte Unknown label, and no BEP 10 override in this slice.
- Local JSTorrent sibling HEAD
  `9895410beeed6aff554053769bd006a3fbd373ef`:
  `packages/ui/src/utils/format.ts::{TORRENT_CLIENTS,parseClientName}` and
  `packages/ui/src/tables/PeerTable.tsx` show the existing first-party product
  expectation for Azureus-style parsing and Client-column fallback. RSTorrent
  moves this responsibility out of React, expands format coverage, uses
  base-36 digits, corrects `lt` to rTorrent, and does not use a peer-ID hex
  prefix as a client name.
- Pinned rqbit revision
  `4e5f94cbcf1d57ec500885c77cf1e24d70232d89`,
  `crates/librqbit_core/src/peer_id.rs::{try_decode_peer_id,
  try_decode_azureus_style,AzureusStyleKind}` confirms its `rQ` code and the
  wider 64-character version alphabet. RSTorrent accepts the conventional
  `0-9A-Za-z.-` alphabet while independently implementing correct digit
  values and bounded display.

## Accepted Design

`rstorrent_protocol::peer_id::identify_client([u8; 20]) -> Option<String>` is
the sole parser. It operates on the already length-bounded handshake value and
has no I/O, clock, retained state, background task, or dependency.

Recognition order is deliberately specific before general:

1. precisely shaped nonstandard BEP 20 prefixes whose version meaning is
   known;
2. Azureus style `-XXvvvv-` with two printable client-code bytes and four
   digits from `0-9A-Za-z.-`;
3. registered Shadow style with three displayable version digits or the
   documented binary form; and
4. Mainline `M<major>-<minor>-<tiny>--`, with each decimal component bounded
   to three digits.

Azureus lookup returns a registered name when known and the sanitized
two-character code otherwise. Its display is `name major.minor.revision`, plus
`.tag` only when the fourth component is nonzero. This follows libtorrent's
useful display convention while retaining all parsed components in the local
function. The 64-character alphabet is decoded only to values 0--63.

The application mapper computes the name once per complete peer observation.
It emits `CapabilityStatus::Available` exactly when a name is present and
`Unavailable` otherwise. `Unsupported` is no longer truthful because the
projection now owns this capability. The existing nullable string, generated
contract, validator, live adapter, client model, sort behavior, cell title,
and fallback rendering remain unchanged.

## Invariants And Bounds

- Input is exactly the handshake's 20-byte peer ID; no peer-controlled
  allocation or variable-length scan precedes validation.
- Every parser checks structural delimiters and character classes before
  decoding or formatting.
- Decimal parsing checks overflow even though at most three digits are
  admitted.
- The returned label is at most 128 UTF-8 bytes, matching the browser
  validator. Tests assert the bound for every registered code.
- No control character, random suffix, nickname, endpoint, or arbitrary byte
  enters `client_name`.
- A fingerprint is display evidence only. It does not affect duplicate-peer,
  trust, scheduling, choke, integrity, ban, or connection policy.
- `peer_id` remains the engine observation fact; `client_name` is a
  deterministic application projection of it and owns no second lifecycle.
- The parser and projection introduce no task, cancellation path, queue,
  retained collection, dependency, or unbounded work.

## Ownership And Data Flow

```text
peer socket handshake task
  -> Handshake.peer_id: [u8; 20]
  -> torrent-owned PeerConnectionObservation.peer_id
  -> PeerView::from_observation
       -> rstorrent_protocol::peer_id::identify_client
       -> existing client_name + capability
  -> existing generated API field
  -> existing LiveApplication mapPeer
  -> existing PeerTable Client cell
```

The socket and torrent coordinator retain their current lifecycle and
cancellation ownership. The identifier is a pure protocol utility; the
session projection owns whether and how its result crosses the application
boundary; React owns only display and sorting.

## Edge-Case Checklist

- common Azureus decimal versions and a zero versus nonzero fourth component;
- uppercase, lowercase, dot, and dash version digits at values 10--63;
- registered, current first-party, and printable unknown client codes;
- `LT` libtorrent versus `lt` rTorrent;
- Shadow ASCII and binary versions, including unknown one-byte codes;
- Mainline one- and multi-digit components, missing delimiters, and too-long
  components;
- BitComet versus BitLord marker and binary decimal version bytes;
- XBT release/debug marker, Opera build digits, and Tixati prefix;
- control bytes in a code, invalid version bytes, misplaced trailing dash,
  random printable IDs, all zeros, and near matches;
- no peer ID before handshake, known peer ID, and unrecognized peer ID;
- capability `Unavailable`/`Available` transitions in complete keyed upserts;
- live TypeScript adapter preservation of a non-null client string; and
- the existing Client column's visible value, title, fallback, and sorting.

## Validation Plan

- `cargo fmt --all -- --check`
- `cargo clippy --workspace -- -D warnings`
- `cargo test --workspace`
- protocol tests for every accepted format family, common client mappings,
  malformed inputs, exact formatting, and output bound;
- session view-set test for handshake-driven client population and capability;
- generated-contract drift check proving no schema/type/version change;
- web typecheck, unit suite, and production build;
- targeted headless browser assertion that a Client value remains visible in
  the existing Peer table under light and dark themes; and
- remove all temporary browser evidence before completion.

## Stopping Condition

This slice is complete when a recognized handshake peer ID produces a bounded
Rust-owned client/version label in the existing API, the unchanged live React
path visibly renders it, unknown/malformed IDs remain honest and safe, all
specified validation passes, owning topics record the result, and no protocol
or view version changes.
