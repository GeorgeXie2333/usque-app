package io.github.georgexie2333.usque

import android.content.ServiceConnection
import io.flutter.plugin.common.MethodCall
import io.flutter.plugin.common.MethodChannel
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertSame
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Test
import java.io.IOException
import java.util.concurrent.CountDownLatch
import java.util.concurrent.Executor
import java.util.concurrent.Executors
import java.util.concurrent.TimeUnit

class AndroidEngineMethodHandlerTest {
    private lateinit var scheduler: ImmediateScheduler
    private lateinit var controlClient: VpnControlClient
    private lateinit var endpoint: RecordingEndpoint
    private lateinit var activityCommands: RecordingActivityCommands
    private lateinit var engineBridge: FakeEngineBridge
    private lateinit var maintenance: RecordingMaintenance
    private lateinit var identityStore: RecordingIdentityStore
    private lateinit var handler: AndroidEngineMethodHandler

    @Before
    fun setUp() {
        scheduler = ImmediateScheduler()
        endpoint = RecordingEndpoint()
        controlClient =
            VpnControlClient(
                scheduler = scheduler,
                serviceBinder = { _: ServiceConnection -> true },
                serviceUnbinder = { },
                endpointFromBinder = { _, _ -> error("unused") },
            )
        controlClient.attachEndpointForTest(endpoint)
        activityCommands = RecordingActivityCommands()
        engineBridge = FakeEngineBridge()
        maintenance = RecordingMaintenance()
        identityStore = RecordingIdentityStore()
        handler =
            AndroidEngineMethodHandler(
                profileConfigPath = "/tmp/profiles-v2.json",
                identityStore = identityStore,
                identityExecutor = Executor { it.run() },
                mainScheduler = scheduler,
                controlClient = controlClient,
                activityCommands = activityCommands,
                engineBridge = engineBridge,
                maintenanceBridge = maintenance,
                warpSecretOkCode = 0,
            )
        controlClient.clearAllAcknowledgedListener =
            VpnControlClient.ClearAllAcknowledgedListener { result ->
                handler.finishClearAllData(result)
            }
    }

    @Test
    fun dispatchesSnapshotToControlClient() {
        val result = RecordingResult()
        handler.handle(MethodCall("snapshot", null), result)
        assertEquals(listOf(UsqueVpnService.MSG_SNAPSHOT), endpoint.whats)
        assertNull(result.errorCode)
    }

    @Test
    fun dispatchesDisconnectAndCancelsVpnPermission() {
        val result = RecordingResult()
        handler.handle(MethodCall("disconnect", null), result)
        assertEquals(1, activityCommands.cancelCount)
        assertEquals("VPN_PERMISSION_CANCELLED", activityCommands.lastCancelCode)
        assertEquals(listOf(UsqueVpnService.MSG_DISCONNECT), endpoint.whats)
    }

    @Test
    fun dispatchesRetryToControlClient() {
        val result = RecordingResult()
        handler.handle(MethodCall("retry", null), result)
        assertEquals(listOf(UsqueVpnService.MSG_RETRY), endpoint.whats)
        assertNull(result.errorCode)
    }

    @Test
    fun setPerAppProxyPersistsAndNotifiesVpnService() {
        val result = RecordingResult()
        handler.handle(
            MethodCall(
                "setPerAppProxy",
                mapOf(
                    "enabled" to true,
                    "package_names" to listOf("com.example.app"),
                ),
            ),
            result,
        )
        assertEquals(listOf(UsqueVpnService.MSG_APPLY_PER_APP), endpoint.whats)
        val saved = result.successValue as Map<*, *>
        assertEquals(true, saved["enabled"])
        assertEquals(listOf("com.example.app"), saved["package_names"])
        assertNull(result.errorCode)
    }

    @Test
    fun setPerAppProxyRejectsMalformedPackageList() {
        val result = RecordingResult()
        handler.handle(
            MethodCall(
                "setPerAppProxy",
                mapOf(
                    "enabled" to true,
                    "package_names" to listOf(1),
                ),
            ),
            result,
        )
        assertEquals("INVALID_ARGUMENT", result.errorCode)
        assertTrue(endpoint.whats.isEmpty())
    }

    @Test
    fun reconfigureActiveProfilePersistsAndNotifiesTheVpnServiceWithoutConnect() {
        engineBridge.profileCatalogJson =
            """{"profiles":[{"id":"p1"}]}"""
        val result = RecordingResult()
        val profile =
            mapOf(
                "id" to "p1",
                "mode" to "vpn",
                "frontends" to mapOf("tunnel" to true, "socks5" to true, "http" to true),
            )
        handler.handle(MethodCall("reconfigureActiveProfile", profile), result)
        assertEquals(1, engineBridge.commands.size)
        assertTrue(engineBridge.commands.single().contains("reconfigure_active_profile"))
        assertEquals(listOf(UsqueVpnService.MSG_RECONFIGURE), endpoint.whats)
        val extrasJson = endpoint.lastExtras?.get(UsqueVpnService.EXTRA_PROFILE_JSON) as String
        assertTrue(extrasJson.contains("\"id\":\"p1\""))
        assertEquals(0, activityCommands.connectCount)
        assertNull(result.errorCode)
    }

