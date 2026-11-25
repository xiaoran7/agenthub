package com.agentplatform.mobile.data

import java.io.StringReader
import java.net.InetAddress
import java.net.ServerSocket
import java.security.InvalidKeyException
import java.security.ProviderException
import java.security.UnrecoverableKeyException
import javax.crypto.AEADBadTagException
import org.json.JSONArray
import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotEquals
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertThrows
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * Targeted regression tests for the full-code-review data-layer fixes:
 * Keystore recovery (AND-H1), bounded SSE framing & overflow signaling (AND-H2),
 * generation-scoped command ledger & permanent 4xx handling (AND-H5 / M4),
 * top-level session provider (M6), JSON null hygiene (M7), and L3-L7 fixes.
 */
class ReviewFixesDataTest {
    // -------------------------------------------------------------- AND-H1

    @Test
    fun `keystore encryption retries once after resetting key on recoverable key failures`() {
        for (failure in listOf(
            InvalidKeyException("invalidated"),
            UnrecoverableKeyException("wiped"),
        )) {
            var resets = 0
            var attempts = 0
            val result =
                encryptWithSingleKeyReset(resetKey = { resets += 1 }) {
                    attempts += 1
                    if (attempts == 1) throw failure
                    "encrypted-ok"
                }
            assertEquals("encrypted-ok", result)
            assertEquals(1, resets)
            assertEquals(2, attempts)
        }
    }

    @Test
    fun `transient ProviderException does not reset master key during encryption`() {
        assertFalse(isRecoverableKeyFailure(ProviderException("transient binder failure")))
        var resets = 0
        assertThrows(ProviderException::class.java) {
            encryptWithSingleKeyReset(resetKey = { resets += 1 }) {
                throw ProviderException("keystore temporarily unavailable")
            }
        }
        assertEquals(0, resets)
    }

    @Test
    fun `keystore encryption never falls back to plaintext if retry fails or error is non-key`() {
        var resets = 0
        assertThrows(InvalidKeyException::class.java) {
            encryptWithSingleKeyReset(resetKey = { resets += 1 }) {
                throw InvalidKeyException("permanently broken")
            }
        }
        assertEquals(1, resets)

        resets = 0
        assertThrows(IllegalStateException::class.java) {
            encryptWithSingleKeyReset(resetKey = { resets += 1 }) {
                throw IllegalStateException("unexpected")
            }
        }
        assertEquals(0, resets)
    }

    @Test
    fun `load failure classification clears and resets key only on permanent key corruption`() {
        assertTrue(shouldClearBlobOnLoadFailure(AEADBadTagException("bad tag")))
        assertFalse(shouldResetKeyOnLoadFailure(AEADBadTagException("bad tag")))

        assertTrue(shouldClearBlobOnLoadFailure(InvalidKeyException("key invalidated")))
        assertTrue(shouldResetKeyOnLoadFailure(InvalidKeyException("key invalidated")))

        assertTrue(shouldClearBlobOnLoadFailure(UnrecoverableKeyException("key missing")))
        assertTrue(shouldResetKeyOnLoadFailure(UnrecoverableKeyException("key missing")))

        assertFalse(shouldClearBlobOnLoadFailure(RuntimeException("transient binder ipc")))
        assertFalse(shouldResetKeyOnLoadFailure(RuntimeException("transient binder ipc")))
    }

    // -------------------------------------------------------------- AND-H2

    @Test
    fun `bounded line reader caps oversized lines and drains remainder cleanly`() {
        val oversized = "data: " + "z".repeat(120_000)
        val stream = "$oversized\r\n\r\nevent: remote\nid: 9\ndata: {\"runner_id\":\"r1\"}\r\n\r\n"
        val reader = BoundedLineReader(StringReader(stream), maxChars = 128)

        val first = reader.readLine()
        assertNotNull(first)
        assertEquals(129, first!!.length)
        assertTrue(first.startsWith("data: zzz"))
        assertEquals("", reader.readLine())
        assertEquals("event: remote", reader.readLine())
        assertEquals("id: 9", reader.readLine())
        assertEquals("""data: {"runner_id":"r1"}""", reader.readLine())
        assertEquals("", reader.readLine())
        assertNull(reader.readLine())
    }

