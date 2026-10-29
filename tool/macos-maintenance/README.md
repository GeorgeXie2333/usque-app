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

Run `bash tool/test_macos_maintenance.sh` for nine isolated contract groups.
These use temporary app fixtures and an injected verifier. They cover failed
verification, retained backups, rollback, unknown-byte preservation, Rosetta
selection, journal/lock file restrictions, bounded subprocesses, and interruption
between atomic replacement and journal acknowledgement. They do not establish
that Developer ID signing, notarization, real application upgrades, actual power
loss recovery, or extension replacement has passed on hardware.

Build with `bash tool/build_macos_maintenance.sh arm64` or `x86_64`. Output is
`build/macos-maintenance/<architecture>/usque-maintenance`. The normal GUI update
entry remains disabled for these unsigned development bundles. System extension
upgrades, approval/restart handling, authenticated health acknowledgement,
privileged authorization, and real upgrade acceptance are separate pending work.
