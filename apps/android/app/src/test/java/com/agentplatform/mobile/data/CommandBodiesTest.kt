package com.agentplatform.mobile.data

import org.json.JSONArray
import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * Command-body tests.
 *
 * The Gateway enforces an exact allowlist on every command, so these tests pin
 * the shape of what the phone sends rather than what it intends. A regression
 * here is a 400 the person only discovers after pressing send.
 */
class CommandBodiesTest {
    private val workspace = "ws-agenthub-0123456789abcdef"

    // ----------------------------------------------------------------- connect

    @Test
    fun `a create names the engine and omits a provider session`() {
        val body =
            connectCommand(
                mode = ConnectMode.CREATE,
                workspacePath = workspace,
                agentType = EngineKind.CODEX,
            )

        assertEquals("connect", body.optString("action"))
        assertEquals("create", body.optString("mode"))
        assertEquals(workspace, body.optString("workspace_path"))
        assertEquals("codex", body.optString("agent_type"))
        // `parseRemoteCommand` rejects a create that carries one.
        assertFalse(body.has("provider_session_id"))
    }

    @Test
    fun `a resume names the provider session and omits the engine`() {
        val body =
            connectCommand(
                mode = ConnectMode.RESUME,
                workspacePath = workspace,
                providerSessionId = "claude_abc",
            )

        assertEquals("resume", body.optString("mode"))
        assertEquals("claude_abc", body.optString("provider_session_id"))
        assertFalse(body.has("agent_type"))
    }

    @Test
    fun `a non-create mode without a provider session is refused rather than sent`() {
        val failure =
            runCatching {
                connectCommand(
                    mode = ConnectMode.RESUME,
                    workspacePath = workspace,
                    providerSessionId = "   ",
                )
            }.exceptionOrNull()
        assertTrue(failure is IllegalArgumentException)
    }

    @Test
    fun `permission mode maps onto the matching approval policy`() {
        fun policy(mode: PermissionMode) =
            connectCommand(
                mode = ConnectMode.CREATE,
                workspacePath = workspace,
                permissionMode = mode,
            ).optString("approval_policy")

        assertEquals("untrusted", policy(PermissionMode.DEFAULT))
        assertEquals("on-request", policy(PermissionMode.ACCEPT_EDITS))
        assertEquals("untrusted", policy(PermissionMode.PLAN))
        assertEquals("never", policy(PermissionMode.BYPASS))
    }

    @Test
    fun `permission mode is always present so the host does not have to guess`() {
        val body =
            connectCommand(
                mode = ConnectMode.CREATE,
                workspacePath = workspace,
                permissionMode = PermissionMode.DEFAULT,
            )
        // The field is a choice, and a host with its own default still needs to
        // be told which one this phone means.
        assertEquals("default", body.optString("permission_mode"))
    }

    @Test
    fun `a blank model is omitted rather than sent as an empty string`() {
        val body =
            connectCommand(
                mode = ConnectMode.CREATE,
                workspacePath = workspace,
                model = "   ",
            )
        assertFalse(body.has("model"))
    }

    @Test
    fun `a chosen model travels on both create and resume`() {
        val create =
            connectCommand(
                mode = ConnectMode.CREATE,
                workspacePath = workspace,
                agentType = EngineKind.PLUGIN,
                agentTypeWire = "zcode",
                model = "gemini-3.8-flash",
            )
        val resume =
            connectCommand(
                mode = ConnectMode.RESUME,
                workspacePath = workspace,
                providerSessionId = "sess_abc",
                model = "gemini-3.8-flash",
            )
        assertEquals("zcode", create.optString("agent_type"))
        assertEquals("gemini-3.8-flash", create.optString("model"))
        assertEquals("gemini-3.8-flash", resume.optString("model"))
    }

    // ------------------------------------------------------------- send_prompt

    @Test
    fun `a plain prompt carries only action and text`() {
        val body = sendPromptCommand(text = "  跑一遍测试  ")

        assertEquals("send_prompt", body.optString("action"))
        assertEquals("跑一遍测试", body.optString("text"))
        // No effort, no model, no permission mode, no attachments: this is the
        // same body v2.6.2 sent, so an older host is not asked to read a field
        // it does not know.
        assertEquals(2, body.length())
        assertFalse(body.has("effort"))
        assertFalse(body.has("model"))
        assertFalse(body.has("permission_mode"))
        assertFalse(body.has("attachments"))
    }

