package io.github.georgexie2333.usque

import android.content.Context
import android.security.keystore.KeyGenParameterSpec
import android.security.keystore.KeyProperties
import android.util.AtomicFile
import android.util.Base64
import java.io.File
import java.nio.ByteBuffer
import java.security.MessageDigest
import java.security.KeyStore
import javax.crypto.Cipher
import javax.crypto.KeyGenerator
import javax.crypto.SecretKey
import javax.crypto.spec.GCMParameterSpec

/**
 * Encrypted storage for identity material. Ciphertexts live in private app
 * preferences while the non-exportable AES key remains in Android Keystore.
 *
 * Operational access is intentionally not authentication-gated: the dedicated
 * VPN process must reconnect in the background. A future reveal/copy UI must
 * independently require BiometricPrompt or device credentials.
 */
internal class SecureIdentityStore(context: Context) {
    internal enum class Record(val key: String) {
        WARP_SECRET("warp-secret"),
        MASQUE_PRIVATE_KEY("masque-private-key"),
        ACCESS_TOKEN("access-token"),
        DEVICE_ID("device-id"),
        LICENSE("license"),
        ENDPOINT_PIN("endpoint-pin"),
    }

    private val legacyPreferences =
        context.applicationContext.getSharedPreferences(PREFERENCES_NAME, Context.MODE_PRIVATE)
    private val identityDirectory =
        File(context.applicationContext.noBackupFilesDir, IDENTITY_DIRECTORY).apply {
            check(isDirectory || mkdirs()) { "Encrypted identity directory could not be created" }
        }

    fun put(profileId: String, record: Record, value: ByteArray) {
        require(validProfileId(profileId)) { "Invalid profile ID" }
        require(value.isNotEmpty() && value.size <= MAX_SECRET_BYTES) {
            "Secret size is outside the allowed range"
        }

        val target = target(profileId, record)
        val cipher = Cipher.getInstance(TRANSFORMATION)
        cipher.init(Cipher.ENCRYPT_MODE, key())
        cipher.updateAAD(target.toByteArray(Charsets.UTF_8))
        val ciphertext = cipher.doFinal(value)
        try {
            val encoded =
                ByteBuffer
                    .allocate(2 + cipher.iv.size + ciphertext.size)
                    .put(FORMAT_VERSION)
                    .put(cipher.iv.size.toByte())
                    .put(cipher.iv)
                    .put(ciphertext)
                    .array()
            try {
                val atomic = AtomicFile(recordFile(target))
                val output = atomic.startWrite()
                try {
                    output.write(encoded)
                    atomic.finishWrite(output)
                } catch (error: Exception) {
                    atomic.failWrite(output)
                    throw error
                }
                legacyPreferences.edit().remove(target).commit()
            } finally {
                encoded.fill(0)
            }
        } finally {
            ciphertext.fill(0)
        }
    }

    /**
     * The caller owns the returned plaintext and must overwrite it immediately
     * after transferring it to the Rust engine.
     */
    fun get(profileId: String, record: Record): ByteArray? {
        require(validProfileId(profileId)) { "Invalid profile ID" }
        val target = target(profileId, record)
        val file = recordFile(target)
        val legacy = !file.isFile
        val encoded =
            if (legacy) {
                val encodedText = legacyPreferences.getString(target, null) ?: return null
                Base64.decode(encodedText, Base64.NO_WRAP)
            } else {
                AtomicFile(file).readFully()
            }
        try {
            require(encoded.size >= 2 + GCM_IV_BYTES + GCM_TAG_BYTES) {
                "Encrypted identity record is truncated"
            }
            val buffer = ByteBuffer.wrap(encoded)
            require(buffer.get() == FORMAT_VERSION) { "Unsupported identity record version" }
            val ivLength = buffer.get().toInt() and 0xff
            require(ivLength == GCM_IV_BYTES && buffer.remaining() > GCM_TAG_BYTES) {
                "Encrypted identity record has an invalid IV"
            }
            val iv = ByteArray(ivLength)
            buffer.get(iv)
            val ciphertext = ByteArray(buffer.remaining())
            buffer.get(ciphertext)
            val plaintext =
                try {
                    val cipher = Cipher.getInstance(TRANSFORMATION)
                    cipher.init(Cipher.DECRYPT_MODE, key(), GCMParameterSpec(GCM_TAG_BITS, iv))
                    cipher.updateAAD(target.toByteArray(Charsets.UTF_8))
                    cipher.doFinal(ciphertext)
                } finally {
                    iv.fill(0)
                    ciphertext.fill(0)
                }
            if (legacy) {
                try {
                    put(profileId, record, plaintext)
                } catch (error: Exception) {
                    plaintext.fill(0)
                    throw error
                }
            }
            return plaintext
        } finally {
            encoded.fill(0)
        }
    }

