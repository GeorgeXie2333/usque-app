package io.github.georgexie2333.usque

import android.content.Context
import java.io.File

internal object AndroidPolicyStore {
    private val startupKeys = setOf(UsqueVpnService.START_ON_BOOT, "boot_connect_pending")

    private fun open(
        context: Context,
        name: String,
        legacy: String,
        accept: (String) -> Boolean,
    ): AtomicPolicyStore {
        val device = context.applicationContext.createDeviceProtectedStorageContext()
        return AtomicPolicyStore(File(device.filesDir, "policy-v2/$name.json")) {
            device.getSharedPreferences(legacy, Context.MODE_PRIVATE).all.filterKeys(accept)
        }
    }

    fun recovery(context: Context): AtomicPolicyStore =
        open(context, "recovery", UsqueVpnService.RECOVERY_PREFERENCES) { it !in startupKeys }

    fun startup(context: Context): AtomicPolicyStore =
        open(context, "startup", UsqueVpnService.RECOVERY_PREFERENCES) { it in startupKeys }

    fun perApp(context: Context): AtomicPolicyStore =
        open(context, "per-app", PerAppProxyStore.PREFERENCES) {
            it == PerAppProxyStore.KEY_ENABLED || it == PerAppProxyStore.KEY_PACKAGES
        }

    fun clear(context: Context) {
        // Persist empty tombstones; deleting new files could re-import legacy data.
        for (store in listOf(recovery(context), startup(context), perApp(context))) {
            check(store.edit().clear().commit()) { "Policy cleanup failed" }
        }
    }
}