    @Test
    fun reconfigureActiveProfileSendsTunnelOffWithLegacyVpnMode() {
        engineBridge.profileCatalogJson =
            """{"profiles":[{"id":"p1"}]}"""
        val result = RecordingResult()
        handler.handle(
            MethodCall(
                "reconfigureActiveProfile",
                mapOf(
                    "id" to "p1",
                    "mode" to "vpn",
                    "frontends" to mapOf("tunnel" to false, "socks5" to true, "http" to true),
                ),
            ),
            result,
        )
        assertEquals(listOf(UsqueVpnService.MSG_RECONFIGURE), endpoint.whats)
        val extras = endpoint.lastExtras ?: error("MSG_RECONFIGURE extras missing")
        val extrasJson = extras[UsqueVpnService.EXTRA_PROFILE_JSON] as String
        assertTrue(extrasJson.contains("\"mode\":\"socks5\""))
        assertTrue(extrasJson.contains("\"tunnel\":false"))
        assertTrue(VpnReconfigure.shouldTearDownTun(endpoint.whats.single(), extrasJson))
        assertNull(result.errorCode)
    }

    @Test
    fun clearAllDataRequiresConfirmation() {
        val result = RecordingResult()
        handler.handle(MethodCall("clearAllData", mapOf("confirmed" to false)), result)
        assertEquals("CONFIRMATION_REQUIRED", result.errorCode)
        assertTrue(endpoint.whats.isEmpty())
    }

    @Test
    fun clearAllDataDispatchesWhenConfirmed() {
        val result = RecordingResult()
        handler.handle(MethodCall("clearAllData", mapOf("confirmed" to true)), result)
        assertEquals(1, activityCommands.cancelCount)
        assertEquals(listOf(UsqueVpnService.MSG_CLEAR_ALL_DATA), endpoint.whats)
        assertEquals(true, endpoint.lastExtras?.get("confirmed"))
    }

    @Test
    fun connectValidatesEngineReadyAndMode() {
        engineBridge.ready = false
        val unavailable = RecordingResult()
        handler.handle(
            MethodCall("connect", mapOf("mode" to "vpn", "id" to "p1")),
            unavailable,
        )
        assertEquals("ENGINE_UNAVAILABLE", unavailable.errorCode)

        engineBridge.ready = true
        val badMode = RecordingResult()
        handler.handle(MethodCall("connect", mapOf("mode" to "wireguard")), badMode)
        assertEquals("INVALID_PROFILE", badMode.errorCode)

        val ok = RecordingResult()
        handler.handle(
            MethodCall("connect", mapOf("mode" to "socks5", "id" to "p1")),
            ok,
        )
        assertEquals(1, activityCommands.connectCount)
        assertEquals("socks5", activityCommands.lastMode)
        assertTrue(activityCommands.lastProfileJson!!.contains("socks5"))
    }

    @Test
    fun connectOmitsNullMapEntriesInProfileJson() {
        engineBridge.ready = true
        val ok = RecordingResult()
        handler.handle(
            MethodCall(
                "connect",
                mapOf(
                    "mode" to "httpProxy",
                    "frontends" to
                        mapOf(
                            "tunnel" to false,
                            "socks5" to false,
                            "http" to true,
                        ),
                    "id" to "p1",
                    "optional" to null,
                ),
            ),
            ok,
        )
        val json = activityCommands.lastProfileJson!!
        assertTrue(json.contains("\"mode\":\"socks5\""))
        assertFalse(json.contains("\"optional\""))
        assertFalse(json.contains(":null"))
    }

    @Test
    fun unknownMethodIsNotImplemented() {
        val result = RecordingResult()
        handler.handle(MethodCall("noSuchMethod", null), result)
        assertTrue(result.notImplementedCalled)
        assertEquals(1, result.completionCount)
    }

    @Test
    fun exportDiagnosticsDelegatesToActivity() {
        val result = RecordingResult()
        handler.handle(MethodCall("exportDiagnostics", null), result)
        assertEquals(1, activityCommands.diagnosticsCount)
    }

    @Test
    fun provisionIdentityRequiresTerms() {
        val result = RecordingResult()
        handler.handle(MethodCall("provisionIdentity", mapOf("terms_accepted" to false)), result)
        assertEquals("TERMS_NOT_ACCEPTED", result.errorCode)
    }

    @Test
    fun zeroTrustRepairRestoresTheOldSecretWhenMetadataStorageFails() {
        val oldSecret = "old-secret".toByteArray()
        identityStore.put(
            "p1",
            SecureIdentityStore.Record.WARP_SECRET,
            oldSecret,
        )
        identityStore.put(
            "p1",
            SecureIdentityStore.Record.IDENTITY_METADATA,
            """{"version":1,"provider":"zero_trust","organization":"example-team"}"""
                .toByteArray(),
        )
        identityStore.failOnPut = SecureIdentityStore.Record.IDENTITY_METADATA

        val result = RecordingResult()
        handler.handle(
            MethodCall(
                "provisionIdentity",
                mapOf(
                    "profile_id" to "p1",
                    "method" to "zeroTrust",
                    "team_name" to "example-team",
                    "callback_uri" to
                        "com.cloudflare.warp://example-team.cloudflareaccess.com/auth?token=test",
                    "terms_accepted" to true,
                ),
            ),
            result,
        )

        assertEquals("ZERO_TRUST_LOCAL_COMMIT_FAILED", result.errorCode)
        assertTrue(
            identityStore
                .get("p1", SecureIdentityStore.Record.WARP_SECRET)!!
                .contentEquals(oldSecret),
        )
    }

