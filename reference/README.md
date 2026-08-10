# Local Reference Checkouts

This directory holds reproducible source checkouts used to understand,
compare, and test BitTorrent behavior. Checkout contents are gitignored;
`README.md` and `pins.toml` are the tracked description and machine-readable
manifest.

Run:

```bash
python3 scripts/references.py sync
python3 scripts/references.py status
python3 scripts/references.py sync --only bittorrent-beps --only libtorrent
```

The script uses Python 3.11 or newer and only the standard library.

`--only NAME` limits either command to one named manifest record and may be
repeated. This permits a DHT reference survey without inspecting or updating an
independently dirty sibling checkout.

`sync` clones missing external checkouts, checks them out at exact detached
revisions, initializes explicitly declared submodules, and fast-forwards a clean
first-party JSTorrent `main` branch. It refuses to replace local changes,
unexpected origins, divergent branches, or repositories at the wrong path.

`status` is read-only and fails when a required checkout is missing, dirty, at
the wrong revision or branch, or configured with an unexpected origin. For a
branch-tracking sibling, status compares against the locally fetched
`origin/main`; run `sync` when current upstream state matters.

## Managed Sources

| Local path | Revision policy | Role |
| --- | --- | --- |
| `reference/bittorrent.org` | `7b7b41f46d57ff1d1cb1e24ed6e9bacfbf958c06` | Authoritative offline BEP source documents and protocol index |
| `reference/rqbit` | `4e5f94cbcf1d57ec500885c77cf1e24d70232d89` | Native Rust/Tokio engine, application API, storage, uTP, and implementation comparison |
| `reference/libtorrent` | `v2.0.13` (`7d7fc38fac61177fa5e02148f791b2f65250b09d`) | Primary external interoperability oracle and broad protocol reference |
| `reference/libutp` | `2b364cbb0650bdab64a5de2abb4518f9f228ec44` | Standalone MIT uTP source, C callback API, and build comparison; the pin has no maintained test suite |
| `reference/librqbit-utp` | `c26f57b2debbe35ed0ace1ad419de529f7a5bf95` (crates.io `0.7.0`) | Rust/Tokio uTP source, tests, task model, and dependency comparison |
| `../jstorrent` | first-party `main` | Product behavior, fixtures, integration scenarios, and Android/ChromeOS lessons |

The bittorrent.org checkout provides the original reStructuredText BEP sources
under `reference/bittorrent.org/beps/`. It supersedes JSTorrent's convenient
but stale Markdown conversion from a May 2020 snapshot and includes the
complete upstream repository at the declared revision. Use the local files for
offline reading and cite the BEP number in implementation and tests.

The upstream repository does not state one repository-wide license. Individual
BEPs may contain their own public-domain or other copyright statement. Keep the
checkout ignored rather than vendoring the prose, and inspect the exact
document before copying any of its text or assets into a tracked RSTorrent file.

The rqbit pin follows the current 9.0 release-candidate development line rather
than the older 8.1.1 stable crate. It includes the implementation we want to
study—especially uTP, full IPv6, local peer discovery, current engine
organization, and the initial BitTorrent v2 groundwork. The exact commit keeps
that moving branch reproducible.

The libtorrent pin matches the 2.0.13 Python binding used by JSTorrent's
integration environment. The source checkout and Python package have different
roles: the checkout supports reading and provenance; the binding runs black-box
peers and fixture creation. Its submodules are deliberately not initialized;
in particular, the GPL `simulation/libsimulator` checkout is outside this
project's reference and product dependency set.

The standalone libutp pin captures BitTorrent's MIT-licensed callback-oriented
C/C++ implementation for Tactical `118`'s source, build, and ownership
comparison. It is not an accepted product dependency or source donor.

The librqbit-utp pin is the exact VCS revision embedded in crates.io package
`0.7.0`. The matching package is locked by rqbit at checksum
`4f3bfdc73944bc76cab24d5690a98816770040a654c449edf5ff2b9ba22626aa`.
The checkout exposes tests and repository metadata omitted or normalized by
the package archive; neither form is an accepted RSTorrent dependency.

JSTorrent remains a sibling because it is a first-party product repository that
may be maintained independently. Do not create a second copy under
`reference/`.

## Source-Use Rules

- These repositories are references and test peers, not RSTorrent product
  dependencies.
- Keep managed external checkouts detached, clean, and at their declared pins.
- Read and preserve the source license before copying any source, fixture, or
  test data. rqbit is Apache-2.0; Rasterbar libtorrent's main library is
  BSD-3-Clause but its root `LICENSE` records file-level exceptions; JSTorrent
  is MIT.
- The bittorrent.org checkout is for local specification reading. Its
  per-document copyright statements vary, so do not vendor BEP prose merely
  because the repository is public.
- Do not use libtorrent's GPL-3.0 `simulation/libsimulator` submodule in the
  RSTorrent product or initial oracle harness.
- Prefer independently authored tests against public protocol behavior.
- Do not commit reference repositories, their build outputs, downloads, packet
  captures, or temporary investigation artifacts.
- Change a pin deliberately and update this map with the reason and relevant
  validation.

See [`../docs/references.md`](../docs/references.md) for the project-wide
reference and provenance policy.
