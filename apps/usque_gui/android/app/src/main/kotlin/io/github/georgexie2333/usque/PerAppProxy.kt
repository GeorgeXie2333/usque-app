package io.github.georgexie2333.usque

internal data class PerAppProxySettings(
    val enabled: Boolean = false,
    val packageNames: List<String> = emptyList(),
) {
    fun toMap(): Map<String, Any?> =
        mapOf(
            "enabled" to enabled,
            "package_names" to packageNames,
        )
}

internal sealed class PerAppPlan {
    data object None : PerAppPlan()

    data object Empty : PerAppPlan()

    data class Allow(
        val packages: List<String>,
    ) : PerAppPlan()
}

internal class PerAppProxyStoreException(
    val code: String,
) : IllegalArgumentException(code)

internal class PerAppProxyEmptyException : IllegalStateException(ANDROID_PER_APP_EMPTY)

internal const val ANDROID_PER_APP_EMPTY = "ANDROID_PER_APP_EMPTY"

internal object PerAppProxyRules {
    const val MAX_PACKAGES = 1024
    const val MAX_PACKAGE_LENGTH = 256
    val PACKAGE_NAME =
        Regex("^[A-Za-z][A-Za-z0-9_]*(?:\\.[A-Za-z][A-Za-z0-9_]*)*$")

    fun sanitize(
        settings: PerAppProxySettings,
        selfPackage: String,
    ): PerAppProxySettings =
        PerAppProxySettings(
            enabled = settings.enabled,
            packageNames = sanitizePackages(settings.packageNames, selfPackage),
        )

    fun sanitizePackages(
        names: Collection<String>,
        selfPackage: String,
    ): List<String> =
        names
            .map(String::trim)
            .filter { name ->
                name.isNotEmpty() &&
                    name != selfPackage &&
                    name.length <= MAX_PACKAGE_LENGTH &&
                    PACKAGE_NAME.matches(name)
            }.distinct()
            .sorted()
            .take(MAX_PACKAGES)

    fun validationError(
        settings: PerAppProxySettings,
        selfPackage: String,
    ): String? {
        if (settings.packageNames.size > MAX_PACKAGES) {
            return "INVALID_ARGUMENT"
        }
        val hasInvalid =
            settings.packageNames.any { raw ->
                val name = raw.trim()
                name.isNotEmpty() &&
                    name != selfPackage &&
                    (name.length > MAX_PACKAGE_LENGTH || !PACKAGE_NAME.matches(name))
            }
        if (hasInvalid) {
            return "INVALID_ARGUMENT"
        }
        val sanitized = sanitize(settings, selfPackage)
        return if (sanitized.enabled && sanitized.packageNames.isEmpty()) {
            ANDROID_PER_APP_EMPTY
        } else {
            null
        }
    }
}

internal object PerAppProxyApplier {
    fun plan(
        settings: PerAppProxySettings,
        isInstalled: (String) -> Boolean,
        selfPackage: String,
    ): PerAppPlan {
        if (!settings.enabled) {
            return PerAppPlan.None
        }
        val allowed =
            PerAppProxyRules
                .sanitizePackages(settings.packageNames, selfPackage)
                .filter(isInstalled)
        return if (allowed.isEmpty()) {
            PerAppPlan.Empty
        } else {
            PerAppPlan.Allow(allowed)
        }
    }
}
