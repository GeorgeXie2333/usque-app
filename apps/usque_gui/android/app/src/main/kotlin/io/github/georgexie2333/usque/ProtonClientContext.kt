package io.github.georgexie2333.usque

import android.app.usage.StorageStatsManager
import android.content.Context
import android.content.pm.PackageManager
import android.content.res.Configuration
import android.os.Build
import android.os.storage.StorageManager
import android.util.DisplayMetrics
import android.view.WindowManager
import android.view.inputmethod.InputMethodManager
import androidx.core.os.LocaleListCompat
import org.json.JSONArray
import org.json.JSONObject
import java.io.File
import java.util.Calendar
import java.util.Locale
import java.util.TimeZone
import java.util.concurrent.TimeUnit
import kotlin.math.sqrt

/** Real platform facts for the private JNI channel; never status, settings or logs. */
internal object ProtonClientContext {
    data class Facts(
        val osRelease: String,
        val brand: String,
        val model: String,
        val locale: String,
        val device: String,
        val isTv: Boolean,
        val appLanguage: String,
        val timezone: String,
        val regionCode: String,
        val timezoneOffset: Int,
        val rooted: Boolean,
        val fontScale: Float,
        val storageBytes: Long,
        val darkMode: Boolean,
        val keyboards: List<String>,
    )

    fun capture(context: Context): String {
        val configuration = context.resources.configuration
        val calendar = Calendar.getInstance()
        val inputMethods = context.getSystemService(Context.INPUT_METHOD_SERVICE) as InputMethodManager
        return encode(
            Facts(
                osRelease = Build.VERSION.RELEASE,
                brand = Build.BRAND,
                model = Build.MODEL,
                locale = Locale.getDefault().language,
                device = Build.DEVICE,
                isTv = isTv(context),
                appLanguage = requireNotNull(LocaleListCompat.getDefault()[0]).language,
                timezone = TimeZone.getDefault().id,
                regionCode = configuration.locales[0].country,
                timezoneOffset = -(calendar.get(Calendar.ZONE_OFFSET) + calendar.get(Calendar.DST_OFFSET)) / 60_000,
                rooted = isRooted(context),
                fontScale = configuration.fontScale,
                storageBytes = storageBytes(context),
                darkMode = configuration.uiMode and Configuration.UI_MODE_NIGHT_MASK == Configuration.UI_MODE_NIGHT_YES,
                keyboards = inputMethods.enabledInputMethodList.map { it.packageName },
            ),
        )
    }

    fun encode(facts: Facts): String =
        JSONObject()
            .put("os_release", facts.osRelease)
            .put("brand", facts.brand)
            .put("model", facts.model)
            .put("locale", facts.locale)
            .put("is_tv", facts.isTv)
            .put(
                "device",
                JSONObject()
                    .put("appLang", facts.appLanguage)
                    .put("timezone", facts.timezone)
                    .put("deviceName", deviceHash("${facts.model}/${facts.brand} ${facts.device}"))
                    .put("regionCode", facts.regionCode)
                    .put("timezoneOffset", facts.timezoneOffset)
                    .put("isJailbreak", facts.rooted)
                    .put("preferredContentSize", facts.fontScale.toString())
                    // Core divides Long bytes by decimal GB before converting to Double.
                    .put("storageCapacity", (facts.storageBytes / 1_000_000_000L).toDouble())
                    .put("isDarkmodeOn", facts.darkMode)
                    .put("keyboards", JSONArray(facts.keyboards)),
            ).toString()

    fun deviceHash(description: String): Long {
        var weight = 1L
        var result = 0L
        for (byte in description.toByteArray(Charsets.UTF_8)) {
            result += byte.toLong() * weight
            weight = weight * 7L % 100_000_007L
        }
        return result
    }

    @Suppress("DEPRECATION")
    private fun isTv(context: Context): Boolean {
        val packages = context.packageManager
        val fireTv = packages.hasSystemFeature("amazon.hardware.fire_tv")
        if (Build.MANUFACTURER == "Amazon") return fireTv
        if (context.resources.configuration.uiMode and Configuration.UI_MODE_TYPE_MASK ==
            Configuration.UI_MODE_TYPE_TELEVISION ||
            packages.hasSystemFeature(PackageManager.FEATURE_TELEVISION) ||
            packages.hasSystemFeature(PackageManager.FEATURE_LEANBACK) || fireTv
        ) {
            return true
        }
        if (!packages.hasSystemFeature(PackageManager.FEATURE_LIVE_TV)) return false
        val metrics = DisplayMetrics()
        (context.getSystemService(Context.WINDOW_SERVICE) as WindowManager).defaultDisplay.getRealMetrics(metrics)
        val width = metrics.widthPixels / metrics.xdpi
        val height = metrics.heightPixels / metrics.ydpi
        return sqrt(width * width + height * height) >= 10f
    }

    private fun storageBytes(context: Context): Long {
        val storage = context.getSystemService(Context.STORAGE_SERVICE) as StorageManager
        val stats = context.getSystemService(Context.STORAGE_STATS_SERVICE) as StorageStatsManager
        return context.getExternalFilesDirs(null).filterNotNull().sumOf { directory ->
            val volume = storage.getStorageVolume(directory)
            when {
                volume == null -> 0L
                volume.isPrimary -> stats.getTotalBytes(StorageManager.UUID_DEFAULT)
                else -> directory.totalSpace
            }
        }
    }

    private fun isRooted(context: Context): Boolean =
        Build.TAGS?.contains("test-keys") == true ||
            rootPaths.any { File(it).exists() } ||
            hasSuCommand() ||
            rootPackages.any { installed(context.packageManager, it) }

    private fun hasSuCommand(): Boolean {
        var process: Process? = null
        return try {
            process = ProcessBuilder("/system/xbin/which", "su").start()
            process.waitFor(500, TimeUnit.MILLISECONDS) &&
                process.inputStream.bufferedReader().use { it.readLine() != null }
        } catch (_: Exception) {
            false
        } finally {
            process?.destroy()
        }
    }

    @Suppress("DEPRECATION")
    private fun installed(
        packages: PackageManager,
        name: String,
    ): Boolean =
        try {
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
                packages.getPackageInfo(name, PackageManager.PackageInfoFlags.of(0))
            } else {
                packages.getPackageInfo(name, 0)
            }
            true
        } catch (_: PackageManager.NameNotFoundException) {
            false
        }

    private val rootPaths =
        listOf(
            "/system/app/Superuser.apk",
            "/sbin/su",
            "/system/bin/su",
            "/system/xbin/su",
            "/data/local/xbin/su",
            "/data/local/bin/su",
            "/system/sd/xbin/su",
            "/system/bin/failsafe/su",
            "/data/local/su",
            "/su/bin/su",
            "/su/bin",
            "/system/xbin/daemonsu",
        )
    private val rootPackages =
        listOf(
            "com.devadvance.rootcloak",
            "com.devadvance.rootcloakplus",
            "com.koushikdutta.superuser",
            "com.thirdparty.superuser",
            "eu.chainfire.supersu",
            "com.noshufou.android.su",
        )
}
