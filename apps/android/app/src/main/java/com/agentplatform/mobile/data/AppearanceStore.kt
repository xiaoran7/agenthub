package com.agentplatform.mobile.data

import android.content.Context

/**
 * Where the appearance choice lives.
 *
 * Deliberately not [SecureConfigStore]: that store exists to keep a credential
 * behind a Keystore key, and putting a colour preference in it would mean a
 * palette change rewrites the encrypted deployment blob. A preference has no
 * secret to protect, so plain shared preferences are the right home — and a
 * wiped Keystore no longer costs the user their chosen theme.
 */
class AppearanceStore(context: Context) {
    private val preferences = context.getSharedPreferences(STORE_NAME, Context.MODE_PRIVATE)

    fun load(): AppearanceTheme =
        AppearanceTheme.fromWire(preferences.getString(KEY_THEME, AppearanceTheme.SYSTEM.wire).orEmpty())

    fun save(theme: AppearanceTheme) {
        preferences.edit().putString(KEY_THEME, theme.wire).apply()
    }

    private companion object {
        const val STORE_NAME = "agent_platform_appearance"
        const val KEY_THEME = "appearance"
    }
}
