package io.github.georgexie2333.usque

internal object NativeEngine {
    private val libraryLoaded: Boolean =
        try {
            System.loadLibrary("usque_android")
            true
        } catch (_: UnsatisfiedLinkError) {
            false
        }

    fun isReady(): Boolean = libraryLoaded && nativeIsReady()

    fun isLinked(): Boolean = libraryLoaded

    fun start(
        tunFileDescriptor: Int,
        profileJson: String,
        warpSecret: ByteArray,
        vpnService: UsqueVpnService,
    ): Int {
        if (!libraryLoaded) return ERROR_NOT_LINKED
        return nativeStart(tunFileDescriptor, profileJson, warpSecret, vpnService)
    }

    fun stop() {
        if (libraryLoaded) nativeStop()
    }

    fun cancel() {
        if (libraryLoaded) nativeCancel()
    }

    fun notifyNetworkChanged(generation: Long) {
        if (libraryLoaded) nativeNotifyNetworkChanged(generation)
    }

    fun startProxy(
        profileJson: String,
        warpSecret: ByteArray,
        vpnService: UsqueVpnService,
    ): Int {
        if (!libraryLoaded) return ERROR_NOT_LINKED
        return nativeStartProxy(profileJson, warpSecret, vpnService)
    }

    fun validateWarpSecret(secret: ByteArray): Int {
        if (!libraryLoaded) return ERROR_NOT_LINKED
        return nativeValidateWarpSecret(secret)
    }

    fun inspectWarpSecret(secret: ByteArray): String? {
        if (!libraryLoaded) return null
        return nativeInspectWarpSecret(secret)
    }

    fun snapshot(): String? {
        if (!libraryLoaded) return null
        return nativeSnapshot()
    }

    fun registerConsumerWarp(locale: String): ByteArray? {
        if (!libraryLoaded) return null
        return nativeRegisterConsumerWarp(locale)
    }

    fun registerConsumerWarpWithLicense(
        locale: String,
        licenseKey: String,
    ): ByteArray? {
        if (!libraryLoaded) return null
        return nativeRegisterConsumerWarpWithLicense(locale, licenseKey)
    }

    fun registerZeroTrustWarp(
        locale: String,
        team: String,
        callbackUri: String,
    ): ByteArray? {
        if (!libraryLoaded) return null
        return nativeRegisterZeroTrustWarp(locale, team, callbackUri)
    }

    fun unbindConsumerWarp(warpSecret: ByteArray): Boolean {
        if (!libraryLoaded) return false
        return nativeUnbindConsumerWarp(warpSecret) == OK
    }

    fun checkForUpdates(): String? {
        if (!libraryLoaded) return null
        return nativeCheckForUpdates()
    }

    fun applyProfileCommand(
        configPath: String,
        requestJson: String,
    ): String? {
        if (!libraryLoaded) return null
        return nativeApplyProfileCommand(configPath, requestJson)
    }

    private external fun nativeIsReady(): Boolean

    private external fun nativeStart(
        tunFileDescriptor: Int,
        profileJson: String,
        warpSecret: ByteArray,
        vpnService: UsqueVpnService,
    ): Int

    private external fun nativeStop()

    private external fun nativeCancel()

    private external fun nativeNotifyNetworkChanged(generation: Long)

    private external fun nativeStartProxy(
        profileJson: String,
        warpSecret: ByteArray,
        vpnService: UsqueVpnService,
    ): Int

    private external fun nativeValidateWarpSecret(secret: ByteArray): Int

    private external fun nativeInspectWarpSecret(secret: ByteArray): String?

    private external fun nativeSnapshot(): String?

    private external fun nativeRegisterConsumerWarp(locale: String): ByteArray?

    private external fun nativeRegisterConsumerWarpWithLicense(
        locale: String,
        licenseKey: String,
    ): ByteArray?

    private external fun nativeRegisterZeroTrustWarp(
        locale: String,
        team: String,
        callbackUri: String,
    ): ByteArray?

    private external fun nativeUnbindConsumerWarp(warpSecret: ByteArray): Int

    private external fun nativeCheckForUpdates(): String?

    private external fun nativeApplyProfileCommand(
        configPath: String,
        requestJson: String,
    ): String?

    const val OK = 0
    const val ERROR_NOT_LINKED = -2
    const val ERROR_INVALID_WARP_SECRET = -3
    const val ERROR_ALREADY_RUNNING = -4
    const val ERROR_INVALID_PROFILE = -5
    const val ERROR_PLATFORM_FAILURE = -6
    const val ERROR_TRANSPORT_FAILURE = -7
    const val ERROR_TUN_FAILURE = -8
}
