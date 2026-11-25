package com.agentplatform.mobile.data

import java.io.IOException
import java.net.InetAddress
import java.net.ServerSocket
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertThrows
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * Credential-parsing and error-text tests.
 *
 * Both are places where a mistake is invisible in a screenshot: a wrongly
 * parsed cookie just logs the user out on every launch, and a vague error
 * leaves them with nothing to do.
 */
class AgentGatewayTest {
    private val value43 = "A".repeat(43)

    @Test
    fun `gateway rejects non-loopback cleartext HTTP but allows HTTPS and localhost`() {
        assertThrows(IllegalArgumentException::class.java) {
            AgentGateway(AppConfig(gatewayBase = "http://gateway.example.com"))
        }
        AgentGateway(AppConfig(gatewayBase = "https://gateway.example.com"))
        AgentGateway(AppConfig(gatewayBase = "http://127.0.0.1:3101"))
        AgentGateway(AppConfig(gatewayBase = "http://[::1]:3101"))
    }

    @Test
    fun `togglePlugin parses single plugin response envelope`() {
        ServerSocket(0, 1, InetAddress.getByName("127.0.0.1")).use { server ->
            val port = server.localPort
            val worker =
                Thread {
                    server.accept().use { sock ->
                        val reader = sock.getInputStream().bufferedReader()
                        var contentLength = 0
                        while (true) {
                            val line = reader.readLine() ?: break
                            if (line.isEmpty()) break
                            if (line.startsWith("Content-Length:", ignoreCase = true)) {
                                contentLength = line.substringAfter(':').trim().toIntOrNull() ?: 0
                            }
                        }
                        if (contentLength > 0) {
                            val buf = CharArray(contentLength)
                            reader.read(buf, 0, contentLength)
                        }
                        val payload =
                            """{"plugin":{"id":"codex","name":"Codex","version":"1.0","description":"","category":"agent","enabled":false,"runtime_status":"disabled","error":null}}"""
                        val bodyBytes = payload.toByteArray()
                        val response =
                            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: ${bodyBytes.size}\r\nConnection: close\r\n\r\n"
                                .toByteArray() + bodyBytes
                        sock.getOutputStream().use { out ->
                            out.write(response)
                            out.flush()
                        }
                    }
                }
            worker.isDaemon = true
            worker.start()

            val gw =
                AgentGateway(
                    AppConfig(
                        gatewayBase = "http://127.0.0.1:$port",
                        accessToken = "tok",
                    ),
                )
            val plugin = gw.togglePlugin("codex", false)
            worker.join(3_000)
            assertNotNull(plugin)
            assertEquals("codex", plugin?.id)
            assertEquals(false, plugin?.enabled)
        }
    }

    @Test
    fun `sse line parser enforces line and data caps while accepting valid remote frames`() {
        val parser = SseLineParser()
        assertNull(parser.feedLine("event: remote"))
        assertNull(parser.feedLine("id: 42"))
        assertNull(parser.feedLine("data: " + "x".repeat(260 * 1024)))
        val lineOverflow = parser.feedLine("")
        assertNotNull(lineOverflow)
        assertEquals(true, lineOverflow?.overflowed)
        assertEquals(42L, lineOverflow?.cursor)
        assertNull(lineOverflow?.remoteEvent)

        assertNull(parser.feedLine("event: remote"))
        assertNull(parser.feedLine("id: 43"))
        repeat(5) {
            assertNull(parser.feedLine("data: " + "y".repeat(60 * 1024)))
        }
        val dataOverflow = parser.feedLine("")
        assertNotNull(dataOverflow)
        assertEquals(true, dataOverflow?.overflowed)
        assertEquals(43L, dataOverflow?.cursor)
        assertNull(dataOverflow?.remoteEvent)

        assertNull(parser.feedLine("event: remote"))
        assertNull(parser.feedLine("id: 44"))
        assertNull(parser.feedLine("""data: {"runner_id":"r-1"}"""))
        val valid = parser.feedLine("")
        assertNotNull(valid)
        assertEquals("remote", valid?.event)
        assertEquals(44L, valid?.cursor)
        assertEquals("r-1", valid?.runnerId)
        assertEquals(false, valid?.overflowed)
        assertTrue(valid != null)
    }

    // ------------------------------------------------------------ session cookie

    @Test
    fun `both cookie names are accepted`() {
        assertEquals(
            "agent_pulse_session=$value43",
            AgentGateway.sessionCookiePair("agent_pulse_session=$value43; Path=/; HttpOnly"),
        )
        assertEquals(
            "__Host-agent_pulse_session=$value43",
            AgentGateway.sessionCookiePair("__Host-agent_pulse_session=$value43; Secure; Path=/"),
        )
    }

    @Test
    fun `cookie attributes after the first semicolon are dropped`() {
        // Only name=value is replayed; sending the attributes back would be a
        // malformed Cookie header.
        assertEquals(
            "__Host-agent_pulse_session=$value43",
            AgentGateway.sessionCookiePair(
                "__Host-agent_pulse_session=$value43; Max-Age=2592000; Secure; HttpOnly; SameSite=Lax",
            ),
        )
    }

