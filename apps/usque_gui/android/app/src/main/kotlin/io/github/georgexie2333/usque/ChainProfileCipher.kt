package io.github.georgexie2333.usque

import android.security.keystore.KeyGenParameterSpec
import android.security.keystore.KeyProperties
import androidx.annotation.Keep
import java.security.KeyStore
import javax.crypto.Cipher
import javax.crypto.KeyGenerator
import javax.crypto.SecretKey
import javax.crypto.spec.GCMParameterSpec

/** Ciphertext IO and atomic updates belong to Rust in the VPN process. */
@Keep
internal object ChainProfileCipher {
    private const val ALIAS = "usque.chain-profiles.v1"

    @Synchronized
    private fun key(): SecretKey {
        val store = KeyStore.getInstance("AndroidKeyStore").apply { load(null) }
        (store.getKey(ALIAS, null) as? SecretKey)?.let { return it }
        return KeyGenerator
            .getInstance(KeyProperties.KEY_ALGORITHM_AES, "AndroidKeyStore")
            .apply {
                init(
                    KeyGenParameterSpec
                        .Builder(ALIAS, KeyProperties.PURPOSE_ENCRYPT or KeyProperties.PURPOSE_DECRYPT)
                        .setBlockModes(KeyProperties.BLOCK_MODE_GCM)
                        .setEncryptionPaddings(KeyProperties.ENCRYPTION_PADDING_NONE)
                        .setKeySize(256)
                        .build(),
                )
            }.generateKey()
    }

    fun seal(
        id: String,
        value: ByteArray,
    ): ByteArray {
        ChainProfileCipherCodec.validatePlaintext(value)
        return ChainProfileCipherCodec.seal(id, value, key())
    }

    fun open(
        id: String,
        value: ByteArray,
    ): ByteArray {
        ChainProfileCipherCodec.validateCiphertext(value)
        return ChainProfileCipherCodec.open(id, value, key())
    }
}

/** Platform codec limits mirror usque_core::proton_vpn::store's record bounds.
 * Rust retains each format's narrower limits, including ordinary chain profiles.
 */
internal object ChainProfileCipherCodec {
    const val MAX_RECORD_PLAINTEXT_BYTES = 8 * 1024 * 1024
    const val MAX_RECORD_CIPHERTEXT_BYTES = 12 * 1024 * 1024

    fun validatePlaintext(value: ByteArray) {
        require(value.size in 1..MAX_RECORD_PLAINTEXT_BYTES)
    }

    fun validateCiphertext(value: ByteArray) {
        require(value.size in 30..MAX_RECORD_CIPHERTEXT_BYTES && value[0] == 1.toByte())
    }

    fun seal(
        id: String,
        value: ByteArray,
        key: SecretKey,
    ): ByteArray {
        validatePlaintext(value)
        val cipher = Cipher.getInstance("AES/GCM/NoPadding")
        cipher.init(Cipher.ENCRYPT_MODE, key)
        cipher.updateAAD("Usque/chain-profiles/v1/$id".toByteArray(Charsets.UTF_8))
        val result = cipher.doFinal(value)
        return try {
            byteArrayOf(1) + cipher.iv + result
        } finally {
            result.fill(0)
        }
    }

    fun open(
        id: String,
        value: ByteArray,
        key: SecretKey,
    ): ByteArray {
        validateCiphertext(value)
        val cipher = Cipher.getInstance("AES/GCM/NoPadding")
        cipher.init(Cipher.DECRYPT_MODE, key, GCMParameterSpec(128, value, 1, 12))
        cipher.updateAAD("Usque/chain-profiles/v1/$id".toByteArray(Charsets.UTF_8))
        val result = cipher.doFinal(value, 13, value.size - 13)
        return try {
            validatePlaintext(result)
            result
        } catch (error: IllegalArgumentException) {
            result.fill(0)
            throw error
        }
    }
}
