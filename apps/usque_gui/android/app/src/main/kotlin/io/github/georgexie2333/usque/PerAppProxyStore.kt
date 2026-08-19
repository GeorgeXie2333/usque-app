package io.github.georgexie2333.usque

import android.annotation.SuppressLint
import android.content.Context
import android.content.SharedPreferences

internal object PerAppProxyStore {
    const val PREFERENCES = "usque_per_app_proxy_v1"
    const val KEY_ENABLED = "enabled"
    const val KEY_PACKAGES = "package_names"

    fun preferences(context: Context): SharedPreferences =
        context.applicationContext
            .createDeviceProtectedStorageContext()
            .getSharedPreferences(PREFERENCES, Context.MODE_PRIVATE)

    fun load(
        context: Context,
        selfPackage: String = context.packageName,
    ): PerAppProxySettings {
        val prefs = preferences(context)
        val stored =
            PerAppProxySettings(
                enabled = prefs.getBoolean(KEY_ENABLED, false),
                packageNames = prefs.getStringSet(KEY_PACKAGES, emptySet())?.toList() ?: emptyList(),
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