    @Test
    fun `sse line parser emits overflowed frame even when first line overflows`() {
        val parser = SseLineParser()
        assertNull(parser.feedLine("data: " + "x".repeat(SseLineParser.MAX_SSE_LINE_CHARS + 1)))
        val frame = parser.feedLine("")
        assertNotNull(frame)
        assertTrue(frame!!.overflowed)
        assertNull(frame.remoteEvent)
    }

    @Test
    fun `streamEvents emits overflowed frame for oversized SSE line and continues to next valid frame`() {
        ServerSocket(0, 1, InetAddress.getByName("127.0.0.1")).use { server ->
            val port = server.localPort
            val worker =
                Thread {
                    server.accept().use { sock ->
                        val reader = sock.getInputStream().bufferedReader()
                        while (true) {
                            val line = reader.readLine() ?: break
                            if (line.isEmpty()) break
                        }
                        val huge = "x".repeat(SseLineParser.MAX_SSE_LINE_CHARS + 512)
                        val body =
                            "id: 10\nevent: remote\ndata: $huge\n\n" +
                                "id: 11\nevent: remote\ndata: {\"cursor\":11,\"runner_id\":\"r1\",\"payload\":{\"type\":\"event\",\"event\":{\"kind\":\"turn/started\"}}}\n\n"
                        val resp =
                            ("HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n" + body)
                                .toByteArray()
                        sock.getOutputStream().use { out ->
                            out.write(resp)
                            out.flush()
                        }
                    }
                }
            worker.isDaemon = true
            worker.start()

            val gw = AgentGateway(AppConfig("http://127.0.0.1:$port", accessToken = "tok"))
            val frames = mutableListOf<SseFrame>()
            gw.streamEvents(after = 9L, onConnected = {}, onFrame = { frames += it })
            worker.join(3_000)

            assertEquals(2, frames.size)
            assertTrue(frames[0].overflowed)
            assertEquals(10L, frames[0].cursor)
            assertFalse(frames[1].overflowed)
            assertEquals(11L, frames[1].cursor)
            assertEquals("turn/started", frames[1].remoteEvent?.eventKind)
        }
    }

    // -------------------------------------------------------------- AND-H5 / M4

    @Test
    fun `command ledger scopes identical payloads by host generation and supports cached hash forget`() {
        val ledger = CommandLedger()
        val payload = """{"action":"interrupt"}"""
        val h1 = ledger.computeHash("r1/h1/gen-1", "interrupt", payload)
        val h2 = ledger.computeHash("r1/h1/gen-2", "interrupt", payload)
        assertNotEquals(h1, h2)

        val k1 = ledger.keyForHash(h1)
        val k2 = ledger.keyForHash(h2)
        assertNotEquals(k1, k2)
        assertEquals(k1, ledger.keyForHash(h1))

        ledger.forgetHashIfPermanent(h1, status = 400, code = "host_generation_mismatch")
        assertEquals(k1, ledger.keyForHash(h1))

        ledger.forgetHashIfPermanent(h1, status = 400, code = "invalid_model")
        assertNotEquals(k1, ledger.keyForHash(h1))

        ledger.forgetHash(h2)
        assertNotEquals(k2, ledger.keyForHash(h2))
    }

    // -------------------------------------------------------------- M6 / M7 / L6

    @Test
    fun `remote session row parses top-level provider and ignores json null`() {
        val row =
            RemoteSessionRow.parse(
                JSONObject()
                    .put("runner_id", "r1")
                    .put("online", true)
                    .put("provider", "antigravity")
                    .put(
                        "binding",
                        JSONObject()
                            .put("hub_session_id", "hub-1")
                            .put("provider_session_id", "opaque-1")
                            .put("workspace_path", "/repo")
                            .put("host_generation", "gen-1")
                            .put("control_connected", true)
                            .put("execution_state", "idle")
                            .put("title", JSONObject.NULL),
                    ),
            )
        assertNotNull(row)
        assertEquals("antigravity", row?.provider)
        assertEquals("", row?.binding?.title)

        val nullProviderRow =
            RemoteSessionRow.parse(
                JSONObject()
                    .put("runner_id", "r1")
                    .put("online", true)
                    .put("provider", JSONObject.NULL)
                    .put(
                        "binding",
                        JSONObject()
                            .put("hub_session_id", "hub-2")
                            .put("provider_session_id", "opaque-2")
                            .put("workspace_path", "/repo")
                            .put("host_generation", "gen-1")
                            .put("control_connected", true)
                            .put("execution_state", "idle"),
                    ),
            )
        assertNull(nullProviderRow?.provider)
    }

