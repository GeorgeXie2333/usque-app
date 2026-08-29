package io.github.georgexie2333.usque

import android.app.PendingIntent
import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.content.pm.PackageInfo
import android.content.pm.PackageInstaller
import android.content.pm.PackageManager
import android.os.Build
import androidx.core.content.edit
import java.io.File
import java.io.FileInputStream
import java.security.MessageDigest
import java.util.Locale
import java.util.zip.ZipFile

internal class AndroidUpdateInstaller(
    private val context: Context,
) {
    data class Request(
        val path: String,
        val version: String,
        val name: String,
        val size: Long,
        val sha256: String,
        val platform: String,
        val variant: String,
    )

    class UpdateException(
        val code: String,
        override val message: String,
    ) : Exception(message)

    val cacheDirectory: File
        get() = File(context.cacheDir, UPDATE_DIRECTORY)

    fun prepareCache(recoverAbandonedOperation: Boolean = false) {
        val root = cacheDirectory
        root.mkdirs()
        if (recoverAbandonedOperation) recoverAbandonedOperation(root)
        cleanupStale(root)
    }

    fun trackPending(request: Request) {
        prepareCache()
        val apk = managedRequestFile(request)
        val persisted =
            installPreferences()
                .edit()
                .putString(PENDING_REQUEST_PATH, apk.absolutePath)
                .commit()
        if (!persisted) {
            throw UpdateException(
                "UPDATE_INSTALL_SESSION_FAILED",
                "Android could not persist the pending update installation.",
            )
        }
    }

    fun verify(
        request: Request,
        allowPartial: Boolean = false,
        cancelled: () -> Boolean = { false },
    ): File {
        ensureActive(cancelled)
        prepareCache()
        val apk = managedRequestFile(request, allowPartial)
        if (!apk.isFile || request.size <= 0L || apk.length() != request.size) {
            throw UpdateException(
                "UPDATE_PACKAGE_SIZE_MISMATCH",
                "The update APK size does not match its release metadata.",
            )
        }
        if (request.platform != "android" || request.variant != runtimeVariant()) {
            throw UpdateException(
                "UPDATE_PACKAGE_ABI_MISMATCH",
                "The update APK does not match this Android device ABI.",
            )
        }
        ensureActive(cancelled)
        val expectedDigest = request.sha256.lowercase(Locale.ROOT)
        if (!SHA256.matches(expectedDigest) || sha256(apk, cancelled) != expectedDigest) {
            throw UpdateException(
                "UPDATE_PACKAGE_DIGEST_MISMATCH",
                "The update APK SHA-256 digest does not match the release manifest.",
            )
        }

        ensureActive(cancelled)
        val current = currentPackageInfo()
        val archive = archivePackageInfo(apk)
        if (archive.packageName != context.packageName) {
            throw UpdateException(
                "UPDATE_PACKAGE_IDENTITY_MISMATCH",
                "The update APK belongs to a different Android package.",
            )
        }
        if (normalizeVersion(archive.versionName) != normalizeVersion(request.version) ||
            longVersionCode(archive) <= longVersionCode(current)
        ) {
            throw UpdateException(
                "UPDATE_PACKAGE_VERSION_INVALID",
                "The update APK must have the advertised version and a higher version code.",
            )
        }
        if (signerDigests(current) != signerDigests(archive)) {
            throw UpdateException(
                "UPDATE_PACKAGE_SIGNER_MISMATCH",
                "The update APK is not signed by the installed Usque identity.",
            )
        }
        ensureActive(cancelled)
        ZipFile(apk).use { zip ->
            val prefix = "lib/${request.variant}/"
            val entries = zip.entries()
            var containsRuntimeVariant = false
            while (entries.hasMoreElements()) {
                ensureActive(cancelled)
                val entry = entries.nextElement()
                if (!entry.isDirectory && entry.name.startsWith(prefix)) {
                    containsRuntimeVariant = true
                    break
                }
            }
            if (!containsRuntimeVariant) {
                throw UpdateException(
                    "UPDATE_PACKAGE_ABI_MISMATCH",
                    "The update APK does not contain native code for this Android device ABI.",
                )
            }
        }
        return apk
    }

    fun commit(
        request: Request,
        cancelled: () -> Boolean = { false },
    ): Int {
        val apk = verify(request, cancelled = cancelled)
        val installer = context.packageManager.packageInstaller
        val parameters = PackageInstaller.SessionParams(PackageInstaller.SessionParams.MODE_FULL_INSTALL)
        parameters.setAppPackageName(context.packageName)
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.S) {
            parameters.setRequireUserAction(PackageInstaller.SessionParams.USER_ACTION_REQUIRED)
        }
        ensureActive(cancelled)
        var sessionId: Int? = null
        try {
            val createdSessionId = installer.createSession(parameters)
            sessionId = createdSessionId
            installer.openSession(createdSessionId).use { session ->
                FileInputStream(apk).use { source ->
                    session.openWrite(request.name, 0L, request.size).use { target ->
                        val buffer = ByteArray(DEFAULT_BUFFER_SIZE)
                        while (true) {
                            ensureActive(cancelled)
                            val read = source.read(buffer)
                            if (read < 0) break
                            target.write(buffer, 0, read)
                        }
                        session.fsync(target)
                    }
                }
                ensureActive(cancelled)
                val journaled =
                    installPreferences()
                        .edit()
                        .putString(sessionKey(createdSessionId), apk.absolutePath)
                        .commit()
                if (!journaled) {
                    throw UpdateException(
                        "UPDATE_INSTALL_SESSION_FAILED",
                        "Android could not persist the update installation session.",
                    )
                }
                val callback =
                    Intent(context, UpdateInstallStatusReceiver::class.java).apply {
                        action = ACTION_INSTALL_STATUS
                        putExtra(EXTRA_SESSION_ID, createdSessionId)
                    }
                val flags =
                    PendingIntent.FLAG_UPDATE_CURRENT or
                        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.S) {
                            PendingIntent.FLAG_MUTABLE
                        } else {
                            0
                        }
                val sender =
                    PendingIntent.getBroadcast(context, createdSessionId, callback, flags).intentSender
                ensureActive(cancelled)
                session.commit(sender)
            }
            clearPendingJournal(apk)
        } catch (error: Exception) {
            if (sessionId != null) {
                runCatching { installer.abandonSession(sessionId) }
                installPreferences().edit(commit = true) { remove(sessionKey(sessionId)) }
            }
            if (!cancelled()) discard(request)
            if (error is UpdateException) throw error
            throw UpdateException(
                "UPDATE_INSTALL_SESSION_FAILED",
                "Android could not create the update installation session: ${error.javaClass.simpleName}",
            )
        }
        return checkNotNull(sessionId)
    }

    fun canRequestPackageInstalls(): Boolean = context.packageManager.canRequestPackageInstalls()

    fun discard(request: Request) {
        runCatching {
            val root = cacheDirectory.canonicalFile
            val finalFile = File(root, request.name).canonicalFile
            val partialFile = File(root, "${request.name}.part").canonicalFile
            val candidate = File(request.path).canonicalFile
            if (finalFile.parentFile != root || partialFile.parentFile != root) return@runCatching
            if (candidate != finalFile && candidate != partialFile) return@runCatching
            runCatching { finalFile.delete() }
            runCatching { partialFile.delete() }
            clearPendingJournal(finalFile)
        }
    }

    fun consumeTerminalResult(): Pair<Boolean, String?>? {
        val preferences = installPreferences()
        if (!preferences.getBoolean(TERMINAL_PENDING, false)) return null
        val result =
            preferences.getBoolean(TERMINAL_SUCCESS, false) to
                preferences.getString(TERMINAL_MESSAGE, null)
        preferences.edit(commit = true) {
            remove(TERMINAL_PENDING)
            remove(TERMINAL_SUCCESS)
            remove(TERMINAL_MESSAGE)
        }
        return result
    }

    private fun currentPackageInfo(): PackageInfo =
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
            context.packageManager.getPackageInfo(
                context.packageName,
                PackageManager.PackageInfoFlags.of(PackageManager.GET_SIGNING_CERTIFICATES.toLong()),
            )
        } else if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.P) {
            @Suppress("DEPRECATION")
            context.packageManager.getPackageInfo(context.packageName, PackageManager.GET_SIGNING_CERTIFICATES)
        } else {
            @Suppress("DEPRECATION")
            context.packageManager.getPackageInfo(context.packageName, PackageManager.GET_SIGNATURES)
        }

    private fun archivePackageInfo(apk: File): PackageInfo {
        val flags =
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.P) {
                PackageManager.GET_SIGNING_CERTIFICATES
            } else {
                @Suppress("DEPRECATION")
                PackageManager.GET_SIGNATURES
            }
        val info =
            @Suppress("DEPRECATION")
            context.packageManager.getPackageArchiveInfo(apk.absolutePath, flags)
        return info
            ?: throw UpdateException(
                "UPDATE_PACKAGE_INVALID",
                "Android could not read the update APK package metadata.",
            )
    }

    private fun signerDigests(info: PackageInfo): Set<String> {
        val signatures =
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.P) {
                val signingInfo = info.signingInfo ?: return emptySet()
                if (signingInfo.hasMultipleSigners()) {
                    signingInfo.apkContentsSigners
                } else {
                    signingInfo.signingCertificateHistory
                }
            } else {
                @Suppress("DEPRECATION")
                info.signatures ?: emptyArray()
            }
        return signatures.map { bytes -> digest(bytes.toByteArray()) }.toSet()
    }

    private fun runtimeVariant(): String {
        val abi =
            Build.SUPPORTED_ABIS
                .firstOrNull()
                .orEmpty()
                .lowercase(Locale.ROOT)
        return variantForAbi(abi)
            ?: throw UpdateException(
                "UPDATE_PACKAGE_ABI_UNSUPPORTED",
                "This Android device ABI does not have a supported Usque update package.",
            )
    }

    private fun managedRequestFile(
        request: Request,
        allowPartial: Boolean = false,
    ): File {
        val root = cacheDirectory.canonicalFile
        val apk = File(request.path).canonicalFile
        val expectedName = if (allowPartial) "${request.name}.part" else request.name
        if (apk.parentFile != root || apk.name != expectedName) {
            throw UpdateException(
                "UPDATE_PACKAGE_INVALID",
                "The update APK is outside Usque's private update cache.",
            )
        }
        return apk
    }

    private fun ensureActive(cancelled: () -> Boolean) {
        if (Thread.currentThread().isInterrupted || cancelled()) {
            throw UpdateException(
                "UPDATE_INSTALL_CANCELLED",
                "The Android update installation was cancelled.",
            )
        }
    }

    private fun sha256(
        file: File,
        cancelled: () -> Boolean,
    ): String {
        val hasher = MessageDigest.getInstance("SHA-256")
        file.inputStream().buffered().use { input ->
            val buffer = ByteArray(DEFAULT_BUFFER_SIZE)
            while (true) {
                ensureActive(cancelled)
                val read = input.read(buffer)
                if (read < 0) break
                hasher.update(buffer, 0, read)
            }
        }
        return digest(hasher.digest())
    }

    private fun digest(bytes: ByteArray): String =
        bytes.joinToString(separator = "") { byte -> "%02x".format(byte.toInt() and 0xff) }

    private fun longVersionCode(info: PackageInfo): Long =
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.P) {
            info.longVersionCode
        } else {
            @Suppress("DEPRECATION")
            info.versionCode.toLong()
        }

    private fun normalizeVersion(value: String?): String = value.orEmpty().trim().removePrefix("v")

    private fun recoverAbandonedOperation(root: File) {
        val preferences = installPreferences()
        val pendingPath = preferences.getString(PENDING_REQUEST_PATH, null)
        if (pendingPath != null) {
            runCatching {
                val canonicalRoot = root.canonicalFile
                val pending = File(pendingPath).canonicalFile
                if (pending.parentFile == canonicalRoot && isManaged(pending)) pending.delete()
            }
            preferences.edit(commit = true) { remove(PENDING_REQUEST_PATH) }
        }
    }

    private fun clearPendingJournal(apk: File) {
        val preferences = installPreferences()
        if (preferences.getString(PENDING_REQUEST_PATH, null) == apk.absolutePath) {
            preferences.edit(commit = true) { remove(PENDING_REQUEST_PATH) }
        }
    }

    private fun cleanupStale(root: File) {
        val cutoff = System.currentTimeMillis() - STALE_MILLIS
        root.listFiles()?.forEach { file ->
            if (file.isFile && isManaged(file) && file.lastModified() < cutoff) {
                runCatching { file.delete() }
            }
        }
    }

    private fun installPreferences() = context.getSharedPreferences(INSTALL_PREFERENCES, Context.MODE_PRIVATE)

    companion object {
        internal const val UPDATE_DIRECTORY = "usque-updates"
        internal const val INSTALL_PREFERENCES = "usque_update_install_v1"
        internal const val ACTION_INSTALL_STATUS = "io.github.georgexie2333.usque.UPDATE_INSTALL_STATUS"
        internal const val EXTRA_SESSION_ID = "session_id"
        internal const val TERMINAL_PENDING = "terminal_pending"
        internal const val TERMINAL_SUCCESS = "terminal_success"
        internal const val TERMINAL_MESSAGE = "terminal_message"
        internal const val PENDING_REQUEST_PATH = "pending_request_path"
        private const val STALE_MILLIS = 7L * 24L * 60L * 60L * 1000L
        private val SHA256 = Regex("^[0-9a-f]{64}$")

        @Volatile
        internal var terminalListener: ((Boolean, String?) -> Unit)? = null

        internal fun sessionKey(sessionId: Int): String = "session_$sessionId"

        internal fun isManaged(file: File): Boolean =
            file.name.startsWith("usque-v") &&
                (file.name.endsWith(".apk") || file.name.endsWith(".apk.part"))

        internal fun variantForAbi(abi: String): String? {
            val normalized = abi.lowercase(Locale.ROOT)
            return when {
                normalized.startsWith("arm64") -> "arm64-v8a"
                normalized.startsWith("x86_64") -> "x86_64"
                normalized.startsWith("armeabi") || normalized.startsWith("arm") -> "armeabi-v7a"
                else -> null
            }
        }

        internal fun containsNativeVariant(
            entries: Sequence<String>,
            variant: String,
        ): Boolean {
            val prefix = "lib/$variant/"
            return entries.any { it.startsWith(prefix) }
        }

        fun fromArguments(arguments: Map<String, Any?>): Request {
            val packageValue =
                arguments["package"] as? Map<*, *>
                    ?: throw UpdateException("INVALID_ARGUMENT", "The update package metadata is missing.")

            fun packageString(key: String): String =
                packageValue[key] as? String
                    ?: throw UpdateException("INVALID_ARGUMENT", "The update package $key is missing.")
            val rawSize =
                packageValue["size"] as? Number
                    ?: throw UpdateException("INVALID_ARGUMENT", "The update package size is missing.")
            return Request(
                path =
                    arguments["path"] as? String
                        ?: throw UpdateException("INVALID_ARGUMENT", "The update package path is missing."),
                version =
                    arguments["version"] as? String
                        ?: throw UpdateException("INVALID_ARGUMENT", "The update version is missing."),
                name = packageString("name"),
                size = rawSize.toLong(),
                sha256 = packageString("sha256"),
                platform = packageString("platform"),
                variant = packageString("variant"),
            )
        }
    }
}

