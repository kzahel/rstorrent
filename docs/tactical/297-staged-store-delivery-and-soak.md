# Tactical 297: Staged Store Delivery And Soak

Status: Active, 2026-10-10. Owners: beta-release-readiness and
product-surfaces-and-migration. Continues the bounded shipment 295 checkpoint.

## Scope and stopping condition

Desktop 0.3.0, source downloads, website and production feed are public and
verified. Android 28 is in review at 5% with managed publishing on; CWS 1.1.3
was last observed pending with publication deferred. A final refresh expired
the Web Store session; its passkey verification needs the maintainer's physical
interaction before current status can be checked. Original submission receipts
remain valid historical evidence. Play quick checks have completed.

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
3. Release approved Android 28 at 5%, verify the Play-installed canary's signer,
   retained library/grants/paused intent and native/extension reconnect.
4. Record the actual rollout start; observe 24 hours before expansion. Stop on
   confirmed payload loss, competing writers or systematic startup failure,
   following the existing operational recovery plan. Do not invent a soak pass.

Unrun second-device, physical Intel, reboot, sleep, grant-loss and background
upload cases remain explicit in 259. Current VM-off and Chromebook cleanup
receipts do not qualify those routes. Linux remains best effort; iOS remains
out of scope. Translation detail and small Files/Storage spacing notes are
presentation follow-ups, not evidence of package changes.

Stop when actual public store delivery, canaries and staged observation are
recorded, or when a concrete external review/device dependency prevents the
next check. Reports/images stay ignored and machines are cleared between use.
