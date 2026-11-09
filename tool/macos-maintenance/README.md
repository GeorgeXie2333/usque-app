# macOS maintenance foundation

This is an unsigned-development implementation and isolated test target. It is
not an enabled production updater. `MacReleaseTrust.allowedTeamIDs` is empty,
so the CLI rejects every operation except read-only `--status` before reading
requests, mounting images, or creating installation staging. No environment
variable, argument, or editable plist can override this policy.

The expected package contract follows the existing Windows helper: a stable
three-part version newer than the installed app, exact DMG name and architecture,
bounded size, and SHA-256. Future configured releases must also pass Apple-issued
same-product signing checks, nested-code validation, Gatekeeper assessment and
Mach-O architecture checks. The helper mounts verified images read-only.

Application replacement uses a private same-volume staging directory, bounded
owner-checked journal files, a serialized lock, synchronized staged files and
directories, and `RENAME_SWAP`. The installed path remains present. A journal
records both bundle digests and both architectures, allowing an Intel build on
Apple Silicon to update to arm64 and restore the Intel backup if necessary.
Recovery refuses unknown bytes. Backups remain after a separate health
acknowledgement. User configuration, credentials and system extension state are
outside this transaction.

Health acknowledgement now uses a versioned, bounded report bound to the exact
transaction UUID, candidate bundle digest, version and architecture. A health
verifier must authenticate its evidence; the default verifier rejects every
report because the live application health channel is not integrated. There is
no caller-controlled `componentsHealthy` Boolean or production CLI confirmation
shortcut. Evidence stays in memory; only its canonical report digest and replay
identifier are persisted. The confirmed state and receipt share one atomic
journal commit. An identical committed retry works after reopening the store,
including after report expiry; changed, stale or unauthenticated reports cannot
confirm a transaction. Rollback retains the receipt and both app versions.
Journal schema 3 validates completed records before a later update can replace
the active journal.

Run `bash tool/test_macos_maintenance.sh` for thirteen isolated contract groups.
These use temporary app fixtures and injected bundle and health verifiers. They cover failed
verification, retained backups, rollback, unknown-byte preservation, Rosetta
selection, journal/lock file restrictions, bounded subprocesses, and interruption
between atomic replacement and journal acknowledgement, confirmation retries
across store instances, rejected health evidence, invalid completed journals,
and receipt preservation during rollback. Injected health evidence does not
establish authenticated communication with the running app. These tests do not establish
that Developer ID signing, notarization, real application upgrades, actual power
loss recovery, or extension replacement has passed on hardware.

Build with `bash tool/build_macos_maintenance.sh arm64` or `x86_64`. Output is
`build/macos-maintenance/<architecture>/usque-maintenance`. The normal GUI update
entry remains disabled for these unsigned development bundles. System extension
upgrades, approval/restart handling, authenticated health acknowledgement,
privileged authorization, and real upgrade acceptance are separate pending work.
