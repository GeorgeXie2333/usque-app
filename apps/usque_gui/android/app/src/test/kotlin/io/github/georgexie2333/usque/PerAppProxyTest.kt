package io.github.georgexie2333.usque

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertThrows
import org.junit.Assert.assertTrue
import org.junit.Test

class PerAppProxyTest {
    private val self = "io.github.georgexie2333.usque"

    @Test
    fun sanitizeDropsSelfPackageInvalidNamesAndDuplicates() {
        val sanitized =
            PerAppProxyRules.sanitize(
                PerAppProxySettings(
                    mode = PerAppMode.OFF,
                    packageNames =
                        listOf(
                            self,
                            "com.example.app",
                            " com.example.app ",
                            "bad name",
                            "",
                            "com.other.app",
                        ),
                ),
                self,
            )
        assertEquals(listOf("com.example.app", "com.other.app"), sanitized.packageNames)
        assertEquals(PerAppMode.OFF, sanitized.mode)
    }

    @Test
    fun enabledEmptyListIsRejected() {
        assertEquals(
            ANDROID_PER_APP_EMPTY,
            PerAppProxyRules.validationError(
                PerAppProxySettings(mode = PerAppMode.INCLUDE, packageNames = emptyList()),
                self,
            ),
        )
        assertEquals(
            ANDROID_PER_APP_EMPTY,
            PerAppProxyRules.validationError(
                PerAppProxySettings(mode = PerAppMode.INCLUDE, packageNames = listOf(self)),
                self,
            ),
        )
    }

    @Test
    fun illegalPackageNameIsRejected() {
        assertEquals(
            "INVALID_ARGUMENT",
            PerAppProxyRules.validationError(
                PerAppProxySettings(
                    mode = PerAppMode.OFF,
                    packageNames = listOf("com.example.app", "!!!"),
                ),
                self,
            ),
        )
    }

    @Test
    fun tooManyPackagesAreRejected() {
        val names = (1..PerAppProxyRules.MAX_PACKAGES + 1).map { "com.example.app$it" }
        assertEquals(
            "INVALID_ARGUMENT",
            PerAppProxyRules.validationError(
                PerAppProxySettings(mode = PerAppMode.OFF, packageNames = names),
                self,
            ),
        )
    }

    @Test
    fun disabledKeepsSanitizedSelection() {
        val error =
            PerAppProxyRules.validationError(
                PerAppProxySettings(
                    mode = PerAppMode.OFF,
                    packageNames = listOf("com.example.one"),
                ),
                self,
            )
        assertNull(error)
    }

    @Test
    fun applierDisabledIsNone() {
        val plan =
            PerAppProxyApplier.plan(
                PerAppProxySettings(mode = PerAppMode.OFF, packageNames = listOf("com.example.app")),
                isInstalled = { true },
                selfPackage = self,
            )
        assertEquals(PerAppPlan.None, plan)
    }

    @Test
    fun applierEnabledFiltersMissingAndSelf() {
        val plan =
            PerAppProxyApplier.plan(
                PerAppProxySettings(
                    mode = PerAppMode.INCLUDE,
                    packageNames = listOf(self, "com.gone.app", "com.keep.app"),
                ),
                isInstalled = { it == "com.keep.app" },
                selfPackage = self,
            )
        assertEquals(PerAppPlan.Allow(listOf("com.keep.app")), plan)
    }

    @Test
    fun applierEnabledWithNoInstalledPackagesIsEmpty() {
        val plan =
            PerAppProxyApplier.plan(
                PerAppProxySettings(mode = PerAppMode.INCLUDE, packageNames = listOf("com.gone.app")),
                isInstalled = { false },
                selfPackage = self,
            )
        assertEquals(PerAppPlan.Empty, plan)
    }

    @Test
    fun excludeModeNeedsItsOwnNonEmptyList() {
        // The include list does not count: each mode keeps a separate list.
        assertEquals(
            ANDROID_PER_APP_EMPTY,
            PerAppProxyRules.validationError(
                PerAppProxySettings(
                    mode = PerAppMode.EXCLUDE,
                    packageNames = listOf("com.example.app"),
                    bypassPackageNames = listOf(self),
                ),
                self,
            ),
        )
        assertNull(
            PerAppProxyRules.validationError(
                PerAppProxySettings(
                    mode = PerAppMode.EXCLUDE,
                    bypassPackageNames = listOf("com.example.game"),
                ),
                self,
            ),
        )
    }

    @Test
    fun invalidOrOversizedBypassListIsRejected() {
        assertEquals(
            "INVALID_ARGUMENT",
            PerAppProxyRules.validationError(
                PerAppProxySettings(mode = PerAppMode.OFF, bypassPackageNames = listOf("!!!")),
                self,
            ),
        )
        val names = (1..PerAppProxyRules.MAX_PACKAGES + 1).map { "com.example.app$it" }
        assertEquals(
            "INVALID_ARGUMENT",
            PerAppProxyRules.validationError(
                PerAppProxySettings(mode = PerAppMode.OFF, bypassPackageNames = names),
                self,
            ),
        )
    }

