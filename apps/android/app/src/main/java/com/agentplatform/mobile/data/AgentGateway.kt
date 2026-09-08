package com.agentplatform.mobile.data

import java.io.IOException
import java.net.HttpURLConnection
import java.net.URI
import java.net.URLEncoder
import java.nio.charset.StandardCharsets
import java.time.Instant
import org.json.JSONArray
import org.json.JSONObject

class GatewayException(
    val status: Int,
    val code: String,
) : IOException("Gateway request failed: $status $code")

class AgentGateway(
    private val config: AppConfig,
) {
    fun fetchSnapshot(): GatewaySnapshot =
        GatewaySnapshot(
            devices = parseDevices(getJson("/v1/devices").getJSONArray("devices")),
            sessions = parseSessions(getJson("/v1/sessions").getJSONArray("sessions")),
        )

    fun fetchTimeline(session: SessionRecord, limit: Int = 100): List<SessionRecord> {
        val query =
            listOf(
                    "device_id" to session.deviceId,
                    "agent_type" to session.agentType.wireValue,
                    "agent_session_id" to session.agentSessionId,
                    "limit" to limit.coerceIn(1, 500).toString(),
                )
                .joinToString("&") { (key, value) ->
                    "${encode(key)}=${encode(value)}"
                }
        return parseSessions(
            getJson("/v1/sessions/events?$query").getJSONArray("events"),
        )
    }

    fun remoteRequest(path: String, body: JSONObject? = null): JSONObject {
        require(path.startsWith("/") && !path.contains("..") && !path.contains('#'))
        return getJson("/v3/remote$path", body)
    }

    private fun getJson(path: String, body: JSONObject? = null): JSONObject {
        val base = config.normalized().gatewayBase
        val connection = URI("$base$path").toURL().openConnection() as HttpURLConnection
        return try {
            connection.requestMethod = if (body == null) "GET" else "POST"
            connection.instanceFollowRedirects = false
            connection.connectTimeout = 8_000
            connection.readTimeout = 12_000
            connection.setRequestProperty("Accept", "application/json")
            if (config.accessToken.isNotBlank()) {
                connection.setRequestProperty("Authorization", "Bearer ${config.accessToken}")
            }
            val host = URI(config.normalized().gatewayBase).host.lowercase()
            if (
                config.developmentUserId.isNotBlank() &&
                    (host == "127.0.0.1" || host == "localhost")
            ) {
                connection.setRequestProperty("x-user-id", config.developmentUserId)
            }

            if (body != null) {
                connection.doOutput = true
                connection.setRequestProperty("Content-Type", "application/json")
                connection.outputStream.use { it.write(body.toString().toByteArray(StandardCharsets.UTF_8)) }
            }
            val status = connection.responseCode
            if (status !in 200..299) {
                val code =
                    runCatching {
                            connection.errorStream
                                ?.bufferedReader(StandardCharsets.UTF_8)
                                ?.use { JSONObject(it.readText()).optString("error") }
                        }
                        .getOrNull()
                        .orEmpty()
                        .ifBlank { "http_error" }
                throw GatewayException(status, code)
            }
            connection.inputStream.bufferedReader(StandardCharsets.UTF_8).use {
                JSONObject(it.readText())
            }
        } finally {
            connection.disconnect()
        }
    }

    private fun parseDevices(values: JSONArray): List<DeviceRecord> =
        buildList(values.length()) {
            repeat(values.length()) { index ->
                val item = values.getJSONObject(index)
                add(
                    DeviceRecord(
                        deviceId = item.getString("device_id"),
                        createdAt = item.instant("created_at"),
                        lastSeenAt = item.instant("last_seen_at"),
                        online = item.getBoolean("online"),
                        revoked = item.getBoolean("revoked"),
                    ),
                )
            }
        }

    private fun parseSessions(values: JSONArray): List<SessionRecord> =
        buildList(values.length()) {
            repeat(values.length()) { index ->
                val item = values.getJSONObject(index)
                val payload = item.getJSONObject("payload")
                val project = payload.getJSONObject("project")
                val state = payload.getJSONObject("state")
                add(
                    SessionRecord(
                        eventId = item.getString("event_id"),
                        sequence = item.getLong("sequence"),
                        deviceId = item.getString("device_id"),
                        agentType = AgentType.fromWire(item.getString("agent_type")),
                        agentSessionId = item.getString("agent_session_id"),
                        observedAt = item.instant("observed_at"),
                        receivedAt = item.optInstant("received_at"),
                        project =
                            ProjectInfo(
                                displayName = project.getString("display_name"),
                                pathFingerprint = project.optNullableString("path_fingerprint"),
                            ),
                        state =
                            SessionState(
                                presence =
                                    Presence.fromWire(state.getString("presence")),
                                activityStatus =
                                    ActivityStatus.fromWire(
                                        state.getString("activity_status"),
                                    ),
                                waitingReason = state.optNullableString("waiting_reason"),
                            ),
                    ),
                )
            }
        }

    private fun JSONObject.instant(key: String): Instant = Instant.parse(getString(key))

    private fun JSONObject.optInstant(key: String): Instant? =
        optString(key).takeIf(String::isNotBlank)?.let(Instant::parse)

    private fun JSONObject.optNullableString(key: String): String? =
        if (isNull(key)) null else optString(key).takeIf(String::isNotBlank)

    private fun encode(value: String): String =
        URLEncoder.encode(value, StandardCharsets.UTF_8.toString())
}

fun describeGatewayError(error: Throwable): String =
    when (error) {
        is GatewayException ->
            when {
                error.status == 401 || error.status == 403 -> "Gateway 拒绝了当前身份"
                error.status >= 500 -> "Gateway 暂时不可用"
                else -> "Gateway 返回错误（${error.status} ${error.code}）"
            }
        is IOException -> "无法连接 Gateway，请检查地址与网络"
        else -> "读取数据时发生未知错误"
    }