    @Test
    fun importLegacyProfilesRejectsMalformedCatalog() {
        val result = RecordingResult()
        handler.handle(MethodCall("importLegacyProfiles", mapOf("profiles" to emptyList<Any>())), result)
        assertEquals("INVALID_ARGUMENT", result.errorCode)
        assertTrue(engineBridge.commands.isEmpty())
    }

    @Test
    fun importLegacyProfilesRequiresLinkedEngine() {
        engineBridge.linked = false
        val result = RecordingResult()
        handler.handle(
            MethodCall(
                "importLegacyProfiles",
                mapOf(
                    "profiles" to listOf(mapOf("id" to "p1")),
                    "active_profile_id" to "p1",
                ),
            ),
            result,
        )
        assertEquals("ENGINE_UNAVAILABLE", result.errorCode)
        assertTrue(engineBridge.commands.isEmpty())
    }

    @Test
    fun upsertProfileRejectsMalformedAndRequiresLinkedEngine() {
        val bad = RecordingResult()
        handler.handle(MethodCall("upsertProfile", "not-a-map"), bad)
        assertEquals("INVALID_ARGUMENT", bad.errorCode)

        engineBridge.linked = false
        val unavailable = RecordingResult()
        handler.handle(MethodCall("upsertProfile", mapOf("id" to "p1", "name" to "Home")), unavailable)
        assertEquals("ENGINE_UNAVAILABLE", unavailable.errorCode)
        assertTrue(engineBridge.commands.isEmpty())
    }

    @Test
    fun updateProxyAuthStoresPasswordInTheVaultAndRejectsUsernameWithoutPassword() {
        val missing = RecordingResult()
        handler.handle(
            MethodCall(
                "updateProxyAuth",
                mapOf(
                    "profile_id" to "p1",
                    "username" to "lan-user",
                    "password" to "",
                    "confirmed" to true,
                ),
            ),
            missing,
        )
        assertEquals("CONFIGURATION_INVALID", missing.errorCode)
        assertNull(identityStore.get("p1", SecureIdentityStore.Record.PROXY_PASSWORD))

        val saved = RecordingResult()
        handler.handle(
            MethodCall(
                "updateProxyAuth",
                mapOf(
                    "profile_id" to "p1",
                    "username" to "lan-user",
                    "password" to "s3cret",
                    "confirmed" to true,
                ),
            ),
            saved,
        )
        assertNull(saved.errorCode)
        assertEquals(
            "s3cret",
            identityStore
                .get("p1", SecureIdentityStore.Record.PROXY_PASSWORD)!!
                .toString(Charsets.UTF_8),
        )

        val cleared = RecordingResult()
        handler.handle(
            MethodCall(
                "updateProxyAuth",
                mapOf(
                    "profile_id" to "p1",
                    "username" to "",
                    "password" to "",
                    "confirmed" to true,
                ),
            ),
            cleared,
        )
        assertNull(cleared.errorCode)
        assertNull(identityStore.get("p1", SecureIdentityStore.Record.PROXY_PASSWORD))
    }

    @Test
    fun deleteProfileRequiresIdAndLinkedEngine() {
        val bad = RecordingResult()
        handler.handle(MethodCall("deleteProfile", emptyMap<String, Any>()), bad)
        assertEquals("INVALID_ARGUMENT", bad.errorCode)

        engineBridge.linked = false
        val unavailable = RecordingResult()
        handler.handle(MethodCall("deleteProfile", mapOf("profile_id" to "p1")), unavailable)
        assertEquals("ENGINE_UNAVAILABLE", unavailable.errorCode)
        assertTrue(engineBridge.commands.isEmpty())
    }

    @Test
    fun setActiveProfileRequiresIdAndLinkedEngine() {
        val bad = RecordingResult()
        handler.handle(MethodCall("setActiveProfile", emptyMap<String, Any>()), bad)
        assertEquals("INVALID_ARGUMENT", bad.errorCode)

        engineBridge.linked = false
        val unavailable = RecordingResult()
        handler.handle(MethodCall("setActiveProfile", mapOf("profile_id" to "p2")), unavailable)
        assertEquals("ENGINE_UNAVAILABLE", unavailable.errorCode)
        assertTrue(engineBridge.commands.isEmpty())
    }

    @Test
    fun checkForUpdatesRequiresLinkedEngineAndDispatches() {
        engineBridge.linked = false
        val unavailable = RecordingResult()
        handler.handle(MethodCall("checkForUpdates", mapOf("manual" to true)), unavailable)
        assertEquals("ENGINE_UNAVAILABLE", unavailable.errorCode)
        assertEquals(0, maintenance.checkCount)

        engineBridge.linked = true
        val ok = RecordingResult()
        handler.handle(MethodCall("checkForUpdates", mapOf("manual" to false)), ok)
        assertEquals(1, maintenance.checkCount)
        assertEquals(false, maintenance.lastManual)
        assertEquals(1, ok.completionCount)
        assertNull(ok.errorCode)
    }

    @Test
    fun createProfileWithIdentityRejectsMalformedRequest() {
        val missingTerms = RecordingResult()
        handler.handle(
            MethodCall(
                "createProfileWithIdentity",
                mapOf(
                    "profile" to mapOf("id" to "p1"),
                    "method" to "register",
                    "terms_accepted" to false,
                ),
            ),
            missingTerms,
        )
        assertEquals("INVALID_ARGUMENT", missingTerms.errorCode)

        val missingProfile = RecordingResult()
        handler.handle(
            MethodCall(
                "createProfileWithIdentity",
                mapOf(
                    "method" to "register",
                    "terms_accepted" to true,
                ),
            ),
            missingProfile,
        )
        assertEquals("INVALID_ARGUMENT", missingProfile.errorCode)
        assertTrue(engineBridge.commands.isEmpty())
    }