    @Test
    fun sanitizeKeepsBothListsIndependent() {
        val sanitized =
            PerAppProxyRules.sanitize(
                PerAppProxySettings(
                    mode = PerAppMode.EXCLUDE,
                    packageNames = listOf("com.bank.app"),
                    bypassPackageNames = listOf(self, "com.game.app", "com.game.app"),
                ),
                self,
            )
        assertEquals(listOf("com.bank.app"), sanitized.packageNames)
        assertEquals(listOf("com.game.app"), sanitized.bypassPackageNames)
        assertEquals(listOf("com.game.app"), sanitized.activePackages)
    }

    @Test
    fun applierExcludeFiltersMissingAndSelf() {
        val plan =
            PerAppProxyApplier.plan(
                PerAppProxySettings(
                    mode = PerAppMode.EXCLUDE,
                    packageNames = listOf("com.bank.app"),
                    bypassPackageNames = listOf(self, "com.gone.app", "com.game.app"),
                ),
                isInstalled = { it != "com.gone.app" },
                selfPackage = self,
            )
        assertEquals(PerAppPlan.Disallow(listOf("com.game.app")), plan)
    }

    @Test
    fun applierExcludeWithNothingInstalledKeepsEveryAppInTheTunnel() {
        val plan =
            PerAppProxyApplier.plan(
                PerAppProxySettings(mode = PerAppMode.EXCLUDE, bypassPackageNames = listOf("com.gone.app")),
                isInstalled = { false },
                selfPackage = self,
            )
        assertEquals(PerAppPlan.None, plan)
    }

    @Test
    fun applierOffIgnoresBothLists() {
        val plan =
            PerAppProxyApplier.plan(
                PerAppProxySettings(
                    mode = PerAppMode.OFF,
                    packageNames = listOf("com.bank.app"),
                    bypassPackageNames = listOf("com.game.app"),
                ),
                isInstalled = { true },
                selfPackage = self,
            )
        assertEquals(PerAppPlan.None, plan)
    }

    @Test
    fun wireMapKeepsTheLegacyFlagMeaningInclude() {
        for (mode in PerAppMode.entries) {
            val map = PerAppProxySettings(mode = mode).toMap()
            assertEquals(mode == PerAppMode.INCLUDE, map["enabled"])
            assertEquals(mode, PerAppMode.parse(map["mode"] as String))
        }
        assertNull(PerAppMode.parse("everything"))
    }

    @Test
    fun storedRecordRoundTripsEveryMode() {
        for (mode in PerAppMode.entries) {
            val settings =
                PerAppProxySettings(
                    mode = mode,
                    packageNames = listOf("com.bank.app"),
                    bypassPackageNames = listOf("com.game.app"),
                )
            assertEquals(settings, PerAppProxyStore.decode(PerAppProxyStore.encode(settings), self))
        }
    }

    @Test
    fun legacyRecordWithoutModeReadsAsIncludeOrOff() {
        val packages = setOf("com.bank.app")
        assertEquals(
            PerAppMode.INCLUDE,
            PerAppProxyStore.decode(mapOf("enabled" to true, "package_names" to packages), self).mode,
        )
        assertEquals(
            PerAppMode.OFF,
            PerAppProxyStore.decode(mapOf("enabled" to false, "package_names" to packages), self).mode,
        )
        assertEquals(PerAppMode.OFF, PerAppProxyStore.decode(emptyMap(), self).mode)
    }

    @Test
    fun excludeRecordReadsAsOffInOlderVersions() {
        // An older version reads only the legacy flag, which EXCLUDE keeps false.
        val stored =
            PerAppProxyStore.encode(
                PerAppProxySettings(mode = PerAppMode.EXCLUDE, bypassPackageNames = listOf("com.game.app")),
            )
        assertEquals(false, stored["enabled"])
    }

    @Test
    fun olderVersionWriteOverridesAStaleStoredMode() {
        val excluded =
            PerAppProxyStore.encode(
                PerAppProxySettings(
                    mode = PerAppMode.EXCLUDE,
                    packageNames = listOf("com.bank.app"),
                    bypassPackageNames = listOf("com.game.app"),
                ),
            )
        // An older version re-enabled its include list.
        val reEnabled = excluded + mapOf("enabled" to true)
        assertEquals(PerAppMode.INCLUDE, PerAppProxyStore.decode(reEnabled, self).mode)
        // An older version changed the include list while leaving it off.
        val edited = excluded + mapOf("package_names" to setOf("com.bank.app", "com.mail.app"))
        assertEquals(PerAppMode.OFF, PerAppProxyStore.decode(edited, self).mode)
        // An untouched record keeps its exclusion.
        assertEquals(PerAppMode.EXCLUDE, PerAppProxyStore.decode(excluded, self).mode)
    }

    @Test
    fun unknownOrMalformedModeFailsClosed() {
        for (values in listOf(
            mapOf<String, Any?>("mode" to "everything"),
            mapOf<String, Any?>("mode" to true),
            mapOf<String, Any?>("bypass_package_names" to "com.game.app"),
        )) {
            assertThrows(IllegalStateException::class.java) {
                PerAppProxyStore.decode(values, self)
            }
        }
    }

    @Test
    fun catalogExcludesSelfPackage() {
        val apps =
            listOf(
                InstalledAppInfo(self, "Usque", isSystem = false, hasInternet = true),
                InstalledAppInfo("com.keep.app", "Keep", isSystem = false, hasInternet = true),
            )
        assertEquals(
            listOf("com.keep.app"),
            InstalledAppCatalog.excludeSelf(apps, self).map { it.packageName },
        )
    }
}
