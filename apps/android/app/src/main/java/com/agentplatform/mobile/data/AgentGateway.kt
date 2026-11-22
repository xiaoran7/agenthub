package com.agentplatform.mobile.data

import java.io.BufferedReader
import java.io.IOException
import java.io.InputStreamReader
import java.net.HttpURLConnection
import java.net.URI
import java.net.URLEncoder
import java.nio.charset.StandardCharsets
import java.util.zip.GZIPInputStream
import org.json.JSONArray
import org.json.JSONException
import org.json.JSONObject

/** A Gateway / account refusal, carrying the machine-readable reason. */
class GatewayException(
    val status: Int,
    val code: String,
) : IOException("Gateway request failed: $status $code")

/**
 * The one HTTP client the phone has.
 *
 * Every call goes through [execute] so identity, timeouts, redirect refusal
 * and error decoding cannot drift between endpoints. Credentials are attached
 * per request shape: bearer token and account cookie are sent as-is, while the
 * development header is only ever added for a loopback host — the Gateway
 * refuses development auth on anything else, and sending it to a remote host
 * would leak a user-chosen identifier to a production endpoint.
 */
class AgentGateway(
    config: AppConfig,
) {
    /** Trimmed once and checked for safe transport scheme before use. */
    private val normalized =
        config.normalized().also { cfg ->
            val err = cfg.gatewayError()
            require(err == null) { err ?: "invalid gateway base" }
        }

    // ---------------------------------------------------------------- remote

    fun fetchSnapshot(): Snapshot =
        Snapshot.parse(getJson("/v3/remote/sessions"))

    fun fetchSession(runnerId: String, hubId: String): SessionDetail =
        SessionDetail.parse(
            getJson("/v3/remote/sessions/${encode(runnerId)}/${encode(hubId)}"),
        )

    fun fetchRunnerHello(runnerId: String): HostDevice? =
        runCatching {
            HostDevice.parse(getJson("/v3/remote/runners/${encode(runnerId)}/hello"))
        }.getOrNull()

    fun fetchPluginsResponse(): RemotePluginsResponse =
        RemotePluginsResponse.parse(getJson("/v3/remote/plugins"))

    fun fetchPlugins(): List<AgentPlugin> =
        fetchPluginsResponse().plugins

    fun togglePlugin(pluginId: String, enabled: Boolean): AgentPlugin? {
        val reply =
            postJson(
                "/v3/remote/plugins/${encode(pluginId)}/toggle",
                JSONObject().put("enabled", enabled),
            )
        reply.optJSONObject("plugin")?.let { return AgentPlugin.parse(it) }
        return reply.optJSONArray("plugins").objects().mapNotNull(AgentPlugin::parse)
            .firstOrNull { it.id == pluginId }
    }

    fun performPluginAction(
        runnerId: String,
        action: String,
        pluginId: String? = null,
        enabled: Boolean? = null,
        registryUrl: String? = null,
    ): List<AgentPlugin> {
        val body = JSONObject().put("action", action)
        if (!pluginId.isNullOrBlank()) body.put("plugin_id", pluginId)
        if (enabled != null) body.put("enabled", enabled)
        if (!registryUrl.isNullOrBlank()) body.put("registry_url", registryUrl)
        val reply =
            postJson(
                "/v3/remote/runners/${encode(runnerId)}/plugins/action",
                body,
            )
        return reply.optJSONArray("plugins").objects().mapNotNull(AgentPlugin::parse)
    }

    fun createDeviceBinding(): DeviceBinding =
        DeviceBinding.parse(postJson("/v1/device-bindings", JSONObject()))
            ?: throw GatewayException(500, "invalid_response")

    fun revokeDevice(deviceId: String) {
        execute("/v1/devices/${encode(deviceId)}", "DELETE", null, Credentials.All)
    }

    /**
     * Reads the session's event log after `after`.
     *
     * A long session may answer with a full page; the caller keeps pulling
     * until a short page arrives, which is how a reopened conversation is
     * seeded without betting on one arbitrary limit.
     */
    fun fetchEvents(
        runnerId: String,
        hubId: String,
        after: Long,
        limit: Int = 256,
    ): List<RemoteEvent> =
        getJson(
            "/v3/remote/events?runner=${encode(runnerId)}&hub=${encode(hubId)}" +
                "&after=$after&limit=${limit.coerceIn(1, 1000)}",
        ).optJSONArray("events").objects().map(RemoteEvent::parse)

    /**
     * Submits one command.
     *
     * `clientCmdId` is the idempotency key: the Gateway persists it before
     * answering 202, so a retry after a dropped response returns the original
     * receipt instead of running the action twice. A caller must reuse the id
     * for a retry of the same action and mint a new one for a new action.
     */
    fun submitCommand(
        runnerId: String,
        hubSessionId: String,
        hostGeneration: String?,
        clientCmdId: String,
        command: JSONObject,
    ): CommandReceipt {
        val body =
            JSONObject()
                .put("client_cmd_id", clientCmdId)
                .put("hub_session_id", hubSessionId)
                .put("host_generation", hostGeneration ?: JSONObject.NULL)
                .put("command", command)
        val reply =
            postJson(
                "/v3/remote/runners/${encode(runnerId)}/commands",
                body,
            )
        return CommandReceipt(
            status = reply.optString("status").ifBlank { "queued" },
            seq = reply.optLong("seq", 0),
            clientCmdId = reply.optString("client_cmd_id").ifBlank { clientCmdId },
        )
    }

    // --------------------------------------------------------------- account

    fun account(): AccountView = AccountView.parse(getJson("/v1/auth/me"))

    fun needsSetup(): Boolean = getJson("/v1/auth/status").optBoolean("needs_setup")

    /**
     * Signs in and returns the account together with the session cookie the
     * Gateway set.
     *
     * Android has no browser cookie jar, so the `Set-Cookie` header is parsed
     * here and persisted by the caller. Only the `name=value` pair is kept:
     * attributes such as `Secure`/`Max-Age` are the browser's business, and
     * the expiry is already enforced server-side.
     */
    fun login(email: String, password: String): LoginResult {
        val response =
            execute(
                path = "/v1/auth/login",
                method = "POST",
                body = JSONObject().put("email", email).put("password", password),
                credentials = Credentials.None,
            )
        val cookie =
            response.setCookies
                .mapNotNull(::sessionCookiePair)
                .firstOrNull()
                ?: throw GatewayException(response.status, "session_cookie_missing")
        return LoginResult(account = AccountView.parse(response.body), sessionCookie = cookie)
    }

    fun logout(): Unit {
        // A 401 here means the session was already gone, which is the same
        // outcome as a successful logout.
        runCatching { execute("/v1/auth/logout", "POST", null, Credentials.All) }
    }

    /**
     * Opens the `/v3/remote/stream?after=<after>` SSE feed and delivers each
     * parsed frame to [onFrame]. Calls [onOpen] with an immediate disconnect
     * handle so callers can tear down the blocking socket on coroutine cancel.
     */
    fun streamEvents(
        after: Long,
        onConnected: (Boolean) -> Unit,
        onFrame: (SseFrame) -> Unit,
        onOpen: ((() -> Unit) -> Unit)? = null,
    ) {
        val normalized = normalized
        val query = if (after > 0) "?after=$after" else ""
        val uri = URI("${normalized.gatewayBase}/v3/remote/stream$query")
        val connection = uri.toURL().openConnection() as HttpURLConnection
        onOpen?.invoke { runCatching { connection.disconnect() } }
        try {
            connection.requestMethod = "GET"
            connection.instanceFollowRedirects = false
            connection.connectTimeout = 10_000
            connection.readTimeout = 30_000
            connection.setRequestProperty("Accept", "text/event-stream")
            connection.setRequestProperty("Accept-Encoding", "identity")
            connection.setRequestProperty("Cache-Control", "no-store")
            attachCredentials(connection, uri, normalized)
            val status = connection.responseCode
            if (status !in 200..299) {
                val errText = readCappedText(connection.errorStream, MAX_ERROR_BYTES)
                throw GatewayException(status, errorCode(errText, status))
            }
            val parser = SseLineParser()
            // "Connected" means bytes are arriving, not that the response head
            // arrived. A proxy that buffers the whole stream hands back a 200
            // and then sends nothing; reporting that as live made the poll
            // loop fall back to its 15s keepalive cadence and the screen went
            // quiet for the length of the turn. The first line — a heartbeat
            // comment counts, the Gateway writes one every 10s — is what
            // proves the stream is actually feeding this client.
            BufferedReader(InputStreamReader(connection.inputStream, StandardCharsets.UTF_8)).use { reader ->
                var firstLine = true
                while (true) {
                    val line = reader.readLine() ?: break
                    if (firstLine) {
                        firstLine = false
                        onConnected(true)
                    }
                    parser.feedLine(line)?.let(onFrame)
                }
            }
        } finally {
            onConnected(false)
            connection.disconnect()
        }
    }

    // ----------------------------------------------------------------- lower

    private enum class Credentials {
        /** No identity header at all: only the login endpoint. */
        None,

        /** Everything this configuration can prove, including the dev header. */
        All,
    }

    private class Response(
        val status: Int,
        val body: JSONObject,
        val setCookies: List<String>,
    )

    private fun getJson(path: String): JSONObject = execute(path, "GET", null, Credentials.All).body

    private fun postJson(path: String, body: JSONObject): JSONObject =
        execute(path, "POST", body, Credentials.All).body

    private fun execute(
        path: String,
        method: String,
        body: JSONObject?,
        credentials: Credentials,
    ): Response {
        require(path.startsWith("/") && !path.contains("..") && !path.contains('#')) {
            "unsafe request path"
        }
        val normalized = normalized
        val uri = URI(normalized.gatewayBase + path)
        val connection = uri.toURL().openConnection() as HttpURLConnection
        return try {
            connection.requestMethod = method
            connection.instanceFollowRedirects = false
            connection.connectTimeout = 10_000
            connection.readTimeout = 25_000
            connection.setRequestProperty("Accept", "application/json")
            connection.setRequestProperty("Accept-Encoding", "gzip")
            if (credentials == Credentials.All) attachCredentials(connection, uri, normalized)

            if (body != null) {
                connection.doOutput = true
                connection.setRequestProperty("Content-Type", "application/json; charset=utf-8")
                connection.outputStream.use {
                    it.write(body.toString().toByteArray(StandardCharsets.UTF_8))
                }
            }
            val status = connection.responseCode
            val rawStream = if (status in 200..299) connection.inputStream else connection.errorStream
            val stream =
                if (
                    rawStream != null &&
                    connection.contentEncoding?.equals("gzip", ignoreCase = true) == true
                ) {
                    GZIPInputStream(rawStream)
                } else {
                    rawStream
                }
            val text = readCappedText(stream, MAX_RESPONSE_BYTES)
            if (status !in 200..299) {
                throw GatewayException(status, errorCode(text, status))
            }
            val parsed =
                try {
                    if (text.isBlank()) JSONObject() else JSONObject(text)
                } catch (_: JSONException) {
                    throw GatewayException(status, "invalid_response")
                }
            Response(status, parsed, connection.headerFields["Set-Cookie"].orEmpty())
        } catch (failure: Throwable) {
            // Tear down only a connection that will not be reused. Calling
            // `disconnect()` after a completed read drops the socket instead of
            // returning it to the pool, so every poll paid a fresh TCP+TLS
            // handshake.
            connection.disconnect()
            throw failure
        }
    }

    private fun readCappedText(stream: java.io.InputStream?, maxBytes: Int): String {
        if (stream == null) return ""
        return stream.use { input ->
            val out = java.io.ByteArrayOutputStream()
            val buf = ByteArray(16 * 1024)
            var total = 0
            while (true) {
                val n = input.read(buf)
                if (n < 0) break
                total += n
                if (total > maxBytes) {
                    throw GatewayException(413, "invalid_response")
                }
                out.write(buf, 0, n)
            }
            String(out.toByteArray(), StandardCharsets.UTF_8)
        }
    }

    private fun attachCredentials(
        connection: HttpURLConnection,
        uri: URI,
        normalized: AppConfig,
    ) {
        if (normalized.sessionCookie.isNotBlank()) {
            val secureCookie = normalized.sessionCookie.startsWith(SECURE_COOKIE)
            if (!secureCookie || uri.scheme == "https") {
                connection.setRequestProperty("Cookie", normalized.sessionCookie)
            }
        }
        if (normalized.accessToken.isNotBlank()) {
            connection.setRequestProperty("Authorization", "Bearer ${normalized.accessToken}")
        }
        val host = uri.host?.lowercase()
        val loopback = AppConfig.isLoopbackUriHost(host)
        if (loopback && normalized.developmentUserId.isNotBlank()) {
            connection.setRequestProperty("x-user-id", normalized.developmentUserId)
        }
    }

    private fun errorCode(text: String, status: Int): String {
        // The Gateway answers `{"error": code}`; `code` is accepted too so a
        // proxy that rewrites the envelope still yields a usable reason. The
        // fallback parses the body, not a fresh object — reading an empty one
        // always returned "" and made this branch unreachable.
        val parsed = runCatching { JSONObject(text) }.getOrNull()
        val code =
            parsed?.optString("error").orEmpty().ifBlank { parsed?.optString("code").orEmpty() }
        if (code.isNotBlank()) return code
        return when (status) {
            401, 403 -> "authentication_required"
            404 -> "not_found"
            in 500..599 -> "gateway_unavailable"
            else -> "http_$status"
        }
    }

    private fun encode(value: String): String =
        URLEncoder.encode(value, "UTF-8")
            .replace("+", "%20")
            .replace("%7E", "~")

    companion object {
        private const val MAX_RESPONSE_BYTES = 16 * 1024 * 1024
        private const val MAX_ERROR_BYTES = 256 * 1024
        /**
         * Extracts the `name=value` pair from one `Set-Cookie` value.
         *
         * Both the local and the `__Host-` prefixed cookie name are accepted,
         * because the Gateway picks the name by whether it thinks the
         * connection is secure.
         */
        fun sessionCookiePair(setCookie: String?): String? {
            if (setCookie == null) return null
            val pair = setCookie.substringBefore(';').trim()
            val name = pair.substringBefore('=', "").trim()
            if (name != LOCAL_COOKIE && name != SECURE_COOKIE) return null
            val value = pair.substringAfter('=', "").trim()
            if (!value.matches(Regex("[A-Za-z0-9_-]{43}"))) return null
            return pair
        }

        const val LOCAL_COOKIE = "agent_pulse_session"
        const val SECURE_COOKIE = "__Host-agent_pulse_session"
    }
}