    @Test
    fun createProfileWithIdentityRequiresLinkedEngine() {
        engineBridge.linked = false
        val result = RecordingResult()
        handler.handle(
            MethodCall(
                "createProfileWithIdentity",
                mapOf(
                    "profile" to mapOf("id" to "p1", "name" to "Work"),
                    "method" to "register",
                    "terms_accepted" to true,
                ),
            ),
            result,
        )
        assertEquals("ENGINE_UNAVAILABLE", result.errorCode)
        assertTrue(engineBridge.commands.isEmpty())
    }

    @Test
    fun zeroTrustCreationDeletesPartialIdentityWhenMetadataStorageFails() {
        identityStore.failOnPut = SecureIdentityStore.Record.IDENTITY_METADATA
        val result = RecordingResult()
        handler.handle(
            MethodCall(
                "createProfileWithIdentity",
                mapOf(
                    "profile" to mapOf("id" to "p-new", "name" to "Work"),
                    "method" to "zeroTrust",
                    "team_name" to "example-team",
                    "callback_uri" to
                        "com.cloudflare.warp://example-team.cloudflareaccess.com/auth?token=test",
                    "terms_accepted" to true,
                ),
            ),
            result,
        )

        assertEquals("ZERO_TRUST_LOCAL_COMMIT_FAILED", result.errorCode)
        assertEquals(1, identityStore.deleteIdentityCount)
        assertNull(identityStore.get("p-new", SecureIdentityStore.Record.WARP_SECRET))
        assertTrue(engineBridge.commands.any { it.contains("complete_identity_creations") })
    }

    @Test
    fun zeroTrustRegistrationFailureReportsOnlySafeHttpStageAndStatus() {
        engineBridge.zeroTrustFailure =
            IOException("USQUE_ZT_HTTP:device_registration:422")
        val result = RecordingResult()
        handler.handle(
            MethodCall(
                "createProfileWithIdentity",
                mapOf(
                    "profile" to mapOf("id" to "p-new", "name" to "Work"),
                    "method" to "zeroTrust",
                    "team_name" to "example-team",
                    "callback_uri" to
                        "com.cloudflare.warp://example-team.cloudflareaccess.com/auth?token=secret",
                    "terms_accepted" to true,
                ),
            ),
            result,
        )

        assertEquals("ZERO_TRUST_HTTP_DEVICE_REGISTRATION_422", result.errorCode)
        assertTrue(result.errorMessage!!.contains("HTTP 422"))
        assertFalse(result.errorMessage!!.contains("secret"))
        assertFalse(result.errorDetails.toString().contains("secret"))
    }

    @Test
    fun consumerLicenseNetworkFailureIsNotReportedAsInvalidLicenseOrZeroTrust() {
        engineBridge.consumerLicenseFailure = IOException("USQUE_CONSUMER_NETWORK")
        val network = RecordingResult()
        handler.handle(
            MethodCall(
                "provisionIdentity",
                mapOf(
                    "profile_id" to "p1",
                    "method" to "registerWithLicense",
                    "license_key" to "AAAA-BBBB-CCCC",
                    "terms_accepted" to true,
                ),
            ),
            network,
        )
        assertEquals("REGISTRATION_FAILED", network.errorCode)
        assertFalse(network.errorMessage.orEmpty().contains("Zero Trust"))
        assertFalse(network.errorMessage.orEmpty().contains("residual device"))

        engineBridge.consumerLicenseFailure =
            IOException("USQUE_CONSUMER_INVALID_LICENSE_KEY")
        val invalidLicense = RecordingResult()
        handler.handle(
            MethodCall(
                "provisionIdentity",
                mapOf(
                    "profile_id" to "p1",
                    "method" to "registerWithLicense",
                    "license_key" to "AAAA-BBBB-CCCC",
                    "terms_accepted" to true,
                ),
            ),
            invalidLicense,
        )
        assertEquals("INVALID_LICENSE_KEY", invalidLicense.errorCode)
    }

    @Test
    fun zeroTrustEnrollmentNetworkFailureWarnsAboutResidualDevice() {
        engineBridge.zeroTrustFailure =
            IOException("USQUE_ZT_NETWORK:masque_enrollment")
        val result = RecordingResult()
        handler.handle(
            MethodCall(
                "createProfileWithIdentity",
                mapOf(
                    "profile" to mapOf("id" to "p-new", "name" to "Work"),
                    "method" to "zeroTrust",
                    "team_name" to "example-team",
                    "callback_uri" to
                        "com.cloudflare.warp://example-team.cloudflareaccess.com/auth?token=secret",
                    "terms_accepted" to true,
                ),
            ),
            result,
        )

        assertEquals("ZERO_TRUST_NETWORK_MASQUE_ENROLLMENT", result.errorCode)
        assertTrue(result.errorMessage!!.contains("residual device"))
        assertFalse(result.errorMessage!!.contains("secret"))
    }