    @Test
    fun `an unrelated cookie is refused`() {
        assertNull(AgentGateway.sessionCookiePair("theme=dark; Path=/"))
        assertNull(AgentGateway.sessionCookiePair("agent_pulse_session_old=$value43; Path=/"))
    }

    @Test
    fun `a malformed value is refused rather than stored`() {
        // A short, padded, or attribute-laden value is not a session token; it
        // would produce an unauthorised request on every later call.
        assertNull(AgentGateway.sessionCookiePair("agent_pulse_session=short; Path=/"))
        assertNull(AgentGateway.sessionCookiePair("agent_pulse_session=${"A".repeat(44)}; Path=/"))
        assertNull(AgentGateway.sessionCookiePair("agent_pulse_session=${"A".repeat(42)}=; Path=/"))
        assertNull(AgentGateway.sessionCookiePair("agent_pulse_session=; Path=/"))
        assertNull(AgentGateway.sessionCookiePair("agent_pulse_session"))
    }

    @Test
    fun `a missing header is not a cookie`() {
        assertNull(AgentGateway.sessionCookiePair(null))
    }

    @Test
    fun `base64url characters are accepted`() {
        val urlSafe = "-_".repeat(10) + "abc" + "Z".repeat(20)
        assertEquals(
            "agent_pulse_session=$urlSafe",
            AgentGateway.sessionCookiePair("agent_pulse_session=$urlSafe; Path=/"),
        )
    }

    // ------------------------------------------------------------- error text

    @Test
    fun `every contract error code has an actionable sentence`() {
        // The interesting ones are states the user can change; none of them may
        // fall through to a bare status number.
        val codes =
            listOf(
                "authentication_required",
                "runner_offline",
                "session_control_lost",
                "host_generation_mismatch",
                "session_not_found",
                "approval_not_pending",
                "runner_backpressure",
                "device_not_found",
                "invalid_cursor",
                "stream_limit",
                "email_verification_required",
                "invalid_credentials",
                "login_rate_limited",
            )
        for (code in codes) {
            val message = describeGatewayError(GatewayException(400, code))
            assertEquals("missing text for $code", true, message.isNotBlank())
            assertEquals("bare code leaked for $code", false, message.contains(code))
        }
    }

    @Test
    fun `an unknown 5xx reads as a temporary outage, an unknown 4xx still names the code`() {
        assertEquals(
            "Gateway 暂时不可用，请稍后重试。",
            describeGatewayError(GatewayException(503, "something_new")),
        )
        assertEquals(
            "Gateway 返回错误（422 something_new）。",
            describeGatewayError(GatewayException(422, "something_new")),
        )
    }

    @Test
    fun `a network failure is described as connectivity, not as a bug`() {
        assertEquals(
            "无法连接 Gateway，请检查地址与网络。",
            describeGatewayError(IOException("connection reset")),
        )
    }

    @Test
    fun `sse line parser decodes remote frames and ignores heartbeats`() {
        val parser = SseLineParser()
        assertNull(parser.feedLine(": heartbeat"))
        assertNull(parser.feedLine(""))
        assertNull(parser.feedLine("id: 42"))
        assertNull(parser.feedLine("event: remote"))
        assertNull(parser.feedLine("""data: {"cursor":42,"runner_id":"runner-x","payload":{}}"""))

        val frame = parser.feedLine("")
        assertEquals(SseFrame(event = "remote", cursor = 42L, runnerId = "runner-x"), frame)
    }

    @Test
    fun `sse line parser decodes full remote event payloads above 4 KiB for zero-poll streaming`() {
        val parser = SseLineParser()
        val bigDelta = "a".repeat(6_000)
        assertNull(parser.feedLine("id: 77"))
        assertNull(parser.feedLine("event: remote"))
        assertNull(
            parser.feedLine(
                """data: {"cursor":77,"runner_id":"runner-fast","payload":{"type":"event","event":{"hub_session_id":"hub-1","kind":"item/agentMessage/delta","raw":{"params":{"delta":"$bigDelta"}}}}}""",
            ),
        )
        val frame = parser.feedLine("")
        assertNotNull(frame)
        assertEquals(77L, frame?.cursor)
        assertEquals("runner-fast", frame?.runnerId)
        assertNotNull(frame?.remoteEvent)
        assertEquals("event", frame?.remoteEvent?.payloadType)
        assertEquals("item/agentMessage/delta", frame?.remoteEvent?.eventKind)
        assertEquals(
            bigDelta,
            frame?.remoteEvent?.event?.optJSONObject("raw")?.optJSONObject("params")?.optString("delta"),
        )
    }

    @Test
    fun `a heartbeat comment is the first line a healthy stream sends`() {
        // `streamEvents` only reports the stream as live once a line arrives,
        // so the Gateway's 10s heartbeat has to come through the parser as a
        // no-op the caller can see. If the parser swallowed it silently the
        // first line would look like a stall, and a healthy socket would be
        // torn down and retried every heartbeat interval.
        val parser = SseLineParser()
        assertNull(parser.feedLine(": heartbeat"))
        assertNull(parser.feedLine("retry: 1000"))
        assertNull(parser.feedLine(""))
    }
}
