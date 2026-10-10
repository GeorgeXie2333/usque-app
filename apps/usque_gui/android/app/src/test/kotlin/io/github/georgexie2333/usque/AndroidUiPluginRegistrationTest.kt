package io.github.georgexie2333.usque

import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test
import java.io.File

class AndroidUiPluginRegistrationTest {
    @Test
    fun everyGeneratedAndroidPluginHasARegistrationFactory() {
        val root =
            generateSequence(File(requireNotNull(System.getProperty("user.dir"))).absoluteFile) { it.parentFile }
                .firstOrNull { File(it, "apps/usque_gui/pubspec.yaml").isFile }
                ?: error("Run this source contract test from the Usque checkout")
        val generated =
            File(
                root,
                "apps/usque_gui/android/app/src/main/java/io/flutter/plugins/GeneratedPluginRegistrant.java",
            )
        assertTrue("Run locked Flutter pub get before Android tests", generated.isFile)
        val pluginNames =
            Regex("flutterEngine\\.getPlugins\\(\\)\\.add\\(new ([A-Za-z0-9_$.]+)\\(\\)\\);")
                .findAll(generated.readText())
                .map { it.groupValues[1] }
                .toSet()
        assertTrue("Generated Android plugin registration format changed", pluginNames.isNotEmpty())
        assertEquals(
            "Update AndroidUiPluginRegistration when Flutter's Android plugin dependencies change",
            pluginNames,
            AndroidUiPluginRegistration.generatedPluginClassNames,
        )
    }
}