    @Test
    fun zeroTrustSafeDiagnosticReasonNeverIncludesTheCallback() {
        engineBridge.zeroTrustFailure = IOException("USQUE_ZT_DIAGNOSTIC:invalid_device_id")
        val result = RecordingResult()
        handler.handle(
            MethodCall(
                "createProfileWithIdentity",
                mapOf(
                    "profile" to mapOf("id" to "p-new", "name" to "Work"),
                    "method" to "zeroTrust",
                    "team_name" to "example-team",
                    "callback_uri" to
                        "com.cloudflare.warp://example-team.cloudflareaccess.com/auth?token=secret",
                    "terms_accepted" to true,
                ),
            ),
            result,
        )

        assertEquals("ZERO_TRUST_DIAGNOSTIC_INVALID_DEVICE_ID", result.errorCode)
        assertTrue(result.errorMessage!!.contains("invalid_device_id"))
        assertFalse(result.errorMessage!!.contains("secret"))
    }

    @Test
    fun missingMetadataOnZeroTrustEndpointIsInvalidAndBlocksExport() {
        identityStore.put(
            "p1",
            SecureIdentityStore.Record.WARP_SECRET,
            "stored-secret".toByteArray(),
        )
        engineBridge.profileCatalogJson =
            """
            {
              "profiles":[{
                "id":"p1",
                "endpoint_v4":"162.159.198.2",
                "endpoint_v6":"2606:4700:103::2",
                "endpoint_port":443,
                "sni":"speed.cloudflare.com",
                "identity_provider":"zero_trust",
                "identity_organization":"example-team"
              }]
            }
            """.trimIndent()

        val catalogResult = RecordingResult()
        handler.handle(
            MethodCall(
                "importLegacyProfiles",
                mapOf(
                    "profiles" to emptyList<Any>(),
                    "active_profile_id" to "",
                ),
            ),
            catalogResult,
        )
        @Suppress("UNCHECKED_CAST")
        val catalog = catalogResult.successValue as Map<String, Any?>

        @Suppress("UNCHECKED_CAST")
        val statuses = catalog["identity_statuses"] as List<Map<String, Any?>>
        assertEquals("invalid", statuses.single()["state"])
        assertEquals("zeroTrust", statuses.single()["provider"])
        assertEquals("notApplicable", statuses.single()["license_state"])

        val exportResult = RecordingResult()
        handler.handle(
            MethodCall("exportWarpSecret", mapOf("profile_id" to "p1")),
            exportResult,
        )
        assertEquals("IDENTITY_OPERATION_UNSUPPORTED", exportResult.errorCode)
    }

    @Test
    fun catalogUsesWarpPlusMetadataInsteadOfAStoredLicense() {
        identityStore.put(
            "p1",
            SecureIdentityStore.Record.WARP_SECRET,
            "stored-secret".toByteArray(),
        )
        identityStore.put(
            "p1",
            SecureIdentityStore.Record.LICENSE,
            "free-sharing-key".toByteArray(),
        )
        identityStore.put(
            "p1",
            SecureIdentityStore.Record.IDENTITY_METADATA,
            """{"version":1,"provider":"consumer","entitlement":"free"}""".toByteArray(),
        )
        engineBridge.profileCatalogJson = """{"profiles":[{"id":"p1"}]}"""

        val statuses = catalogIdentityStatuses()
        assertEquals("ready", statuses.single()["state"])
        assertEquals("free", statuses.single()["license_state"])
        assertEquals("Free", statuses.single()["account_type"])
    }

    @Test
    fun catalogMarksWarpPlusFromEntitlementEvenWithoutALicenseRecord() {
        identityStore.put(
            "p1",
            SecureIdentityStore.Record.WARP_SECRET,
            "stored-secret".toByteArray(),
        )
        identityStore.put(
            "p1",
            SecureIdentityStore.Record.IDENTITY_METADATA,
            """{"version":1,"provider":"consumer","entitlement":"warp_plus"}""".toByteArray(),
        )
        engineBridge.profileCatalogJson = """{"profiles":[{"id":"p1"}]}"""

        val statuses = catalogIdentityStatuses()
        assertEquals("warpPlus", statuses.single()["license_state"])
        assertEquals("WARP+", statuses.single()["account_type"])
    }

    @Test
    fun catalogIgnoresLegacyApiWarpPlusBoolean() {
        identityStore.put(
            "p1",
            SecureIdentityStore.Record.WARP_SECRET,
            "stored-secret".toByteArray(),
        )
        identityStore.put(
            "p1",
            SecureIdentityStore.Record.LICENSE,
            "api-sharing-key".toByteArray(),
        )
        identityStore.put(
            "p1",
            SecureIdentityStore.Record.IDENTITY_METADATA,
            """{"version":1,"provider":"consumer","warp_plus":true}""".toByteArray(),
        )
        engineBridge.profileCatalogJson = """{"profiles":[{"id":"p1"}]}"""

        val statuses = catalogIdentityStatuses()
        assertEquals("free", statuses.single()["license_state"])
        assertEquals("Free", statuses.single()["account_type"])
    }

    @Test
    fun catalogWithoutMetadataOrLicenseIsFree() {
        identityStore.put(
            "p1",
            SecureIdentityStore.Record.WARP_SECRET,
            "stored-secret".toByteArray(),
        )
        engineBridge.profileCatalogJson = """{"profiles":[{"id":"p1"}]}"""

        val statuses = catalogIdentityStatuses()
        assertEquals("free", statuses.single()["license_state"])
        assertEquals("Free", statuses.single()["account_type"])
    }

    @Suppress("UNCHECKED_CAST")
    private fun catalogIdentityStatuses(): List<Map<String, Any?>> {
        val catalogResult = RecordingResult()
        handler.handle(
            MethodCall(
                "importLegacyProfiles",
                mapOf(
                    "profiles" to emptyList<Any>(),
                    "active_profile_id" to "",
                ),
            ),
            catalogResult,
        )
        val catalog = catalogResult.successValue as Map<String, Any?>
        return catalog["identity_statuses"] as List<Map<String, Any?>>
    }