    @Test
    fun `json null fields do not stringify to literal null in models`() {
        val plugin =
            AgentPlugin.parse(
                JSONObject()
                    .put("id", "zcode")
                    .put("name", JSONObject.NULL)
                    .put("version", JSONObject.NULL)
                    .put("description", JSONObject.NULL)
                    .put("category", JSONObject.NULL),
            )
        assertNotNull(plugin)
        assertEquals("zcode", plugin?.name)
        assertEquals("1.0.0", plugin?.version)
        assertEquals("", plugin?.description)
        assertEquals("agent", plugin?.category)

        val account =
            AccountView.parse(
                JSONObject()
                    .put(
                        "user",
                        JSONObject()
                            .put("user_id", "u1")
                            .put("email", "t@example.com")
                            .put("display_name", JSONObject.NULL),
                    )
                    .put("workspace", JSONObject().put("name", JSONObject.NULL))
                    .put("subscription", JSONObject().put("plan", JSONObject.NULL)),
            )
        assertEquals("t@example.com", account.displayName)
        assertNull(account.workspaceName)
        assertNull(account.plan)
    }

    @Test
    fun `plugin capabilities defaults match backend normalization`() {
        val defaults = PluginCapabilities.parse(null)
        assertTrue(defaults.discover)
        assertTrue(defaults.launch)
        assertTrue(defaults.resume)
        assertTrue(defaults.models)
        assertTrue(defaults.history)
        assertFalse(defaults.approvals)

        val custom =
            PluginCapabilities.parse(
                JSONObject()
                    .put("launch", false)
                    .put("approvals", true),
            )
        assertTrue(custom.discover)
        assertFalse(custom.launch)
        assertTrue(custom.resume)
        assertTrue(custom.models)
        assertTrue(custom.history)
        assertTrue(custom.approvals)
    }

    // -------------------------------------------------------------- L3 / L4 / L5 / L7

    @Test
    fun `submitCommand fails fast on blank hostGeneration and backend error codes are mapped`() {
        val gw = AgentGateway(AppConfig("http://127.0.0.1:1", accessToken = "tok"))
        val errNull =
            assertThrows(GatewayException::class.java) {
                gw.submitCommand("r1", "h1", null, "cmd-1", JSONObject().put("action", "interrupt"))
            }
        assertEquals(400, errNull.status)
        assertEquals("host_generation_required", errNull.code)

        val errBlank =
            assertThrows(GatewayException::class.java) {
                gw.submitCommand("r1", "h1", "   ", "cmd-2", JSONObject().put("action", "interrupt"))
            }
        assertEquals("host_generation_required", errBlank.code)

        val codes =
            listOf(
                "forbidden",
                "workspace_permission_denied",
                "plugin_permission_denied",
                "command_id_conflict",
                "body_too_large",
                "payload_too_large",
                "host_generation_required",
                "runner_not_ready",
                "provider_rejected",
                "plugin_not_found",
                "plugin_action_timeout",
                "plugin_sha256_mismatch",
                "antigravity_not_available",
                "antigravity_session_not_found",
            )
        for (code in codes) {
            val text = describeGatewayError(GatewayException(400, code))
            assertTrue("missing message for $code", text.isNotBlank())
            assertFalse("raw code leaked for $code", text.contains(code))
        }
    }

    @Test
    fun `remote attachment assigns distinct stable ids per instance`() {
        val a = RemoteAttachment("same.txt", "text/plain", "hello", "utf8")
        val b = RemoteAttachment("same.txt", "text/plain", "hello", "utf8")
        assertTrue(a.id.isNotBlank())
        assertNotEquals(a.id, b.id)
    }

