package com.agentplatform.mobile.ui

import androidx.compose.ui.graphics.Color
import com.agentplatform.mobile.R
import com.agentplatform.mobile.data.EngineKind
import com.agentplatform.mobile.data.HostDevice
import com.agentplatform.mobile.data.HostHello
import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class MobilePolishTest {
    @Test
    fun `workspaceChoiceSubtitle never exposes raw ws token hash and includes machine`() {
        val id = "ws-Desktop-ef85f944822e11f9"
        val withLabel =
            workspaceChoiceSubtitle(
                workspaceId = id,
                workspaceLabels = mapOf(id to "~/Desktop"),
                deviceName = "xiaotang",
                sessionCount = 4,
                duplicated = false,
            )
        assertEquals("xiaotang · ~/Desktop", withLabel)
        assertFalse(withLabel.contains("ef85f944822e11f9"))

        val legacyRunner =
            workspaceChoiceSubtitle(
                workspaceId = id,
                workspaceLabels = emptyMap(),
                deviceName = "xiaotang",
                sessionCount = 3,
                duplicated = false,
            )
        assertEquals("xiaotang · 3 个历史会话", legacyRunner)
        assertFalse(legacyRunner.contains("ws-Desktop"))

        val legacyDuplicate =
            workspaceChoiceSubtitle(
                workspaceId = id,
                workspaceLabels = emptyMap(),
                deviceName = "xiaotang",
                sessionCount = 0,
                duplicated = true,
            )
        assertEquals("xiaotang · 已授权目录 · #ef85", legacyDuplicate)
    }

    @Test
    fun `workspaceDisplayPath decodes triple underscore slugs when labels absent`() {
        assertEquals(
            "D: ClaudeSpace/Project/blog",
            workspaceDisplayPath("ws-D___ClaudeSpace___Project___blog-0123456789abcdef"),
        )
        assertNull(workspaceDisplayPath("ws-Desktop-ef85f944822e11f9"))
    }

    @Test
    fun `HostHello parses workspace_labels and buildWorkspaceSheetChoices sorts active first`() {
        val helloJson =
            JSONObject(
                """
                {
                  "host_generation": "gen-1",
                  "device_name": "xiaotang",
                  "workspaces": [
                    "ws-Desktop-ef85f944822e11f9",
                    "ws-Agent-Nexus-c3fb77078ae87450"
                  ],
                  "workspace_labels": {
                    "ws-Desktop-ef85f944822e11f9": "~/Desktop",
                    "ws-Agent-Nexus-c3fb77078ae87450": "D: ClaudeSpace/Project/Agent-Nexus"
                  },
                  "discovery": {
                    "candidates": [
                      {
                        "provider_session_id": "antigravity_1",
                        "workspace_path": "ws-Agent-Nexus-c3fb77078ae87450",
                        "title": "Recent task",
                        "last_activity_at_ms": 2000
                      }
                    ]
                  }
                }
                """.trimIndent(),
            )
        val hello = HostHello.parse(helloJson)
        assertNotNull(hello)
        assertEquals("~/Desktop", hello!!.workspaceLabels["ws-Desktop-ef85f944822e11f9"])

        val host = HostDevice(runnerId = "r1", online = true, revoked = false, hello = hello)
        val choices = buildWorkspaceSheetChoices(host, emptyList())
        assertEquals(2, choices.size)
        assertEquals("Agent-Nexus", choices[0].title)
        assertEquals("xiaotang · D: ClaudeSpace/Project/Agent-Nexus", choices[0].subtitle)
        assertEquals("1 会话", choices[0].badge)
        assertEquals("Desktop", choices[1].title)
        assertEquals("xiaotang · ~/Desktop", choices[1].subtitle)
    }

    @Test
    fun `resolveBrandIcon maps all native and plugin agents to authentic brand drawables`() {
        assertEquals(R.drawable.ic_brand_codex, resolveBrandIcon(EngineKind.CODEX).resId)
        assertEquals(R.drawable.ic_brand_claude_code, resolveBrandIcon(EngineKind.CLAUDE_CODE).resId)
        assertEquals(R.drawable.ic_brand_claude, resolveBrandIcon(EngineKind.CLAUDE_DESKTOP).resId)
        assertEquals(R.drawable.ic_brand_antigravity, resolveBrandIcon(EngineKind.ANTIGRAVITY).resId)
        assertEquals(R.drawable.ic_brand_deepseek, resolveBrandIcon(EngineKind.DEEPSEEK_HARNESS).resId)
        assertEquals(R.drawable.ic_brand_gemini_cli, resolveBrandIcon(EngineKind.PLUGIN, "gemini_cli").resId)
        val grok = resolveBrandIcon(EngineKind.PLUGIN, "grok")
        assertEquals(R.drawable.ic_brand_grok, grok.resId)
        assertTrue(grok.monochrome)
    }

    @Test
    fun `parseInlineMarkdown renders nested bold and inline code without exposing backticks or stars`() {
        val rendered =
            parseInlineMarkdown(
                text = "**`#6004`（你）**  **`1.0.0`**  `dingyu2006` (`瑜`)",
                codeBg = Color.LightGray,
                codeColor = Color.Blue,
                linkColor = Color.Blue,
            )
        val plain = rendered.text
        assertFalse("backticks leaked: $plain", plain.contains('`'))
        assertFalse("asterisks leaked: $plain", plain.contains("**"))
        assertTrue(plain.contains("#6004"))
        assertTrue(plain.contains("（你）"))
        assertTrue(plain.contains("1.0.0"))
        assertTrue(plain.contains("dingyu2006"))
        assertTrue(plain.contains("瑜"))
        assertTrue(rendered.spanStyles.isNotEmpty())
    }

    @Test
    fun `splitTableCells preserves escaped pipes and pipes inside inline code`() {
        val cells = splitTableCells("| **`#6004`（你）** | `a | b` | foo \\| bar |")
        assertEquals(listOf("**`#6004`（你）**", "`a | b`", "foo | bar"), cells)
    }

    @Test
    fun `formatToolActivity extracts clean summary for update_focus JSON and skips brace`() {
        val item =
            TranscriptItem(
                id = "t1",
                kind = TranscriptKind.TOOL,
                title = "update_focus",
                command = "{\n  \"focus\": \"Sampling recent Cangjie-Examples PRs to compare conventions\"\n}",
                output = "OK",
                state = ToolState.COMPLETED,
            )
        val formatted = formatToolActivity(item)
        assertEquals(ToolCategory.FOCUS, formatted.category)
        assertEquals("更新当前焦点", formatted.title)
        assertEquals("Sampling recent Cangjie-Examples PRs to compare conventions", formatted.subtitle)
        assertTrue(formatted.outputIsTrivialOk)
        assertEquals(1, formatted.params.size)
        assertEquals("当前焦点", formatted.params.first().label)
    }

    @Test
    fun `formatToolActivity skips PowerShell here-string opener for run_command`() {
        val item =
            TranscriptItem(
                id = "t2",
                kind = TranscriptKind.TOOL,
                title = "run_command: @'…",
                command = "@'\ngh pr list --limit 8\n'@",
                output = "#6004 ...",
                state = ToolState.COMPLETED,
            )
        val formatted = formatToolActivity(item)
        assertEquals(ToolCategory.COMMAND, formatted.category)
        assertEquals("运行命令", formatted.title)
        assertEquals("gh pr list --limit 8", formatted.subtitle)
        assertFalse(formatted.subtitle.orEmpty().startsWith("@'"))
    }

    @Test
    fun `routeTransitionDirection guarantees forward push and backward pop for all paths`() {
        assertEquals(RouteTransitionDirection.FORWARD, routeTransitionDirection(DEST_SESSIONS, "session"))
        assertEquals(RouteTransitionDirection.FORWARD, routeTransitionDirection(DEST_SESSIONS, "launch"))
        assertEquals(RouteTransitionDirection.FORWARD, routeTransitionDirection("launch", "session"))
        assertEquals(RouteTransitionDirection.FORWARD, routeTransitionDirection(DEST_SESSIONS, DEST_SETTINGS))
        assertEquals(RouteTransitionDirection.FORWARD, routeTransitionDirection(DEST_SETTINGS, DEST_HOSTS))
        assertEquals(RouteTransitionDirection.FORWARD, routeTransitionDirection(DEST_HOSTS, "launch"))
        assertEquals(RouteTransitionDirection.FORWARD, routeTransitionDirection(DEST_HOSTS, "session"))

        assertEquals(RouteTransitionDirection.BACKWARD, routeTransitionDirection("session", DEST_SESSIONS))
        assertEquals(RouteTransitionDirection.BACKWARD, routeTransitionDirection("launch", DEST_SESSIONS))
        assertEquals(RouteTransitionDirection.BACKWARD, routeTransitionDirection(DEST_HOSTS, DEST_SETTINGS))
        assertEquals(RouteTransitionDirection.BACKWARD, routeTransitionDirection(DEST_HOSTS, DEST_SESSIONS))
        assertEquals(RouteTransitionDirection.BACKWARD, routeTransitionDirection(DEST_SETTINGS, DEST_SESSIONS))
        assertEquals(RouteTransitionDirection.BACKWARD, routeTransitionDirection("launch", DEST_HOSTS))

        assertEquals(
            RouteTransitionDirection.CROSSFADE,
            routeTransitionDirection(DEST_SESSIONS, DEST_SETTINGS, useSideRail = true),
        )
        assertEquals(RouteTransitionDirection.CROSSFADE, routeTransitionDirection("gate", DEST_SESSIONS))
        assertEquals(RouteTransitionDirection.CROSSFADE, routeTransitionDirection(DEST_SESSIONS, "gate"))

        assertTrue(routeLayerRank("session") > routeLayerRank("launch"))
        assertTrue(routeLayerRank("launch") > routeLayerRank(DEST_HOSTS))
        assertTrue(routeLayerRank(DEST_HOSTS) > routeLayerRank(DEST_SETTINGS))
        assertTrue(routeLayerRank(DEST_SETTINGS) > routeLayerRank(DEST_SESSIONS))
    }
}
