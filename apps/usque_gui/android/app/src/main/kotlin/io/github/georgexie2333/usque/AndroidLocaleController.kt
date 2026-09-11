package io.github.georgexie2333.usque

import android.content.Context
import android.content.res.Configuration
import androidx.annotation.StringRes
import java.util.Locale

/**
 * Resolves native Android copy from the same catalog selected by Flutter.
 *
 * The selected catalog is stored in device-protected preferences so the
 * dedicated VPN process can localize recovery notifications before unlock.
 * Live processes are also updated over Binder; SharedPreferences is not used
 * as a cross-process change-notification mechanism.
 */
internal object AndroidLocaleController {
    const val SYSTEM_CATALOG = "system"

    private const val PREFERENCES = "usque_native_locale_v1"
    private const val CATALOG_KEY = "native_locale_catalog"

    private val languageTags =
        mapOf(
            "en" to "en",
            "zh_CN" to "zh-CN",
            "zh_HK" to "zh-HK",
            "zh_TW" to "zh-TW",
            "ja" to "ja",
            "ko" to "ko",
            "es" to "es",
            "pt" to "pt-BR",
            "fr" to "fr",
            "nl" to "nl",
            "tr" to "tr",
            "ru" to "ru",
            "fa" to "fa",
            "ar" to "ar",
            "de" to "de",
            "id" to "id",
            "it" to "it",
            "pl" to "pl",
            "th" to "th",
            "uk" to "uk",
            "vi" to "vi",
        )

    @Volatile
    private var processCatalog: String? = null

    fun persist(
        context: Context,
        catalogId: String,
    ): Boolean {
        if (!isSupported(catalogId)) return false
        val saved =
            context
                .createDeviceProtectedStorageContext()
                .getSharedPreferences(PREFERENCES, Context.MODE_PRIVATE)
                .edit()
                .putString(CATALOG_KEY, catalogId)
                .commit()
        if (saved) processCatalog = catalogId
        return saved
    }

    /** Updates this process after the main process has persisted the value. */
    fun applyToProcess(catalogId: String): Boolean {
        if (!isSupported(catalogId)) return false
        processCatalog = catalogId
        return true
    }

    fun clearProcessOverride() {
        processCatalog = SYSTEM_CATALOG
    }

    fun clear(context: Context): Boolean {
        val cleared =
            context
                .createDeviceProtectedStorageContext()
                .getSharedPreferences(PREFERENCES, Context.MODE_PRIVATE)
                .edit()
                .clear()
                .commit()
        if (cleared) clearProcessOverride()
        return cleared
    }

    fun getString(
        context: Context,
        @StringRes resource: Int,
        vararg arguments: Any,
    ): String {
        val localized = localizedContext(context)
        return if (arguments.isEmpty()) {
            localized.getString(resource)
        } else {
            localized.getString(resource, *arguments)
        }
    }

    internal fun languageTagForCatalog(catalogId: String): String? =
        if (catalogId == SYSTEM_CATALOG) null else languageTags[catalogId]

    private fun localizedContext(context: Context): Context {
        val catalog = processCatalog ?: readPersistedCatalog(context).also { processCatalog = it }
        val languageTag = languageTagForCatalog(catalog) ?: return context
        val locale = Locale.forLanguageTag(languageTag)
        val configuration = Configuration(context.resources.configuration)
        configuration.setLocale(locale)
        configuration.setLayoutDirection(locale)
        return context.createConfigurationContext(configuration)
    }

    private fun readPersistedCatalog(context: Context): String {
        val stored =
            context
                .createDeviceProtectedStorageContext()
                .getSharedPreferences(PREFERENCES, Context.MODE_PRIVATE)
                .getString(CATALOG_KEY, SYSTEM_CATALOG)
        return stored?.takeIf(::isSupported) ?: SYSTEM_CATALOG
    }

    private fun isSupported(catalogId: String): Boolean =
        catalogId == SYSTEM_CATALOG || languageTags.containsKey(catalogId)
}
