package io.github.georgexie2333.usque

import io.flutter.plugins.sharedpreferences.SharedPreferencesApi
import io.flutter.plugins.sharedpreferences.SharedPreferencesAsyncApi
import io.flutter.plugins.sharedpreferences.SharedPreferencesPigeonOptions
import io.flutter.plugins.sharedpreferences.StringListResult

/** JNI enters these methods synchronously, before the delegate's first suspension. */
internal class TrackedPreferencesApi(
    private val delegate: SharedPreferencesApi,
    private val work: TrackedPreferencesWork,
) : SharedPreferencesApi {
    override suspend fun remove(key: String): Boolean = work.run { delegate.remove(key) }

    override suspend fun setBool(
        key: String,
        value: Boolean,
    ): Boolean = work.run { delegate.setBool(key, value) }

    override suspend fun setString(
        key: String,
        value: String,
    ): Boolean = work.run { delegate.setString(key, value) }

    override suspend fun setInt(
        key: String,
        value: Long,
    ): Boolean = work.run { delegate.setInt(key, value) }

    override suspend fun setDouble(
        key: String,
        value: Double,
    ): Boolean = work.run { delegate.setDouble(key, value) }

    override suspend fun setEncodedStringList(
        key: String,
        value: String,
    ): Boolean = work.run { delegate.setEncodedStringList(key, value) }

    override suspend fun setDeprecatedStringList(
        key: String,
        value: List<String>,
    ): Boolean = work.run { delegate.setDeprecatedStringList(key, value) }

    override suspend fun clear(
        prefix: String,
        allowList: List<String>?,
    ): Boolean = work.run { delegate.clear(prefix, allowList) }

    override suspend fun getAll(
        prefix: String,
        allowList: List<String>?,
    ): Map<String, Any> = work.run { delegate.getAll(prefix, allowList) }
}

internal class TrackedPreferencesAsyncApi(
    private val delegate: SharedPreferencesAsyncApi,
    private val work: TrackedPreferencesWork,
) : SharedPreferencesAsyncApi {
    override suspend fun setBool(
        key: String,
        value: Boolean,
        options: SharedPreferencesPigeonOptions,
    ) = work.run { delegate.setBool(key, value, options) }

    override suspend fun setString(
        key: String,
        value: String,
        options: SharedPreferencesPigeonOptions,
    ) = work.run { delegate.setString(key, value, options) }

    override suspend fun setInt(
        key: String,
        value: Long,
        options: SharedPreferencesPigeonOptions,
    ) = work.run { delegate.setInt(key, value, options) }

    override suspend fun setDouble(
        key: String,
        value: Double,
        options: SharedPreferencesPigeonOptions,
    ) = work.run { delegate.setDouble(key, value, options) }

    override suspend fun setEncodedStringList(
        key: String,
        value: String,
        options: SharedPreferencesPigeonOptions,
    ) = work.run { delegate.setEncodedStringList(key, value, options) }

    override suspend fun setDeprecatedStringList(
        key: String,
        value: List<String>,
        options: SharedPreferencesPigeonOptions,
    ) = work.run { delegate.setDeprecatedStringList(key, value, options) }

    override suspend fun getString(
        key: String,
        options: SharedPreferencesPigeonOptions,
    ): String? = work.run { delegate.getString(key, options) }

    override suspend fun getBool(
        key: String,
        options: SharedPreferencesPigeonOptions,
    ): Boolean? = work.run { delegate.getBool(key, options) }

    override suspend fun getDouble(
        key: String,
        options: SharedPreferencesPigeonOptions,
    ): Double? = work.run { delegate.getDouble(key, options) }

    override suspend fun getInt(
        key: String,
        options: SharedPreferencesPigeonOptions,
    ): Long? = work.run { delegate.getInt(key, options) }

    override suspend fun getPlatformEncodedStringList(
        key: String,
        options: SharedPreferencesPigeonOptions,
    ): List<String>? = work.run { delegate.getPlatformEncodedStringList(key, options) }

    override suspend fun getStringList(
        key: String,
        options: SharedPreferencesPigeonOptions,
    ): StringListResult? = work.run { delegate.getStringList(key, options) }

    override suspend fun clear(
        allowList: List<String>?,
        options: SharedPreferencesPigeonOptions,
    ) = work.run { delegate.clear(allowList, options) }

    override suspend fun getAll(
        allowList: List<String>?,
        options: SharedPreferencesPigeonOptions,
    ): Map<String, Any> = work.run { delegate.getAll(allowList, options) }

    override suspend fun getKeys(
        allowList: List<String>?,
        options: SharedPreferencesPigeonOptions,
    ): List<String> = work.run { delegate.getKeys(allowList, options) }
}