    @Test
    fun `fetchEventsPage honors truncated flag and login parses lowercase set-cookie header`() {
        val cookieValue = "B".repeat(43)
        ServerSocket(0, 1, InetAddress.getByName("127.0.0.1")).use { server ->
            val port = server.localPort
            val worker =
                Thread {
                    repeat(2) {
                        server.accept().use { sock ->
                            val reader = sock.getInputStream().bufferedReader()
                            val reqLine = reader.readLine().orEmpty()
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
                            val resp =
                                if (reqLine.startsWith("GET /v3/remote/events")) {
                                    val body = """{"events":[],"truncated":true}"""
                                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: ${body.toByteArray().size}\r\nConnection: close\r\n\r\n$body"
                                } else {
                                    val body = """{"user":{"user_id":"u1","email":"t@example.com"}}"""
                                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nset-cookie: agent_pulse_session=$cookieValue; Path=/; HttpOnly\r\nContent-Length: ${body.toByteArray().size}\r\nConnection: close\r\n\r\n$body"
                                }
                            sock.getOutputStream().use { out ->
                                out.write(resp.toByteArray())
                                out.flush()
                            }
                        }
                    }
                }
            worker.isDaemon = true
            worker.start()

            val gw = AgentGateway(AppConfig("http://127.0.0.1:$port", accessToken = "tok"))
            val page = gw.fetchEventsPage("r1", "h1", after = 50L)
            assertTrue(page.events.isEmpty())
            assertTrue(page.truncated)

            val login = gw.login("t@example.com", "secret")
            assertEquals("agent_pulse_session=$cookieValue", login.sessionCookie)
            assertEquals("u1", login.account.userId)
            worker.join(3_000)
        }
    }

    // -------------------------------------------------------------- AND-P1-2 / P1-4 / P2-6 / P2-7 / P3-1

    @Test
    fun `decodesAsUtf8 rejects short invalid binary payloads`() {
        assertFalse(AttachmentReader.isText("bom.txt", "text/plain", byteArrayOf(0xFF.toByte(), 0xFE.toByte())))
        assertFalse(AttachmentReader.isText("single.txt", "text/plain", byteArrayOf(0x80.toByte())))
        assertFalse(
            AttachmentReader.isText(
                "bad3.txt",
                "text/plain",
                byteArrayOf(0xE2.toByte(), 0x82.toByte(), 0xFF.toByte()),
            ),
        )
        assertTrue(AttachmentReader.isText("ok.txt", "text/plain", "hello".toByteArray()))
    }

    @Test
    fun `uppercase HTTPS gateway scheme is accepted by validation`() {
        val upper = AppConfig("HTTPS://gateway.example.com", accessToken = "tok")
        assertNull(upper.gatewayError())
        assertNull(upper.validationError())

        val loopbackUpper = AppConfig("HTTP://127.0.0.1:3001", developmentUserId = "dev-user")
        assertNull(loopbackUpper.gatewayError())
        assertNull(loopbackUpper.validationError())
    }

    @Test
    fun `explicit json null in runner snapshot string arrays and events never stringifies to null`() {
        val snap =
            RunnerPluginSnapshot.parse(
                JSONObject()
                    .put("runner", JSONObject.NULL)
                    .put("runner_id", "r-real")
                    .put("online", true),
            )
        assertEquals("r-real", snap?.runnerId)
        assertNull(
            RunnerPluginSnapshot.parse(
                JSONObject().put("runner", JSONObject.NULL).put("runner_id", JSONObject.NULL),
            ),
        )

        val hello =
            HostHello.parse(
                JSONObject()
                    .put("host_generation", JSONObject.NULL)
                    .put("workspaces", JSONArray().put("/repo").put(JSONObject.NULL).put("")),
            )
        assertEquals("", hello?.hostGeneration)
        assertEquals(listOf("/repo"), hello?.workspaces)

        val ev =
            RemoteEvent.parse(
                JSONObject()
                    .put("cursor", 1)
                    .put("runner_id", JSONObject.NULL)
                    .put("payload", JSONObject().put("type", JSONObject.NULL)),
            )
        assertEquals("", ev.runnerId)
        assertEquals("", ev.payloadType)
    }

    @Test
    fun `streamEvents exits immediately when shouldContinue is false and SSE limits align`() {
        assertEquals(SseLineParser.MAX_SSE_DATA_CHARS, SseLineParser.MAX_SSE_LINE_CHARS)
        val gw = AgentGateway(AppConfig("http://127.0.0.1:1", accessToken = "tok"))
        var connected = false
        val frames = mutableListOf<SseFrame>()
        gw.streamEvents(
            after = 0L,
            onConnected = { connected = it },
            onFrame = { frames += it },
            shouldContinue = { false },
        )
        assertFalse(connected)
        assertTrue(frames.isEmpty())
    }
}
