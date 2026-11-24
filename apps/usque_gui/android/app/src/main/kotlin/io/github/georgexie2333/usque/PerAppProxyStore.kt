package io.github.georgexie2333.usque

import android.annotation.SuppressLint
import android.content.Context
import java.security.MessageDigest

internal object PerAppProxyStore {
    const val PREFERENCES = "usque_per_app_proxy_v1"
    const val KEY_ENABLED = "enabled"
    const val KEY_PACKAGES = "package_names"
    const val KEY_MODE = "mode"
    const val KEY_BYPASS_PACKAGES = "bypass_package_names"

    // Fingerprint of the legacy keys written together with KEY_MODE. Older
    // versions update only the legacy keys; a mismatch means their choice is
    // newer, so a stale stored mode such as EXCLUDE must not come back.
    const val KEY_MODE_BASIS = "mode_basis"

    fun preferences(context: Context): AtomicPolicyStore = AndroidPolicyStore.perApp(context)

    fun load(
        context: Context,
        selfPackage: String = context.packageName,
    ): PerAppProxySettings = decode(preferences(context).snapshot(), selfPackage)

    internal fun decode(
        values: Map<String, Any?>,
        selfPackage: String,
    ): PerAppProxySettings {
        check(values[KEY_ENABLED] == null || values[KEY_ENABLED] is Boolean) { "Invalid per-app enabled flag" }
        check(values[KEY_PACKAGES] == null || values[KEY_PACKAGES] is Set<*>) { "Invalid per-app package set" }
        check(values[KEY_BYPASS_PACKAGES] == null || values[KEY_BYPASS_PACKAGES] is Set<*>) {
            "Invalid per-app bypass package set"
        }
        check(values[KEY_MODE] == null || values[KEY_MODE] is String) { "Invalid per-app mode" }
        check(values[KEY_MODE_BASIS] == null || values[KEY_MODE_BASIS] is String) { "Invalid per-app mode basis" }
        val enabled = values[KEY_ENABLED] as? Boolean ?: false
        val packages = stringSet(values[KEY_PACKAGES])
        val bypassPackages = stringSet(values[KEY_BYPASS_PACKAGES])
        val storedMode =
            (values[KEY_MODE] as? String)?.let { wire ->
                checkNotNull(PerAppMode.parse(wire)) { "Unsupported per-app mode" }
            }
        val mode =
            when {
                storedMode != null && values[KEY_MODE_BASIS] == modeBasis(enabled, packages) -> storedMode
                enabled -> PerAppMode.INCLUDE
                else -> PerAppMode.OFF
            }
        return PerAppProxyRules.sanitize(
            PerAppProxySettings(mode = mode, packageNames = packages, bypassPackageNames = bypassPackages),
            selfPackage,
        )
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
        val editor = preferences(context).edit()
        encode(sanitized).forEach { (key, value) ->
            when (value) {
                is Boolean -> editor.putBoolean(key, value)
                is String -> editor.putString(key, value)
                is Set<*> -> editor.putStringSet(key, value.map { it as String }.toSet())
                else -> error("Unsupported per-app policy value")
            }
        }
        check(editor.commit()) {
            "Android could not persist per-app proxy settings"
        }
        return sanitized
    }

    /** The complete record [save] writes; [decode] reads it back unchanged. */
    internal fun encode(settings: PerAppProxySettings): Map<String, Any> {
        val enabled = settings.mode == PerAppMode.INCLUDE
        return mapOf(
            KEY_ENABLED to enabled,
            KEY_PACKAGES to settings.packageNames.toSet(),
            KEY_BYPASS_PACKAGES to settings.bypassPackageNames.toSet(),
            KEY_MODE to settings.mode.wire,
            KEY_MODE_BASIS to modeBasis(enabled, settings.packageNames),
        )
    }

    @SuppressLint("ApplySharedPref", "UseKtx")
    fun clear(context: Context) {
        check(preferences(context).edit().clear().commit()) {
            "Android per-app proxy settings could not be cleared"
        }
    }

    internal fun modeBasis(
        enabled: Boolean,
        packages: Collection<String>,
    ): String {
        val digest = MessageDigest.getInstance("SHA-256")
        digest.update("v1\n$enabled\n".toByteArray(Charsets.UTF_8))
        packages.sorted().forEach { name -> digest.update("$name\n".toByteArray(Charsets.UTF_8)) }
        return digest.digest().joinToString("") { byte -> "%02x".format(byte) }
    }

    private fun stringSet(value: Any?): List<String> =
        (value as? Set<*>)?.map {
            require(it is String)
            it
        } ?: emptyList()
}
