package com.agentplatform.mobile.data

import android.content.Context
import android.security.keystore.KeyGenParameterSpec
import android.security.keystore.KeyProperties
import android.util.Base64
import java.nio.charset.StandardCharsets
import java.security.KeyStore
import javax.crypto.Cipher
import javax.crypto.KeyGenerator
import javax.crypto.SecretKey
import javax.crypto.spec.GCMParameterSpec
import org.json.JSONObject

/**
 * Keeps the native client's deployment credential encrypted with a
 * non-exportable Android Keystore key. Nothing is written to logs or backups.
 */
class SecureConfigStore(context: Context) {
    private val preferences =
        context.getSharedPreferences("agent_platform_secure_config", Context.MODE_PRIVATE)

    fun load(): AppConfig? {
        val iv = preferences.getString(KEY_IV, null) ?: return null
        val ciphertext = preferences.getString(KEY_CIPHERTEXT, null) ?: return null
        return runCatching {
                val cipher = Cipher.getInstance(TRANSFORMATION)
                cipher.init(
                    Cipher.DECRYPT_MODE,
                    getOrCreateKey(),
                    GCMParameterSpec(128, Base64.decode(iv, Base64.NO_WRAP)),
                )
                val json =
                    JSONObject(
                        String(
                            cipher.doFinal(Base64.decode(ciphertext, Base64.NO_WRAP)),
                            StandardCharsets.UTF_8,
                        ),
                    )
                AppConfig(
                    gatewayBase = json.getString("gateway_base"),
                    accessToken = json.optString("access_token"),
                    developmentUserId = json.optString("development_user_id"),
                )
            }
            .getOrElse {
                clear()
                null
            }
    }

    fun save(config: AppConfig) {
        val normalized = config.normalized()
        check(normalized.validationError() == null) { "invalid app configuration" }
        val plaintext =
            JSONObject()
                .put("gateway_base", normalized.gatewayBase)
                .put("access_token", normalized.accessToken)
                .put("development_user_id", normalized.developmentUserId)
                .toString()
                .toByteArray(StandardCharsets.UTF_8)
        val cipher = Cipher.getInstance(TRANSFORMATION)
        cipher.init(Cipher.ENCRYPT_MODE, getOrCreateKey())
        val ciphertext = cipher.doFinal(plaintext)
        preferences
            .edit()
            .putString(KEY_IV, Base64.encodeToString(cipher.iv, Base64.NO_WRAP))
            .putString(
                KEY_CIPHERTEXT,
                Base64.encodeToString(ciphertext, Base64.NO_WRAP),
            )
            .apply()
    }

    fun clear() {
        preferences.edit().clear().apply()
    }

    private fun getOrCreateKey(): SecretKey {
        val keyStore = KeyStore.getInstance(KEYSTORE_PROVIDER).apply { load(null) }
        val existing = keyStore.getKey(KEY_ALIAS, null)
        if (existing is SecretKey) {
            return existing
        }
        val generator =
            KeyGenerator.getInstance(KeyProperties.KEY_ALGORITHM_AES, KEYSTORE_PROVIDER)
        generator.init(
            KeyGenParameterSpec.Builder(
                    KEY_ALIAS,
                    KeyProperties.PURPOSE_ENCRYPT or KeyProperties.PURPOSE_DECRYPT,
                )
                .setBlockModes(KeyProperties.BLOCK_MODE_GCM)
                .setEncryptionPaddings(KeyProperties.ENCRYPTION_PADDING_NONE)
                .setKeySize(256)
                .build(),
        )
        return generator.generateKey()
    }

    private companion object {
        const val KEYSTORE_PROVIDER = "AndroidKeyStore"
        const val KEY_ALIAS = "agent_platform_gateway_v1"
        const val TRANSFORMATION = "AES/GCM/NoPadding"
        const val KEY_IV = "iv"
        const val KEY_CIPHERTEXT = "ciphertext"
    }
}