    fun delete(profileId: String, record: Record) {
        require(validProfileId(profileId)) { "Invalid profile ID" }
        val target = target(profileId, record)
        AtomicFile(recordFile(target)).delete()
        legacyPreferences.edit().remove(target).commit()
    }

    fun deleteIdentity(profileId: String) {
        require(validProfileId(profileId)) { "Invalid profile ID" }
        val editor = legacyPreferences.edit()
        Record.entries.forEach { record ->
            val target = target(profileId, record)
            AtomicFile(recordFile(target)).delete()
            editor.remove(target)
        }
        editor.commit()
    }

    fun clearAll() {
        identityDirectory.listFiles()?.forEach { file ->
            if (file.isFile) AtomicFile(file).delete()
        }
        check(legacyPreferences.edit().clear().commit()) {
            "Encrypted identity preferences could not be cleared"
        }
        val keyStore = KeyStore.getInstance(ANDROID_KEYSTORE).apply { load(null) }
        if (keyStore.containsAlias(KEY_ALIAS)) {
            keyStore.deleteEntry(KEY_ALIAS)
        }
    }

    private fun key(): SecretKey {
        val keyStore = KeyStore.getInstance(ANDROID_KEYSTORE).apply { load(null) }
        (keyStore.getKey(KEY_ALIAS, null) as? SecretKey)?.let { return it }

        val generator =
            KeyGenerator.getInstance(KeyProperties.KEY_ALGORITHM_AES, ANDROID_KEYSTORE)
        generator.init(
            KeyGenParameterSpec
                .Builder(
                    KEY_ALIAS,
                    KeyProperties.PURPOSE_ENCRYPT or KeyProperties.PURPOSE_DECRYPT,
                )
                .setKeySize(256)
                .setBlockModes(KeyProperties.BLOCK_MODE_GCM)
                .setEncryptionPaddings(KeyProperties.ENCRYPTION_PADDING_NONE)
                .setRandomizedEncryptionRequired(true)
                .setUserAuthenticationRequired(false)
                .build(),
        )
        return generator.generateKey()
    }

    private fun target(profileId: String, record: Record): String =
        "$TARGET_PREFIX/$profileId/${record.key}"

    private fun recordFile(target: String): File {
        val digest = MessageDigest.getInstance("SHA-256").digest(target.toByteArray(Charsets.UTF_8))
        val name =
            digest.joinToString(separator = "") { byte ->
                "%02x".format(byte.toInt() and 0xff)
            }
        digest.fill(0)
        return File(identityDirectory, "$name.bin")
    }

    private fun validProfileId(value: String): Boolean =
        value.length in 1..64 &&
            value.all { character ->
                character.isLetterOrDigit() || character == '-' || character == '_' || character == '.'
            }

    private companion object {
        const val ANDROID_KEYSTORE = "AndroidKeyStore"
        const val KEY_ALIAS = "io.github.georgexie2333.usque.identity.v1"
        const val PREFERENCES_NAME = "usque_identity_v1"
        const val IDENTITY_DIRECTORY = "usque_identity_v1"
        const val TARGET_PREFIX = "identity"
        const val TRANSFORMATION = "AES/GCM/NoPadding"
        const val MAX_SECRET_BYTES = 128 * 1024
        const val GCM_IV_BYTES = 12
        const val GCM_TAG_BITS = 128
        const val GCM_TAG_BYTES = GCM_TAG_BITS / 8
        const val FORMAT_VERSION: Byte = 1
    }
}