    @Test
    fun missingMetadataWithBoundTeamAllowsOnlySameTeamRepair() {
        identityStore.put(
            "p1",
            SecureIdentityStore.Record.WARP_SECRET,
            "old-secret".toByteArray(),
        )
        engineBridge.profileCatalogJson =
            """
            {
              "profiles":[{
                "id":"p1",
                "identity_provider":"zero_trust",
                "identity_organization":"example-team"
              }]
            }
            """.trimIndent()

        val repaired = RecordingResult()
        handler.handle(
            MethodCall(
                "provisionIdentity",
                mapOf(
                    "profile_id" to "p1",
                    "method" to "zeroTrust",
                    "team_name" to "example-team",
                    "callback_uri" to
                        "com.cloudflare.warp://example-team.cloudflareaccess.com/auth?token=test",
                    "terms_accepted" to true,
                ),
            ),
            repaired,
        )
        assertNull(repaired.errorCode)
        assertTrue(
            engineBridge.commands.any {
                it.contains("\"identity_provider\":\"zero_trust\"") &&
                    it.contains("\"organization\":\"example-team\"")
            },
        )

        identityStore.delete("p1", SecureIdentityStore.Record.IDENTITY_METADATA)
        val crossTeam = RecordingResult()
        handler.handle(
            MethodCall(
                "provisionIdentity",
                mapOf(
                    "profile_id" to "p1",
                    "method" to "zeroTrust",
                    "team_name" to "other-team",
                    "callback_uri" to
                        "com.cloudflare.warp://other-team.cloudflareaccess.com/auth?token=test",
                    "terms_accepted" to true,
                ),
            ),
            crossTeam,
        )
        assertEquals("IDENTITY_PROVIDER_CHANGE_UNSUPPORTED", crossTeam.errorCode)
    }

    @Test
    fun finishClearAllDataCompletesOnlyWhenStillInFlight() {
        val localScheduler = ImmediateScheduler()
        val localClient =
            VpnControlClient(
                scheduler = localScheduler,
                serviceBinder = { true },
                serviceUnbinder = { },
                endpointFromBinder = { _, _ -> error("unused") },
            )
        val localEndpoint = RecordingEndpoint()
        localClient.attachEndpointForTest(localEndpoint)
        val localHandler =
            AndroidEngineMethodHandler(
                profileConfigPath = "/tmp/profiles-v2.json",
                identityStore = identityStore,
                identityExecutor = Executor { it.run() },
                mainScheduler = localScheduler,
                controlClient = localClient,
                activityCommands = activityCommands,
                engineBridge = engineBridge,
                maintenanceBridge = maintenance,
                warpSecretOkCode = 0,
            )
        localClient.clearAllAcknowledgedListener =
            VpnControlClient.ClearAllAcknowledgedListener { r ->
                localHandler.finishClearAllData(r)
            }

        val wipeResult = RecordingResult()
        assertTrue(localClient.requestClearAllData(wipeResult))
        localClient.deliverSnapshotReply(
            localEndpoint.messages.single().second,
            null,
            null,
            mapOf("phase" to "disconnected"),
        )

        assertEquals(1, wipeResult.completionCount)
        assertNull(wipeResult.errorCode)
        assertTrue(identityStore.clearAllCount >= 1)
        assertTrue(engineBridge.commands.any { it.contains("clear_all_data") })
        assertTrue(maintenance.clearCount >= 1)
    }

    @Test
    fun finishClearAllDataSkippedAfterDestroyCancel() {
        val localScheduler = ImmediateScheduler()
        val deferred = mutableListOf<Runnable>()
        val localIdentity = RecordingIdentityStore()
        val localEngine = FakeEngineBridge()
        val localMaintenance = RecordingMaintenance()
        val localClient =
            VpnControlClient(
                scheduler = localScheduler,
                serviceBinder = { true },
                serviceUnbinder = { },
                endpointFromBinder = { _, _ -> error("unused") },
            )
        val localEndpoint = RecordingEndpoint()
        localClient.attachEndpointForTest(localEndpoint)
        val localHandler =
            AndroidEngineMethodHandler(
                profileConfigPath = "/tmp/profiles-v2.json",
                identityStore = localIdentity,
                identityExecutor = Executor { deferred.add(it) },
                mainScheduler = localScheduler,
                controlClient = localClient,
                activityCommands = activityCommands,
                engineBridge = localEngine,
                maintenanceBridge = localMaintenance,
                warpSecretOkCode = 0,
            )
        localClient.clearAllAcknowledgedListener =
            VpnControlClient.ClearAllAcknowledgedListener { r ->
                localHandler.finishClearAllData(r)
            }

        val wipeResult = RecordingResult()
        assertTrue(localClient.requestClearAllData(wipeResult))
        localClient.deliverSnapshotReply(
            localEndpoint.messages.single().second,
            null,
            null,
            mapOf("phase" to "disconnected"),
        )
        assertEquals(1, deferred.size)
        assertEquals(0, wipeResult.completionCount)

        localClient.destroy()
        assertEquals("CLEAR_ALL_CANCELLED", wipeResult.errorCode)
        assertEquals(1, wipeResult.completionCount)

        // Queued wipe runs after destroy — must not complete again or mutate state.
        val clearAllBefore = localIdentity.clearAllCount
        val commandsBefore = localEngine.commands.size
        val maintenanceBefore = localMaintenance.clearCount
        deferred.forEach { it.run() }
        assertEquals(1, wipeResult.completionCount)
        assertEquals("CLEAR_ALL_CANCELLED", wipeResult.errorCode)
        assertEquals(clearAllBefore, localIdentity.clearAllCount)
        assertEquals(commandsBefore, localEngine.commands.size)
        assertEquals(maintenanceBefore, localMaintenance.clearCount)
        assertFalse(localEngine.commands.any { it.contains("clear_all_data") })
    }