data class LoginResult(
    val account: AccountView,
    val sessionCookie: String,
)

/**
 * Turns any failure into one sentence a person can act on.
 *
 * The Gateway's own codes are the interesting ones: they describe a state the
 * user can change (offline host, lost control, expired approval). A bare HTTP
 * status is only a fallback, because "400" tells nobody what to do.
 */
fun describeGatewayError(error: Throwable): String =
    when (error) {
        is GatewayException ->
            when (error.code) {
                "authentication_required" -> "登录已失效，请重新登录。"
                "device_not_found", "runner_not_found" -> "这台电脑已被撤销或未注册。"
                "runner_offline" -> "电脑 Runner 离线，恢复连线后可继续。"
                "session_control_lost", "host_generation_mismatch" ->
                    "会话控制已断开，请点重新连接继续接管。"
                "session_not_found", "claude_session_not_found", "acp_session_not_found" ->
                    "目标会话已不存在或无法恢复。"
                "approval_not_pending" -> "该审批已处理或已过期。"
                "question_not_pending" -> "该提问已回答或已过期。"
                "runner_backpressure" -> "电脑端繁忙，请稍后重试。"
                "workspace_not_ready", "workspace_revoked", "workspace_unknown" ->
                    "该工作区当前不可用。"
                "plugin_disabled" -> "对应扩展插件未开启，请先在设置中启用该插件。"
                "zcode_not_available", "provider_not_available" ->
                    "电脑端未安装或未启动对应智能体 CLI。"
                "runner_upgrade_required" -> "请先更新这台电脑上的 AgentHub Runner。"
                "native_writer_busy" ->
                    "Codex 桌面端或其他应用仍在使用这条会话，请在那里关闭后重试。"
                "explicit_control_required" ->
                    "请主动接管会话后继续。"
                "session_not_started" ->
                    "请先发送一条消息，保存会话后再归还控制。"
                "session_control_releasing" ->
                    "正在归还会话控制，请稍后重试。"
                "external_session_not_resumable", "attach_not_supported" ->
                    "该外部会话暂不支持远程接管。"
                "invalid_model" -> "所选模型在当前引擎不可用，请切换模型。"
                "invalid_permission_mode" -> "当前引擎不支持该权限模式。"
                "invalid_effort" -> "当前模型不支持该思考强度。"
                "invalid_field" -> "指令参数不符合契约校验，请检查输入。"
                "device_limit_reached" -> "当前工作区绑定设备数已达上限。"
                "binding_code_expired", "binding_code_used" -> "绑定码已失效，请重新生成。"
                "invalid_cursor" -> "读取游标失效，正在重新同步。"
                "stream_limit" -> "同时打开的会话过多，请稍后重试。"
                "session_cookie_missing" -> "Gateway 未返回登录凭据，请检查部署配置。"
                "invalid_response" -> "Gateway 返回了无法解析的数据。"
                "gateway_unavailable" -> "Gateway 暂时不可用，请稍后重试。"
                "email_verification_required" -> "请先完成邮箱验证再登录。"
                "invalid_credentials" -> "邮箱或密码不正确。"
                "login_rate_limited" -> "登录尝试过于频繁，请稍后再试。"
                "invalid_email" -> "邮箱格式无效。"
                "invalid_password" -> "密码不符合要求。"
                else ->
                    if (error.status in 500..599) "Gateway 暂时不可用，请稍后重试。"
                    else "Gateway 返回错误（${error.status} ${error.code}）。"
            }
        is IOException -> "无法连接 Gateway，请检查地址与网络。"
        else -> "读取数据时发生未知错误。"
    }