internal class UpdateInstallStatusReceiver : BroadcastReceiver() {
    override fun onReceive(
        context: Context,
        intent: Intent,
    ) {
        if (intent.action != AndroidUpdateInstaller.ACTION_INSTALL_STATUS) return
        val status = intent.getIntExtra(PackageInstaller.EXTRA_STATUS, PackageInstaller.STATUS_FAILURE)
        var terminalMessage: String? = null
        if (status == PackageInstaller.STATUS_PENDING_USER_ACTION) {
            @Suppress("DEPRECATION")
            val confirmation = intent.getParcelableExtra<Intent>(Intent.EXTRA_INTENT)
            confirmation?.addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
            val launched = confirmation != null && runCatching { context.startActivity(confirmation) }.isSuccess
            if (launched) return
            terminalMessage = "Android could not open the package installation confirmation."
        }
        val sessionId = intent.getIntExtra(AndroidUpdateInstaller.EXTRA_SESSION_ID, -1)
        val preferences =
            context.getSharedPreferences(AndroidUpdateInstaller.INSTALL_PREFERENCES, Context.MODE_PRIVATE)
        val path = preferences.getString(AndroidUpdateInstaller.sessionKey(sessionId), null)
        val pendingPath = preferences.getString(AndroidUpdateInstaller.PENDING_REQUEST_PATH, null)
        if (path != null) runCatching { File(path).delete() }
        val success = status == PackageInstaller.STATUS_SUCCESS
        val message =
            if (success) {
                null
            } else if (status == PackageInstaller.STATUS_FAILURE_ABORTED) {
                "The Android update installation was cancelled."
            } else {
                terminalMessage
                    ?: intent.getStringExtra(PackageInstaller.EXTRA_STATUS_MESSAGE)
                    ?: "Android could not install the update package."
            }
        preferences.edit(commit = true) {
            remove(AndroidUpdateInstaller.sessionKey(sessionId))
            if (path != null && pendingPath == path) remove(AndroidUpdateInstaller.PENDING_REQUEST_PATH)
            putBoolean(AndroidUpdateInstaller.TERMINAL_PENDING, true)
            putBoolean(AndroidUpdateInstaller.TERMINAL_SUCCESS, success)
            if (message == null) {
                remove(AndroidUpdateInstaller.TERMINAL_MESSAGE)
            } else {
                putString(AndroidUpdateInstaller.TERMINAL_MESSAGE, message)
            }
        }
        AndroidUpdateInstaller.terminalListener?.invoke(success, message)
    }
}

internal class UpdatePackageReplacedReceiver : BroadcastReceiver() {
    override fun onReceive(
        context: Context,
        intent: Intent,
    ) {
        if (intent.action != Intent.ACTION_MY_PACKAGE_REPLACED) return
        val root = File(context.cacheDir, AndroidUpdateInstaller.UPDATE_DIRECTORY)
        root.listFiles()?.forEach { file ->
            if (file.isFile && AndroidUpdateInstaller.isManaged(file)) runCatching { file.delete() }
        }
        context
            .getSharedPreferences(AndroidUpdateInstaller.INSTALL_PREFERENCES, Context.MODE_PRIVATE)
            .edit(commit = true) { clear() }
    }
}