    @Test
    fun finishClearAllDataClaimedWipeReportsRealResultAfterDestroy() {
        val localScheduler = ImmediateScheduler()
        val wipeStarted = CountDownLatch(1)
        val releaseWipe = CountDownLatch(1)
        val localIdentity = BlockingClearIdentityStore(wipeStarted, releaseWipe)
        val localEngine = FakeEngineBridge()
        val localMaintenance = RecordingMaintenance()
        val wipeExecutor = Executors.newSingleThreadExecutor()
        val localClient =
            VpnControlClient(
                scheduler = localScheduler,
                serviceBinder = { true },
                serviceUnbinder = { },
                endpointFromBinder = { _, _ -> error("unused") },
            )
        val localEndpoint = RecordingEndpoint()
        localClient.attachEndpointForTest(localEndpoint)
        val localHandler =
            AndroidEngineMethodHandler(
                profileConfigPath = "/tmp/profiles-v2.json",
                identityStore = localIdentity,
                identityExecutor = wipeExecutor,
                mainScheduler = localScheduler,
                controlClient = localClient,
                activityCommands = activityCommands,
                engineBridge = localEngine,
                maintenanceBridge = localMaintenance,
                warpSecretOkCode = 0,
            )
        localClient.clearAllAcknowledgedListener =
            VpnControlClient.ClearAllAcknowledgedListener { r ->
                localHandler.finishClearAllData(r)
            }

        try {
            val wipeResult = RecordingResult()
            assertTrue(localClient.requestClearAllData(wipeResult))
            localClient.deliverSnapshotReply(
                localEndpoint.messages.single().second,
                null,
                null,
                mapOf("phase" to "disconnected"),
            )

            assertTrue(wipeStarted.await(5, TimeUnit.SECONDS))
            assertSame(wipeResult, localClient.claimedClearAllForTest())

            // Once destructive work is claimed, destroy must not falsely report cancellation.
            localClient.destroy()
            assertEquals(0, wipeResult.completionCount)

            releaseWipe.countDown()
            wipeExecutor.shutdown()
            assertTrue(wipeExecutor.awaitTermination(5, TimeUnit.SECONDS))

            assertEquals(1, wipeResult.completionCount)
            assertNull(wipeResult.errorCode)
            assertEquals(1, localIdentity.clearAllCount)
            assertTrue(localEngine.commands.any { it.contains("clear_all_data") })
            assertEquals(1, localMaintenance.clearCount)
            assertNull(localClient.claimedClearAllForTest())
        } finally {
            releaseWipe.countDown()
            wipeExecutor.shutdownNow()
        }
    }

    private class RecordingActivityCommands : AndroidEngineMethodHandler.ActivityCommands {
        var cancelCount = 0
        var lastCancelCode: String? = null
        var connectCount = 0
        var lastProfileJson: String? = null
        var lastMode: String? = null
        var diagnosticsCount = 0

        override fun cancelPendingVpnConnection(
            code: String,
            message: String,
        ) {
            cancelCount += 1
            lastCancelCode = code
        }

        override fun connectAfterValidation(
            profileJson: String,
            mode: String,
            result: MethodChannel.Result,
        ) {
            connectCount += 1
            lastProfileJson = profileJson
            lastMode = mode
            result.success(mapOf("phase" to "preparing"))
        }

        override fun selectDiagnosticsDestination(result: MethodChannel.Result) {
            diagnosticsCount += 1
            result.success(null)
        }

        override fun selectWarpSecretDestination(
            profileId: String,
            result: MethodChannel.Result,
        ) {
            result.success(null)
        }

        override fun copySensitiveText(
            label: String,
            value: String,
        ) = Unit

        override fun consumeLaunchTarget(): String? = null

        override fun beginZeroTrustLogin(team: String): String = "https://$team.cloudflareaccess.com/warp"

        override fun consumeZeroTrustCallback(): String? = null

        override fun cancelZeroTrustLogin() = Unit

        override fun platformPreferences(): Map<String, Any?> = mapOf("start_on_boot" to false, "close_to_tray" to true)

        override fun setStartOnBoot(enabled: Boolean) = Unit

        override fun requestAddQuickSettingsTile(result: MethodChannel.Result) {
            result.success(null)
        }

        override fun openAlwaysOnVpnSettings() = Unit

        override fun listInstalledApps(): List<Map<String, Any?>> = emptyList()

        override fun getAppIcon(packageName: String): ByteArray? = null

        override fun loadPerAppProxy(): Map<String, Any?> =
            mapOf("enabled" to false, "package_names" to emptyList<String>())

        override fun savePerAppProxy(
            enabled: Boolean,
            packageNames: List<String>,
        ): Map<String, Any?> = mapOf("enabled" to enabled, "package_names" to packageNames)
    }