data class SseFrame(
    val event: String,
    val cursor: Long,
    val runnerId: String?,
)

/**
 * Line-oriented SSE frame decoder matching the Gateway's `/v3/remote/stream`
 * wire format (`id:`, `event:`, `data:` + blank-line boundary).
 */
internal class SseLineParser {
    private var eventName = "message"
    private var frameId = 0L
    private val dataBuilder = StringBuilder()
    private var hasField = false
    private var overflowed = false

    fun feedLine(line: String): SseFrame? {
        if (line.isEmpty()) {
            val wasOverflow = overflowed
            val hadField = hasField
            val name = eventName
            val id = frameId
            val dataLen = dataBuilder.length
            val rawData = if (!wasOverflow && name == "remote" && dataLen in 1 until 4096) {
                dataBuilder.toString()
            } else {
                null
            }
            eventName = "message"
            frameId = 0L
            dataBuilder.setLength(0)
            if (dataBuilder.capacity() > 64 * 1024) {
                dataBuilder.trimToSize()
            }
            hasField = false
            overflowed = false
            if (!hadField || wasOverflow) return null
            val runnerId = rawData?.let {
                runCatching { JSONObject(it).optNullableString("runner_id") }.getOrNull()
            }
            return SseFrame(event = name, cursor = id, runnerId = runnerId)
        }
        if (line.startsWith(':')) return null
        if (line.length > MAX_SSE_LINE_CHARS) {
            overflowed = true
            return null
        }
        val sep = line.indexOf(':')
        val field = if (sep < 0) line else line.substring(0, sep)
        val rawValue = if (sep < 0) "" else line.substring(sep + 1)
        val value = if (rawValue.startsWith(' ')) rawValue.substring(1) else rawValue
        when (field) {
            "event" -> {
                eventName = value
                hasField = true
            }
            "id" -> {
                frameId = value.toLongOrNull() ?: frameId
            }
            "data" -> {
                val added = value.length + if (dataBuilder.isNotEmpty()) 1 else 0
                if (dataBuilder.length + added > MAX_SSE_DATA_CHARS) {
                    overflowed = true
                    dataBuilder.setLength(0)
                } else if (!overflowed) {
                    if (dataBuilder.isNotEmpty()) dataBuilder.append('\n')
                    dataBuilder.append(value)
                }
                hasField = true
            }
        }
        return null
    }

    private companion object {
        const val MAX_SSE_LINE_CHARS = 64 * 1024
        const val MAX_SSE_DATA_CHARS = 256 * 1024
    }
}
