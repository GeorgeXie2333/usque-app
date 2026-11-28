package io.github.georgexie2333.usque

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test
import org.w3c.dom.Element
import java.io.File
import javax.xml.parsers.DocumentBuilderFactory

class ControlServiceProcessTest {
    private val app = "io.github.example"

    @Test
    fun suffixMatchesTheVpnServiceProcessInTheManifest() {
        val root =
            generateSequence(File(requireNotNull(System.getProperty("user.dir"))).absoluteFile) { it.parentFile }
                .first { File(it, "apps/usque_gui/pubspec.yaml").isFile }
        val manifest =
            DocumentBuilderFactory
                .newInstance()
                .newDocumentBuilder()
                .parse(File(root, "apps/usque_gui/android/app/src/main/AndroidManifest.xml"))
        val services = manifest.getElementsByTagName("service")
        val vpn =
            (0 until services.length)
                .map { services.item(it) as Element }
                .single { it.getAttribute("android:name") == ".UsqueVpnService" }
        assertEquals(ControlServiceProcess.SUFFIX, vpn.getAttribute("android:process"))
    }

    @Test
    fun aCachedVpnProcessWithoutTheServiceIsStopped() {
        val service = UsqueVpnService::class.java.name
        // The :vpn process stays cached after the service stopped.
        assertTrue(ControlServiceProcess.serviceStopped(app, emptyList()))
        assertTrue(ControlServiceProcess.serviceStopped(app, listOf(app to "$app.QuickSettingsTileService")))
        assertTrue(ControlServiceProcess.serviceStopped(app, listOf("other.app" to service)))
        assertFalse(ControlServiceProcess.serviceStopped(app, listOf(app to service)))
        // No listing proves nothing.
        assertFalse(ControlServiceProcess.serviceStopped(app, null))
        assertFalse(ControlServiceProcess.serviceStopped("", emptyList()))
    }

    @Test
    fun onlyATrustworthyListWithoutTheServiceProcessProvesAbsence() {
        assertTrue(ControlServiceProcess.knownAbsent(app, listOf(app)))
        assertTrue(ControlServiceProcess.knownAbsent(app, listOf(app, "other.app:vpn", null)))
        // The shared tile can keep the process alive without the VPN service.
        assertFalse(ControlServiceProcess.knownAbsent(app, listOf(app, "$app:vpn")))
        assertFalse(ControlServiceProcess.knownAbsent(app, null))
        assertFalse(ControlServiceProcess.knownAbsent(app, emptyList()))
        assertFalse(ControlServiceProcess.knownAbsent(app, listOf("other.app")))
        assertFalse(ControlServiceProcess.knownAbsent("", listOf("", ":vpn")))
    }
}
