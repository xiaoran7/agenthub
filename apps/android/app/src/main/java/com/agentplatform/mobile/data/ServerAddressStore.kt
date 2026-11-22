package com.agentplatform.mobile.data

import android.content.Context

/**
 * Persists the gateway base URL and last-used email address in plain SharedPreferences.
 *
 * Unlike [SecureConfigStore] which is encrypted with Android Keystore and wiped on
 * logout, this store preserves the non-sensitive server address and user login identity
 * across app launches, credential expirations, and user logouts.
 */
class ServerAddressStore(context: Context) {
    private val preferences =
        context.getSharedPreferences(STORE_NAME, Context.MODE_PRIVATE)

    fun loadGatewayBase(): String =
        preferences.getString(KEY_GATEWAY_BASE, "").orEmpty()

    fun saveGatewayBase(base: String) {
        preferences.edit().putString(KEY_GATEWAY_BASE, base.trim()).apply()
    }

    fun loadEmail(): String =
        preferences.getString(KEY_EMAIL, "").orEmpty()

    fun saveEmail(email: String) {
        preferences.edit().putString(KEY_EMAIL, email.trim()).apply()
    }

    fun loadDevUser(): String =
        preferences.getString(KEY_DEV_USER, "").orEmpty()

    fun saveDevUser(devUser: String) {
        preferences.edit().putString(KEY_DEV_USER, devUser.trim()).apply()
    }

    private companion object {
        const val STORE_NAME = "agent_platform_server_memory"
        const val KEY_GATEWAY_BASE = "last_gateway_base"
        const val KEY_EMAIL = "last_email"
        const val KEY_DEV_USER = "last_dev_user"
    }
}
