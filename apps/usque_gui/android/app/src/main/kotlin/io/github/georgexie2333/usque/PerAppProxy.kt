package io.github.georgexie2333.usque

/**
 * INCLUDE sends only the selected apps through the VPN. EXCLUDE sends every
 * app except the selected ones. Each mode keeps its own list so that switching
 * modes never reinterprets apps chosen for the tunnel as apps to leave out.
 */
internal enum class PerAppMode(
    val wire: String,
) {
    OFF("off"),
    INCLUDE("include"),
    EXCLUDE("exclude"),
    ;

    companion object {
        fun parse(value: String): PerAppMode? = entries.firstOrNull { it.wire == value }
    }
}

internal data class PerAppProxySettings(
    val mode: PerAppMode = PerAppMode.OFF,
    val packageNames: List<String> = emptyList(),
    val bypassPackageNames: List<String> = emptyList(),
) {
    /** The packages the selected mode applies; empty when per-app routing is off. */
    val activePackages: List<String>
        get() =
            when (mode) {
                PerAppMode.OFF -> emptyList()
                PerAppMode.INCLUDE -> packageNames
                PerAppMode.EXCLUDE -> bypassPackageNames
            }

    fun toMap(): Map<String, Any?> =
        mapOf(
            // Older clients understand only this flag, which means INCLUDE.
            "enabled" to (mode == PerAppMode.INCLUDE),
            "mode" to mode.wire,
            "package_names" to packageNames,
            "bypass_package_names" to bypassPackageNames,
        )
}

internal sealed class PerAppPlan {
    data object None : PerAppPlan()

    data object Empty : PerAppPlan()

    data class Allow(
        val packages: List<String>,
    ) : PerAppPlan()

    data class Disallow(
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
            mode = settings.mode,
            packageNames = sanitizePackages(settings.packageNames, selfPackage),
            bypassPackageNames = sanitizePackages(settings.bypassPackageNames, selfPackage),
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
        for (names in listOf(settings.packageNames, settings.bypassPackageNames)) {
            if (names.size > MAX_PACKAGES) {
                return "INVALID_ARGUMENT"
            }
            val hasInvalid =
                names.any { raw ->
                    val name = raw.trim()
                    name.isNotEmpty() &&
                        name != selfPackage &&
                        (name.length > MAX_PACKAGE_LENGTH || !PACKAGE_NAME.matches(name))
                }
            if (hasInvalid) {
                return "INVALID_ARGUMENT"
            }
        }
        val sanitized = sanitize(settings, selfPackage)
        return if (sanitized.mode != PerAppMode.OFF && sanitized.activePackages.isEmpty()) {
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
    ): PerAppPlan =
        when (settings.mode) {
            PerAppMode.OFF -> {
                PerAppPlan.None
            }

            PerAppMode.INCLUDE -> {
                val allowed =
                    PerAppProxyRules
                        .sanitizePackages(settings.packageNames, selfPackage)
                        .filter(isInstalled)
                if (allowed.isEmpty()) PerAppPlan.Empty else PerAppPlan.Allow(allowed)
            }

            PerAppMode.EXCLUDE -> {
                // Leaving nothing to exclude keeps every app in the tunnel,
                // which is the protected direction, so it is not an error.
                val excluded =
                    PerAppProxyRules
                        .sanitizePackages(settings.bypassPackageNames, selfPackage)
                        .filter(isInstalled)
                if (excluded.isEmpty()) PerAppPlan.None else PerAppPlan.Disallow(excluded)
            }
        }
}