    private class FakeEngineBridge : AndroidEngineMethodHandler.EngineBridge {
        var ready = true
        var linked = true
        var zeroTrustFailure: IOException? = null
        var consumerLicenseFailure: IOException? = null
        var profileCatalogJson = """{"profiles":[{"id":"p1"}]}"""
        val commands = mutableListOf<String>()

        override fun isLinked(): Boolean = linked

        override fun isReady(): Boolean = ready

        override fun applyProfileCommand(
            configPath: String,
            requestJson: String,
        ): String? {
            commands.add(requestJson)
            return profileCatalogJson
        }

        override fun registerConsumerWarp(locale: String): ByteArray? = byteArrayOf(1, 2, 3)

        override fun registerConsumerWarpWithLicense(
            locale: String,
            licenseKey: String,
        ): ByteArray? {
            consumerLicenseFailure?.let { throw it }
            return byteArrayOf(4, 5, 6)
        }

        override fun registerZeroTrustWarp(
            locale: String,
            team: String,
            callbackUri: String,
        ): ByteArray? {
            zeroTrustFailure?.let { throw it }
            return """
                {
                  "warp_secret":"secret",
                  "identity_metadata":
                    "{\"version\":1,\"provider\":\"zero_trust\",\"organization\":\"$team\"}",
                  "endpoint_v4":"162.159.197.2",
                  "endpoint_v6":"2606:4700:102::2",
                  "endpoint_port":443,
                  "sni":"zt-masque.cloudflareclient.com"
                }
                """.trimIndent()
                .toByteArray()
        }

        override fun unbindConsumerWarp(warpSecret: ByteArray): Boolean = true

        override fun validateWarpSecret(secret: ByteArray): Int = 0
    }

    private class RecordingMaintenance : AndroidEngineMethodHandler.MaintenanceBridge {
        var checkCount = 0
        var lastManual: Boolean? = null
        var clearCount = 0

        override fun checkForUpdates(manual: Boolean): Map<String, Any?> {
            checkCount += 1
            lastManual = manual
            return mapOf("manual" to manual, "available" to false)
        }

        override fun clearLocalState() {
            clearCount += 1
        }
    }

    private class RecordingIdentityStore : AndroidEngineMethodHandler.IdentityStore {
        var putCount = 0
        var clearAllCount = 0
        var deleteIdentityCount = 0
        var failOnPut: SecureIdentityStore.Record? = null
        private val records = mutableMapOf<Pair<String, SecureIdentityStore.Record>, ByteArray>()

        override fun put(
            profileId: String,
            record: SecureIdentityStore.Record,
            value: ByteArray,
        ) {
            putCount += 1
            if (record == failOnPut) throw IllegalStateException("injected secure-storage failure")
            records[profileId to record] = value.clone()
        }

        override fun get(
            profileId: String,
            record: SecureIdentityStore.Record,
        ): ByteArray? = records[profileId to record]?.clone()

        override fun delete(
            profileId: String,
            record: SecureIdentityStore.Record,
        ) {
            records.remove(profileId to record)
        }

        override fun deleteIdentity(profileId: String) {
            deleteIdentityCount += 1
            records.keys.removeAll { it.first == profileId }
        }

        override fun clearAll() {
            clearAllCount += 1
            records.clear()
        }
    }

    private class BlockingClearIdentityStore(
        private val wipeStarted: CountDownLatch,
        private val releaseWipe: CountDownLatch,
    ) : AndroidEngineMethodHandler.IdentityStore {
        var clearAllCount = 0

        override fun put(
            profileId: String,
            record: SecureIdentityStore.Record,
            value: ByteArray,
        ) = Unit

        override fun get(
            profileId: String,
            record: SecureIdentityStore.Record,
        ): ByteArray? = null

        override fun delete(
            profileId: String,
            record: SecureIdentityStore.Record,
        ) = Unit

        override fun deleteIdentity(profileId: String) = Unit

        override fun clearAll() {
            wipeStarted.countDown()
            if (!releaseWipe.await(5, TimeUnit.SECONDS)) {
                throw IllegalStateException("Timed out waiting to release the clear-all test")
            }
            clearAllCount += 1
        }
    }

    private class RecordingEndpoint : VpnControlClient.ControlEndpoint {
        val whats = mutableListOf<Int>()
        var lastExtras: Map<String, Any?>? = null
        val messages = mutableListOf<Pair<Int, Int>>()

        override fun send(
            what: Int,
            requestId: Int,
            extras: Map<String, Any?>?,
        ): Boolean {
            whats.add(what)
            messages.add(what to requestId)
            lastExtras = extras
            return true
        }
    }

    private class ImmediateScheduler : VpnControlClient.MainScheduler {
        override fun post(action: () -> Unit) = action()

        override fun postDelayed(
            delayMillis: Long,
            token: Any,
            action: () -> Unit,
        ) = Unit

        override fun cancel(token: Any) = Unit
    }

    private class RecordingResult : MethodChannel.Result {
        var completionCount = 0
        var successValue: Any? = null
        var errorCode: String? = null
        var errorMessage: String? = null
        var errorDetails: Any? = null
        var notImplementedCalled = false

        override fun success(result: Any?) {
            completionCount += 1
            successValue = result
        }

        override fun error(
            errorCode: String,
            errorMessage: String?,
            errorDetails: Any?,
        ) {
            completionCount += 1
            this.errorCode = errorCode
            this.errorMessage = errorMessage
            this.errorDetails = errorDetails
        }

        override fun notImplemented() {
            completionCount += 1
            notImplementedCalled = true
        }
    }
}
