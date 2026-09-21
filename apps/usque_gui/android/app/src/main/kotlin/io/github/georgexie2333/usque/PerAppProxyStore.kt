package io.github.georgexie2333.usque

import android.annotation.SuppressLint
import android.content.Context

internal object PerAppProxyStore {
    const val PREFERENCES = "usque_per_app_proxy_v1"
    const val KEY_ENABLED = "enabled"
    const val KEY_PACKAGES = "package_names"

    fun preferences(context: Context): AtomicPolicyStore = AndroidPolicyStore.perApp(context)

    fun load(
        context: Context,
        selfPackage: String = context.packageName,
    ): PerAppProxySettings {
        val values = preferences(context).snapshot()
        val stored =
            PerAppProxySettings(
                enabled = values[KEY_ENABLED] as? Boolean ?: false,
                packageNames =
                    (values[KEY_PACKAGES] as? Set<*>)?.map {
                        require(it is String)
                        it
                    } ?: emptyList(),
            )
        return PerAppProxyRules.sanitize(stored, selfPackage)
    }

    @SuppressLint("ApplySharedPref", "UseKtx")
    fun save(
        context: Context,
        settings: PerAppProxySettings,
        selfPackage: String = context.packageName,
    ): PerAppProxySettings {
        val error = PerAppProxyRules.validationError(settings, selfPackage)
        if (error != null) {
            throw PerAppProxyStoreException(error)
        }
        val sanitized = PerAppProxyRules.sanitize(settings, selfPackage)
        check(
            preferences(context)
                .edit()
                .putBoolean(KEY_ENABLED, sanitized.enabled)
                .putStringSet(KEY_PACKAGES, sanitized.packageNames.toSet())
                .commit(),
        ) {
            "Android could not persist per-app proxy settings"
        }
        return sanitized
    }

    @SuppressLint("ApplySharedPref", "UseKtx")
    fun clear(context: Context) {
        check(preferences(context).edit().clear().commit()) {
            "Android per-app proxy settings could not be cleared"
        }
    }
}
