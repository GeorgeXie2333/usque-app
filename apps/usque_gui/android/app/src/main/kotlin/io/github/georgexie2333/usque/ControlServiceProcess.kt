package io.github.georgexie2333.usque

/**
 * A missing control-service process proves that [UsqueVpnService] is not
 * running, so a passive read need not wait for a binding that cannot connect.
 * A running process proves nothing because the tile service shares it. Any
 * doubt reports the process as present, which keeps the ordinary bind deadline.
 */
internal object ControlServiceProcess {
    /** Matches the `android:process` attribute of [UsqueVpnService]. */
    const val SUFFIX = ":vpn"

    fun knownAbsent(
        packageName: String,
        runningProcessNames: List<String?>?,
    ): Boolean =
        packageName.isNotEmpty() &&
            runningProcessNames != null &&
            // A list without this UI process is not a trustworthy view of the app.
            packageName in runningProcessNames &&
            runningProcessNames.none { it == packageName + SUFFIX }
}