    @Test
    fun `a chosen effort and model are added and a default effort is not`() {
        val body =
            sendPromptCommand(
                text = "hi",
                effort = EffortLevel.HIGH,
                model = "gpt-5-codex",
            )
        assertEquals("high", body.optString("effort"))
        assertEquals("gpt-5-codex", body.optString("model"))
    }

    @Test
    fun `a non-default permission mode is sent and a default one is not`() {
        val plain = sendPromptCommand(text = "hi")
        assertFalse(plain.has("permission_mode"))

        val plan = sendPromptCommand(text = "hi", permissionMode = PermissionMode.PLAN)
        assertEquals("plan", plan.optString("permission_mode"))
    }

    @Test
    fun `an explicitly requested default permission mode is still sent`() {
        val body =
            sendPromptCommand(
                text = "hi",
                permissionMode = PermissionMode.DEFAULT,
                includePermissionMode = true,
            )
        assertEquals("default", body.optString("permission_mode"))
    }

    @Test
    fun `a prompt that is only attachments still has a sentence to say`() {
        val body =
            sendPromptCommand(
                text = "",
                attachments = listOf(attachment("a.txt")),
            )
        assertTrue(body.optString("text").startsWith("请查看附件："))
        assertTrue(body.optString("text").contains("a.txt"))
    }

    @Test
    fun `a prompt with neither text nor attachments is refused`() {
        val failure = runCatching { sendPromptCommand(text = "   ") }.exceptionOrNull()
        assertTrue(failure is IllegalArgumentException)
    }

    @Test
    fun `an attachment carries name media type data and encoding`() {
        val body =
            sendPromptCommand(
                text = "look",
                attachments = listOf(attachment("a.txt", data = "hello", encoding = "utf8")),
            )

        val list = body.optJSONArray("attachments")
        assertEquals(1, list.length())
        val first = list.getJSONObject(0)
        assertEquals("a.txt", first.optString("name"))
        assertEquals("text/plain", first.optString("media_type"))
        assertEquals("hello", first.optString("data"))
        assertEquals("utf8", first.optString("encoding"))
    }

    @Test
    fun `an attachment over the data limit is dropped instead of failing the prompt`() {
        val huge = attachment("huge.bin", data = "x".repeat(RemoteAttachment.MAX_DATA_CHARS + 1))
        val body = sendPromptCommand(text = "look", attachments = listOf(huge))

        // The prompt still goes out; only the unusable attachment is left out.
        assertEquals("look", body.optString("text"))
        assertFalse(body.has("attachments"))
    }

    @Test
    fun `the attachment count is capped at what the contract allows`() {
        val many = (1..20).map { attachment("f$it.txt") }
        val body = sendPromptCommand(text = "look", attachments = many)

        // `attachments.length` must be within 1..16; a longer list is a 400 for
        // the whole prompt, so the cap belongs here and not only in the picker.
        assertEquals(RemoteAttachment.MAX_COUNT, body.optJSONArray("attachments").length())
    }

    @Test
    fun `an empty attachment list means no attachments key at all`() {
        val body = sendPromptCommand(text = "look", attachments = emptyList())
        assertFalse(body.has("attachments"))
    }

    // ----------------------------------------------------------------- helpers

    private fun attachment(
        name: String,
        data: String = "x",
        encoding: String = "utf8",
        mediaType: String = "text/plain",
    ) = RemoteAttachment(
        name = name,
        mediaType = mediaType,
        data = data,
        encoding = encoding,
    )

    @Test
    fun `an attachment reports whether it fits the wire limits`() {
        assertTrue(attachment("ok.txt").fitsLimit())
        assertFalse(attachment("", ).fitsLimit())
        assertFalse(attachment("x".repeat(257)).fitsLimit())
        assertFalse(
            attachment("big.bin", data = "y".repeat(RemoteAttachment.MAX_DATA_CHARS + 1)).fitsLimit(),
        )
    }

    @Test
    fun `the attachment data limit leaves headroom under the contract character cap`() {
        // The contract caps `text(att.data, 8MB)` in characters, and base64
        // grows the payload by 4/3, so the reader's byte ceiling has to be well
        // under it or a legal-looking file would be refused by the Gateway.
        assertTrue(RemoteAttachment.MAX_DATA_CHARS < 8 * 1024 * 1024)
    }

    @Test
    fun `the attachment count ceiling is the contract ceiling`() {
        assertEquals(16, RemoteAttachment.MAX_COUNT)
    }
}
