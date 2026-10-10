# Tactical 297: Staged Store Delivery And Soak

Status: Active, 2026-10-10. Owners: beta-release-readiness and
product-surfaces-and-migration. Continues the bounded shipment 295 checkpoint.

## Scope and stopping condition

Desktop 0.3.0, source downloads, website and production feed are public and
verified. On October 10 the maintainer renews Web Store authentication; the
existing item still shows Pending review. CWS 1.1.3 publication stays deferred.
Play approves all 92 changes, including exact Android 28 and its 5% rollout.
Publication is confirmed at 16:51 UTC; production shows release 28 available to
5% of users, and release 23 remains available. Managed publishing stays on.
Separate ignored report 088 retains approval, confirmation and actual rollout
screenshots. The actual public Play listing shows the new three-image gallery,
copy, October 10 update date and data-safety disclosure. This does not prove
production-channel installation. Fresh cleanup passes on both Chromebooks;
no VM was booted. Console crash/ANR metrics are unavailable, not a zero-failure pass.
The 24-hour observation cannot finish before October 11 at 16:51 UTC; elapsed
time alone does not qualify expansion.

Google approvals are external dependencies, not missing authorization.
Preserve exact ff632 package bindings,
existing app/store identities, original signing roots and disabled hosted
identifier/counter gates. Do not rebuild or relabel these packages.
Store updates may publish independently; one approval does not require waiting
for the other store when the documented mixed-version recovery story applies.

Next executable steps:

1. Record actual approvals or address specific rejection findings. Reconcile
   dependency-review expiry/current advisories before later publication.
2. Publish the approved extension, then verify its actual store-installed
   version on the available machines. Preserve the documented mixed-version
   recovery story if Android review finishes first.
3. Keep Android 28 at 5%; verify an actual production-channel installed canary's
   signer, retained library/grants/paused intent and native/extension reconnect.
   Existing internal-track 28 installations do not prove production delivery.
   Read-only report088 confirms cohort B remains on the earlier internal 28;
   cohort A has no connected ADB device and its proxy is not listening.
4. Record the actual rollout start; observe 24 hours before expansion. Stop on
   confirmed payload loss, competing writers or systematic startup failure,
   following the existing operational recovery plan. Do not invent a soak pass.

Unrun second-device, physical Intel, reboot, sleep, grant-loss and background
upload cases remain explicit in 259. Current VM-off and Chromebook cleanup
receipts do not qualify those routes. Linux remains best effort; iOS remains
out of scope. Translation detail and small Files/Storage spacing notes are
presentation follow-ups, not evidence of package changes.

Play's release dashboard reports DEX obfuscation at 0%, due February 2027,
and an edge-to-edge recommendation. Record both for the next Android slice;
neither prevented approval or the staged release. Do not rebuild the frozen
candidate during this rollout. Dependency review remains valid through
October 12, with the previously recorded GLib remediation and rustls 0.23.45.

Stop when actual public store delivery, canaries and staged observation are
recorded, or when a concrete external review/device dependency prevents the
next check. Reports/images stay ignored and machines are cleared between use.
