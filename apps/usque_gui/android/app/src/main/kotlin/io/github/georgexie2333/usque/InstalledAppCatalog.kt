package io.github.georgexie2333.usque

import android.Manifest
import android.content.Context
import android.content.pm.ApplicationInfo
import android.content.pm.PackageManager
import android.graphics.Bitmap
import android.graphics.Canvas
import android.graphics.drawable.BitmapDrawable
import android.graphics.drawable.Drawable
import androidx.core.graphics.createBitmap
import java.io.ByteArrayOutputStream

internal data class InstalledAppInfo(
    val packageName: String,
    val label: String,
    val isSystem: Boolean,
    val hasInternet: Boolean,
) {
    fun toMap(): Map<String, Any?> =
        mapOf(
            "package_name" to packageName,
            "label" to label,
            "is_system" to isSystem,
            "has_internet" to hasInternet,
        )
}

internal object InstalledAppCatalog {
    const val ICON_SIZE_PX = 48

    fun excludeSelf(
        apps: List<InstalledAppInfo>,
        selfPackage: String,
    ): List<InstalledAppInfo> = apps.filter { it.packageName != selfPackage }

    fun list(context: Context): List<Map<String, Any?>> {
        val packageManager = context.packageManager
        val selfPackage = context.packageName
        return excludeSelf(
            packageManager.getInstalledApplications(0).map { info ->
                InstalledAppInfo(
                    packageName = info.packageName,
                    label = info.loadLabel(packageManager).toString().take(256),
                    isSystem = info.flags and ApplicationInfo.FLAG_SYSTEM != 0,
                    hasInternet =
                        packageManager.checkPermission(
                            Manifest.permission.INTERNET,
                            info.packageName,
                        ) == PackageManager.PERMISSION_GRANTED,
                )
            },
            selfPackage,
        ).sortedWith(compareBy(String.CASE_INSENSITIVE_ORDER) { it.label })
            .map(InstalledAppInfo::toMap)
    }

    fun iconPng(
        context: Context,
        packageName: String,
        sizePx: Int = ICON_SIZE_PX,
    ): ByteArray? {
        if (packageName == context.packageName ||
            packageName.isBlank() ||
            !PerAppProxyRules.PACKAGE_NAME.matches(packageName)
        ) {
            return null
        }
        return try {
            val drawable = context.packageManager.getApplicationIcon(packageName)
            drawableToPng(drawable, sizePx)
        } catch (_: PackageManager.NameNotFoundException) {
            null
        }
    }

    internal fun drawableToPng(
        drawable: Drawable,
        sizePx: Int,
    ): ByteArray {
        val bitmap =
            when {
                drawable is BitmapDrawable &&
                    drawable.bitmap != null &&
                    drawable.bitmap.width == sizePx &&
                    drawable.bitmap.height == sizePx -> {
                    drawable.bitmap
                }

                else -> {
                    val created = createBitmap(sizePx, sizePx)
                    val canvas = Canvas(created)
                    drawable.setBounds(0, 0, sizePx, sizePx)
                    drawable.draw(canvas)
                    created
                }
            }
        val output = ByteArrayOutputStream()
        bitmap.compress(Bitmap.CompressFormat.PNG, 100, output)
        if (bitmap !== (drawable as? BitmapDrawable)?.bitmap) {
            bitmap.recycle()
        }
        return output.toByteArray()
    }
}
