package com.agentplatform.mobile.data

import org.json.JSONArray
import org.json.JSONObject

internal fun JSONArray?.objects(): List<JSONObject> =
    if (this == null) emptyList() else (0 until length()).mapNotNull { optJSONObject(it) }

internal fun JSONArray?.strings(): List<String> =
    if (this == null) emptyList() else (0 until length()).mapNotNull { i ->
        if (isNull(i)) null else optString(i).takeIf(String::isNotEmpty)
    }

internal fun JSONObject.optNullableString(key: String): String? {
    if (isNull(key)) return null
    return optString(key).takeIf { it.isNotBlank() }
}

internal fun JSONObject.optStringOrEmpty(key: String): String =
    if (isNull(key)) "" else optString(key, "")

internal fun JSONObject.optLongOrNull(key: String): Long? {
    if (isNull(key) || !has(key)) return null
    val value = optLong(key, Long.MIN_VALUE)
    return if (value == Long.MIN_VALUE) null else value
}

internal fun JSONObject.optStringMap(key: String): Map<String, String> {
    val obj = optJSONObject(key) ?: return emptyMap()
    val result = LinkedHashMap<String, String>()
    val iter = obj.keys()
    while (iter.hasNext()) {
        val k = iter.next()
        if (k.isBlank() || obj.isNull(k)) continue
        val v = obj.optString(k, "").trim()
        if (v.isNotEmpty()) result[k] = v
    }
    return result
}
