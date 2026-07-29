# Product Vision: JSTorrent Rebuilt Around A Native Engine

Status: directional long-term vision; product naming and graduation timing
remain open.

## Thesis

RSTorrent is likely the incubation project for a new generation of JSTorrent,
not a permanently separate product.

The implementation is independent because the engine needs a clean
architecture, not because the existing product identity should be discarded.
Once the native engine and its clients are demonstrably ready, they may
graduate into the JSTorrent product and replace the current TypeScript engine
under the hood.

The `JS` in JSTorrent describes the project's origin, not necessarily a
permanent implementation constraint or a promise users depend on. The durable
meaning of the brand is closer to "just torrent": an approachable client and
engine with unusually good platform, browser, and application integration.

## Product Promise

The successor should preserve and improve the qualities that made JSTorrent
valuable:

- simple installation and operation for people who do not want to administer a
  torrent daemon;
- first-class desktop, Android, and ChromeOS experiences;
- excellent browser and web integration;
- useful automation and integration surfaces;
- understandable diagnostics and support behavior; and
- one coherent product rather than several loosely conforming engines.

This is not a Rust rewrite for its own sake. Rust is the means to make one
first-party engine own networking, protocol state, hashing, scheduling,
persistence, and hot-path data movement across supported native products.

## Intended Product Shape

Native clients should normally run the engine in-process:

```text
Desktop UI ─────────── in-process adapter ─┐
Android UI ─────────── in-process adapter ─┼──> typed application service
CLI and automation ─── in-process adapter ─┤              │
Browser extension ── authenticated control ┘              ▼
                                                  Rust torrent engine
```

The extension is ultimately a first-party control and integration surface, not
a second torrent engine. It may add torrents, present state, initiate actions,
integrate browser workflows, and surface results. Peer sockets, piece payloads,
hashing, scheduling, and ordinary file I/O remain in the native engine.

The exact extension transport is deliberately undecided. Native messaging,
local authenticated IPC, or another narrow control mechanism requires an
explicit architecture and security decision. The vision does not authorize a
generic socket proxy, filesystem proxy, public daemon API, or REST/WebSocket
service during engine bring-up.

## Incubation And Graduation

The separate RSTorrent name and repository create room to build deeply without
destabilizing the current JSTorrent product or treating its internals as
compatibility constraints.

A likely progression is:

1. Build and validate the engine under the RSTorrent working name.
2. Ship an explicitly experimental native client to learn from real use.
3. Establish reliable desktop and Android/ChromeOS products around the same
   engine.
4. Design migration and the browser-extension control boundary from proven
   application contracts.
5. Graduate the implementation into the JSTorrent product and brand when it is
   safer and more useful than the engine it replaces.
6. Retire legacy engine paths deliberately rather than maintaining two product
   architectures indefinitely.

This is product succession, not a line-by-line port. Early RSTorrent tacticals
do not inherit JSTorrent feature parity, API compatibility, persistence
compatibility, or UI reproduction as acceptance criteria. Compatibility work
is added only when it serves an actual graduation path.

## Graduation Evidence

The name or default engine should not change merely because a demonstration
works. Graduation should be supported by evidence that includes:

- reliable downloading, seeding, verification, resume, and recovery for the
  deliberately supported protocol surface;
- interoperability against mature clients and real swarms;
- bounded resource use and measured hot-path performance;
- actionable diagnostics and support tooling;
- physical Android/ChromeOS lifecycle and storage validation;
- stable application contracts used by more than one first-party client;
- a reviewed migration path for user settings, torrent state, and content;
- a secured and lifecycle-aware extension control channel, if the extension is
  part of that release; and
- an upgrade, coexistence, rollback, and retirement plan for the current
  JSTorrent implementation.

Native performance and a single shared engine are strong architectural
advantages, but performance and reliability claims should be measured rather
than assumed.

## What This Vision Does Not Require

- Renaming the repository or preview client now.
- Concealing an experimental engine behind the stable JSTorrent release.
- Reproducing the current TypeScript module graph or process topology.
- Removing TypeScript, Kotlin, or other appropriate UI and integration code.
- Building the browser extension before native application contracts exist.
- Turning the native engine into a general-purpose remote daemon.
- Preserving every historical feature before the new client is useful.
- Ending maintenance of the current JSTorrent product before a responsible
  transition is available.

## Open Product Questions

- Whether RSTorrent remains only an internal/incubation name or is used for
  public previews.
- When the implementation is mature enough to carry the JSTorrent name.
- Which settings, torrent state, and integration contracts need migration.
- How an existing extension discovers, authenticates, and coordinates with the
  native product.
- Which web integrations belong in the extension, local application API, or
  product UI.
- How long the current and successor implementations should coexist.

These questions should be answered from working engine and client evidence.
They are not prerequisites for the first protocol bring-up tactical.
