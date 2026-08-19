package io.github.georgexie2333.usque

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class PerAppProxyTest {
    private val self = "io.github.georgexie2333.usque"

    @Test
    fun sanitizeDropsSelfPackageInvalidNamesAndDuplicates() {
        val sanitized =
            PerAppProxyRules.sanitize(
                PerAppProxySettings(
                    enabled = false,
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
        assertEquals(false, sanitized.enabled)
    }

    @Test
    fun enabledEmptyListIsRejected() {
        assertEquals(
            ANDROID_PER_APP_EMPTY,
            PerAppProxyRules.validationError(
                PerAppProxySettings(enabled = true, packageNames = emptyList()),
                self,
            ),
        )
        assertEquals(
            ANDROID_PER_APP_EMPTY,
            PerAppProxyRules.validationError(
                PerAppProxySettings(enabled = true, packageNames = listOf(self)),
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
                    enabled = false,
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
                PerAppProxySettings(enabled = false, packageNames = names),
                self,
            ),
        )
    }

    @Test
    fun disabledKeepsSanitizedSelection() {
        val error =
            PerAppProxyRules.validationError(
                PerAppProxySettings(
                    enabled = false,
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
                PerAppProxySettings(enabled = false, packageNames = listOf("com.example.app")),
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
                    enabled = true,
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
                PerAppProxySettings(enabled = true, packageNames = listOf("com.gone.app")),
                isInstalled = { false },
                selfPackage = self,
            )
        assertEquals(PerAppPlan.Empty, plan)
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
