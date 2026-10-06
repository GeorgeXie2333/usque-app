package io.github.georgexie2333.usque

/** Keeps tile presentation separate from full diagnostics and contains recovery-read failures. */
internal object TileControlPolicy {
    fun recoveryPresentation(hasRecoveryProfile: () -> Boolean): QuickSettingsTileState.Presentation =
        try {
            if (hasRecoveryProfile()) QuickSettingsTileState.active() else QuickSettingsTileState.inactive()
        } catch (_: Exception) {
            // A failed read is unknown; it must not crash the colocated VPN process.
            QuickSettingsTileState.pending("checking")
        }

    inline fun <T> snapshotFor(
        what: Int,
        tileSnapshot: () -> T,
        fullSnapshot: () -> T,
    ): T =
        when (what) {
            UsqueVpnService.MSG_TILE_SNAPSHOT, UsqueVpnService.MSG_TILE_TOGGLE -> tileSnapshot()
            else -> fullSnapshot()
        }
}
