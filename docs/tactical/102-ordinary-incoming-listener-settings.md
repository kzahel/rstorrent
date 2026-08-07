# Tactical 102: Ordinary Incoming Listener Settings

Status: Implemented

## Outcome

Make the product treatment of BitTorrent incoming traffic match an ordinary
client: incoming TCP is enabled, listens on all IPv4 interfaces, and asks the
user only whether the port is automatic or fixed. Keep disabled and loopback
bootstrap policies available below the product UI for controlled tests and
headless tooling.

## Scope

- Bind the ordinary automatic and fixed TCP listener modes to `0.0.0.0`.
- Bind coordinated session UDP to the same wildcard address and selected port.
- Retain a best-effort concrete routed IPv4 address separately for UPnP and
  peer-advertisement bookkeeping; inability to select one must not prevent the
  wildcard listener from starting.
- Replace the product's five listener-policy choices with **Automatic port**
  and **Fixed port**.
- Do not expose the preferred automatic candidate, bind scope, disabled mode,
  or loopback mode in product settings.
- Preserve the internal serialized policies so tests, controlled CLI flows,
  and existing profile data remain readable.

## Invariants

1. Fresh product settings continue to enable ordinary incoming connections.
2. Automatic mode emits `automatic_local_network`; fixed mode emits
   `fixed_local_network` and validates `1024..=65535`.
3. Both ordinary policies bind IPv4 wildcard TCP and UDP sockets.
4. Disabled and loopback policies remain explicit non-product mechanisms.
5. A fixed-port conflict remains a typed recoverable failure; automatic mode
   retains bounded successor and system-port fallback.
6. Web administration listener and authentication settings remain a separate
   concern and are not presented in this section.

## Non-goals

- IPv6 listeners, per-interface selection, firewall control, or additional
  port-mapping protocols.
- Removing legacy listener variants from the persisted contract.
- Adding a normal-product switch that disables incoming traffic.

## Validation And Evidence

- Engine tests assert ordinary TCP and coordinated UDP bind to wildcard while
  a concrete routed address remains available for reachability bookkeeping.
- React tests assert only Automatic and Fixed are presented and that saving a
  fixed port emits the ordinary all-interface policy.
- The live settings test exercises persistence and recovery through the two
  product choices without loopback or disabled controls.

## Stopping Condition

Stop when ordinary modes bind wildcard sockets, product settings expose only
Automatic and Fixed, focused Rust and web tests pass, the owning topic records
the corrected behavior, and the completed slice is committed.
