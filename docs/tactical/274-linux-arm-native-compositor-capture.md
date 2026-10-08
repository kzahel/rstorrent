# Tactical 274: Linux ARM Native Compositor Capture

Status: Active, 2026-10-08. Qualification-only follow-up to 259.

Topic: `web-ui-design`.

## Scope and stopping condition

Historical original-signed ARM migration passes eight assertions and six
refusals, but ordinary native PNG capture returns transparent pixels throughout
the actual application frame. This does not establish a rendering defect.

First check the installed GNOME session's native Screencast D-Bus interface and
record five seconds with its default encoder into an owned temporary path.
If that route supplies usable actual pixels, repeat the unchanged real old-
writer populated migration helper and capture settled migrated/restarted
frames through the same route. Keep source and artifact identities explicit:
historical evidence cannot qualify the unbuilt final signed source.

Stop after an actual supported recording and populated visual adjudication,
or a concrete interface/provider refusal, with exported receipts, restored
state, removed owned guest artifacts, verified shutdown and released claim.
Do not substitute transparent captures or accessibility for pixel acceptance.

## Invariants, ownership and bounds

Machine Control owns target selection, claim, guest transport and lifecycle.
The active user's existing GNOME service owns compositor recording. One live
Gio connection owns start/stop, with five seconds of recording and bounded
D-Bus calls. The controller exports only its owned returned file, verifies its
hash and removes it and its exact recent-file URI after export. The host's
existing ffmpeg extracts frames without editing their content.

Prepare the controller before boot. Use a ten-minute claim for the prerequisite
check and a fifteen-minute whole bound for any subsequent populated rehearsal.
Finally cleanup is serialized before shutdown; do not issue guest commands
after verified shutdown. No outer VM UI, graphics flags, permission-store edits,
authentication bypass, dependencies, product state injection, cache variation,
release or store change. An unsupported/refused service remains a visible gap.

## Reference and validation

Read GNOME Shell 46.0's original interface XML, Screencast service and base
D-Bus service. The interface returns the actual filename; the service tracks
the caller and StopScreencast belongs to that same live caller. Default encoder
selection belongs to GNOME. Inspect installed service introspection/version
before claiming availability; no source is imported.

- [Interface XML](https://raw.githubusercontent.com/GNOME/gnome-shell/46.0/data/dbus-interfaces/org.gnome.Shell.Screencast.xml)
- [Recording service](https://raw.githubusercontent.com/GNOME/gnome-shell/46.0/js/dbusServices/screencast/screencastService.js)
- [Base D-Bus service](https://raw.githubusercontent.com/GNOME/gnome-shell/46.0/js/dbusServices/dbusService.js)

Validate the actual returned recording with independent size/hash and decoded
frames, then inspect real app pixels where available. Record refusals without
retrying through an undocumented bypass. Existing Rust/Android app baselines
remain applicable; this slice changes no product code.
